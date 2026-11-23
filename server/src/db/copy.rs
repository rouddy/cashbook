//! DB 사이 데이터 복사 (SQLite ↔ MySQL): `cashbook copy-db <원본 URL> <대상 URL>`
//!
//! - 두 DB 모두 마이그레이션을 적용한 뒤(원본도 밀린 마이그레이션이 있으면 적용됨) 테이블을 외래 키 순서대로 옮긴다.
//! - id를 그대로 옮기므로 거래-혜택 연결, 반복 규칙, API 토큰이 유지된다. (MySQL AUTO_INCREMENT와 SQLite
//!   AUTOINCREMENT는 명시한 id보다 큰 값부터 다시 매긴다)
//! - 대상은 비어 있어야 한다 (실수로 기존 데이터를 덮지 않도록). 한 트랜잭션으로 넣어 실패하면 아무것도 남지 않는다.
//! - 원본도 한 트랜잭션 안에서 읽으므로 서버가 실행 중이어도 한 시점의 일관된 내용이 복사된다 (백업에 사용 가능).

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    TransactionTrait,
};

use super::connect;
use crate::entity::{
    account, account_balance, api_token, card_benefit, card_tier, category, ledger, payment_method, plan_item,
    recurring_rule, recurring_rule_benefit, transaction, transaction_benefit, user,
};

/// 한 번에 넣는 행 수 (열 20개 × 500 = 1만 개 파라미터: SQLite 32766, MySQL 65535 한도 아래)
const CHUNK: usize = 500;

async fn copy_entity<E, S, D>(src: &S, dst: &D) -> Result<u64, DbErr>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel>,
    E::ActiveModel: ActiveModelTrait<Entity = E> + Send,
    S: ConnectionTrait,
    D: ConnectionTrait,
{
    let rows = E::find().all(src).await?;
    let total = rows.len() as u64;
    let mut rows = rows.into_iter();
    loop {
        let chunk: Vec<E::ActiveModel> = rows.by_ref().take(CHUNK).map(IntoActiveModel::into_active_model).collect();
        if chunk.is_empty() {
            break;
        }
        E::insert_many(chunk).exec_without_returning(dst).await?;
    }
    Ok(total)
}

/// 부모 테이블부터 (외래 키 순서). 계정(`users`)은 맨 앞에 따로 옮긴다.
macro_rules! for_each_table {
    ($m:ident) => {
        $m!(ledger, "ledgers");
        $m!(category, "categories");
        $m!(account, "accounts");
        $m!(account_balance, "account_balances");
        $m!(payment_method, "payment_methods");
        $m!(card_benefit, "card_benefits");
        $m!(card_tier, "card_tiers");
        $m!(recurring_rule, "recurring_rules");
        $m!(recurring_rule_benefit, "recurring_rule_benefits");
        $m!(transaction, "transactions");
        $m!(transaction_benefit, "transaction_benefits");
        $m!(plan_item, "plan_items");
        $m!(api_token, "api_tokens");
    };
}

/// URL에서 비밀번호를 가린 표시용 문자열
pub fn redact(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(at)) if at > s + 3 => {
            let creds = &url[s + 3..at];
            match creds.find(':') {
                Some(c) => format!("{}{}:***{}", &url[..s + 3], &creds[..c], &url[at..]),
                None => url.to_string(),
            }
        }
        _ => url.to_string(),
    }
}

pub async fn run(from: &str, to: &str) -> Result<Vec<(&'static str, u64)>, DbErr> {
    if from.trim() == to.trim() {
        return Err(DbErr::Custom("원본과 대상이 같습니다".into()));
    }
    let src = connect(from).await?;
    let dst = connect(to).await?;

    let mut not_empty = Vec::new();
    macro_rules! check_empty {
        ($module:ident, $name:literal) => {
            let n = $module::Entity::find().count(&dst).await?;
            if n > 0 {
                not_empty.push(format!("{} {}행", $name, n));
            }
        };
    }
    for_each_table!(check_empty);
    // 마이그레이션이 만든 기본 계정(아무도 넘겨받지 않은 계정)만 있으면 빈 DB로 본다
    let claimed = user::Entity::find().filter(user::Column::GoogleSub.is_not_null()).count(&dst).await?;
    if claimed > 0 {
        not_empty.push(format!("users {claimed}행"));
    }
    if !not_empty.is_empty() {
        return Err(DbErr::Custom(format!(
            "대상 DB가 비어 있지 않습니다 ({}). 새 DB(마이그레이션만 적용된 상태)로 복사하세요",
            not_empty.join(", ")
        )));
    }

    let reader = src.begin().await?;
    let writer = dst.begin().await?;
    let mut stats = Vec::new();
    user::Entity::delete_many().exec(&writer).await?;
    stats.push(("users", copy_entity::<user::Entity, _, _>(&reader, &writer).await?));
    macro_rules! copy_table {
        ($module:ident, $name:literal) => {
            stats.push(($name, copy_entity::<$module::Entity, _, _>(&reader, &writer).await?));
        };
    }
    for_each_table!(copy_table);
    writer.commit().await?;
    reader.commit().await?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::enums::{TxSource, TxStatus};
    use chrono::{NaiveDate, Utc};
    use sea_orm::Set;

    #[test]
    fn redacts_password() {
        assert_eq!(redact("mysql://cash:s3cr3t@db:3306/cashbook"), "mysql://cash:***@db:3306/cashbook");
        assert_eq!(redact("sqlite:///data/cashbook.db?mode=rwc"), "sqlite:///data/cashbook.db?mode=rwc");
        assert_eq!(redact("mysql://root@db/cashbook"), "mysql://root@db/cashbook");
    }

    #[tokio::test]
    async fn copies_rows_with_ids_and_refuses_non_empty_target() {
        let dir = std::env::temp_dir().join(format!("cashbook-copy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a.db"), dir.join("b.db"));
        let url = |p: &std::path::Path| format!("sqlite://{}?mode=rwc", p.display());

        let src = connect(&url(&a)).await.unwrap();
        user::ActiveModel {
            id: Set(5),
            google_sub: Set(Some("google-5".into())),
            email: Set(Some("b@example.com".into())),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&src)
        .await
        .unwrap();
        let ledger = ledger::ActiveModel {
            user_id: Set(1),
            id: Set(7),
            name: Set("가계부".into()),
            description: Set(None),
            is_default: Set(true),
            sort_order: Set(0),
            created_at: Set(Utc::now()),
        }
        .insert(&src)
        .await
        .unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        for id in [3, 11] {
            transaction::ActiveModel {
                user_id: Set(1),
                id: Set(id),
                ledger_id: Set(ledger.id),
                date: Set(today),
                amount: Set(1000 * id as i64),
                description: Set(format!("거래{id}")),
                status: Set(TxStatus::Confirmed),
                installment_months: Set(1),
                exclude_from_performance: Set(false),
                source: Set(TxSource::Manual),
                raw_text: Set(Some("긴 원문 ".repeat(100))),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
                ..Default::default()
            }
            .insert(&src)
            .await
            .unwrap();
        }
        drop(src);

        let stats = run(&url(&a), &url(&b)).await.unwrap();
        assert!(stats.contains(&("users", 2)) && stats.contains(&("ledgers", 1)) && stats.contains(&("transactions", 2)));
        let dst = connect(&url(&b)).await.unwrap();
        let mut ids: Vec<i32> = transaction::Entity::find().all(&dst).await.unwrap().iter().map(|t| t.id).collect();
        ids.sort();
        assert_eq!(ids, vec![3, 11]);
        // 다음 id는 옮긴 id 뒤부터
        let next = transaction::ActiveModel {
            user_id: Set(1),
            ledger_id: Set(7),
            date: Set(today),
            amount: Set(1),
            description: Set("새 거래".into()),
            status: Set(TxStatus::Confirmed),
            installment_months: Set(1),
            exclude_from_performance: Set(false),
            source: Set(TxSource::Manual),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&dst)
        .await
        .unwrap();
        assert!(next.id > 11);
        drop(dst);

        // 대상이 비어 있지 않으면 거부
        let err = run(&url(&a), &url(&b)).await.unwrap_err().to_string();
        assert!(err.contains("비어 있지 않습니다"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
