//! 거래 분할: 한 결제를 여러 장부에 나눠 담는다 (예: 카드 결제 한 건을 가계부와 용돈에).
//!
//! 나눠 옮긴 거래(자식)는 `split_parent_id`로 원래 거래(부모)를 가리키고, 부모와 자식 금액의 합이 실제 결제 금액이다.
//! - 결제 자체의 속성(날짜·결제수단·할부·실적 제외·상태)은 묶음 안에서 항상 같게 맞춘다.
//! - 카드 혜택 지정은 부모에 두고, 카드 실적·혜택은 묶음을 한 건으로 계산한다(`card_report`).
//! - 확정 거래만 나눈다. 예정 거래는 반복 규칙이 지우고 다시 만들 수 있어서 짝이 깨진다.
//! - 묶음의 거래를 지우거나 상태를 바꾸려면 먼저 합친다(`merge`).

use std::collections::HashMap;
use std::fmt;

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::entity::{category, enums::TxStatus, ledger, transaction, transaction_benefit};

#[derive(Debug)]
pub enum SplitError {
    Invalid(String),
    Db(DbErr),
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::Invalid(msg) => write!(f, "{msg}"),
            SplitError::Db(e) => write!(f, "DB 오류: {e}"),
        }
    }
}

impl std::error::Error for SplitError {}

impl From<DbErr> for SplitError {
    fn from(e: DbErr) -> Self {
        SplitError::Db(e)
    }
}

fn invalid(msg: impl Into<String>) -> SplitError {
    SplitError::Invalid(msg.into())
}

/// 분할 묶음: 원래 거래(부모)와 나눠 옮긴 거래들(자식, id 순)
#[derive(Debug, Clone)]
pub struct SplitGroup {
    pub parent: transaction::Model,
    pub children: Vec<transaction::Model>,
}

impl SplitGroup {
    /// 실제 결제 금액
    pub fn total(&self) -> i64 {
        self.parent.amount + self.children.iter().map(|c| c.amount).sum::<i64>()
    }

    pub fn members(&self) -> impl Iterator<Item = &transaction::Model> {
        std::iter::once(&self.parent).chain(self.children.iter())
    }
}

/// 거래가 속한 분할 묶음. 분할되지 않은 거래(또는 부모가 없는 자식)면 None.
pub async fn group_of<C: ConnectionTrait>(db: &C, tx: &transaction::Model) -> Result<Option<SplitGroup>, DbErr> {
    let parent = match tx.split_parent_id {
        Some(pid) => match transaction::Entity::find_by_id(pid).one(db).await? {
            Some(p) => p,
            None => return Ok(None),
        },
        None => tx.clone(),
    };
    let children = transaction::Entity::find()
        .filter(transaction::Column::SplitParentId.eq(parent.id))
        .order_by_asc(transaction::Column::Id)
        .all(db)
        .await?;
    if children.is_empty() {
        return Ok(None);
    }
    Ok(Some(SplitGroup { parent, children }))
}

/// `txs` 가운데 분할 묶음에 속한 거래(부모 또는 자식)가 있는지
pub async fn any_split<C: ConnectionTrait>(db: &C, txs: &[transaction::Model]) -> Result<bool, DbErr> {
    if txs.iter().any(|t| t.split_parent_id.is_some()) {
        return Ok(true);
    }
    let ids: Vec<i32> = txs.iter().map(|t| t.id).collect();
    let child = transaction::Entity::find()
        .filter(transaction::Column::SplitParentId.is_in(ids))
        .one(db)
        .await?;
    Ok(child.is_some())
}

/// 다른 장부에서 같은 항목: 이름·종류가 같고 보관하지 않은 카테고리 (여럿이면 정렬 순서가 앞선 것)
pub fn same_category_in(categories: &HashMap<i32, category::Model>, ledger_id: i32, source: &category::Model) -> Option<i32> {
    categories
        .values()
        .filter(|t| t.ledger_id == ledger_id && !t.archived && t.name == source.name && t.kind == source.kind)
        .min_by_key(|t| (t.sort_order, t.id))
        .map(|t| t.id)
}

/// `total`에서 `part`를 떼어낼 수 있는지: 둘 다 0이 아니고 부호가 같으며 남는 쪽도 0이 아니어야 한다.
pub fn check_part(total: i64, part: i64) -> Result<(), String> {
    if total == 0 {
        return Err("0원 거래는 나눌 수 없습니다".into());
    }
    if part == 0 || part.signum() != total.signum() || part.abs() >= total.abs() {
        return Err(if total > 0 {
            format!("나눌 금액은 1원 이상, 결제 금액 {total}원보다 작아야 합니다")
        } else {
            format!("환불을 나눌 때는 금액도 음수로, 절댓값이 {}원보다 작아야 합니다", -total)
        });
    }
    Ok(())
}

/// 묶음 안 거래 `id`의 금액을 `new_amount`로 바꿀 때 결제 합계가 그대로이도록 짝 거래의 새 금액을 정한다.
/// 자식을 고치면 부모가, 부모를 고치면 마지막 자식이 차액을 맡는다. 바뀌는 게 없으면 None.
pub fn rebalance(group: &SplitGroup, id: i32, new_amount: i64) -> Result<Option<(i32, i64)>, String> {
    let Some(current) = group.members().find(|t| t.id == id) else {
        return Err("분할 묶음에 없는 거래입니다".into());
    };
    if current.amount == new_amount {
        return Ok(None);
    }
    let partner = if id == group.parent.id { group.children.last().unwrap_or(&group.parent) } else { &group.parent };
    let partner_amount = partner.amount + current.amount - new_amount;
    let total = group.total();
    if [new_amount, partner_amount].iter().any(|v| *v == 0 || v.signum() != total.signum()) {
        return Err(format!(
            "분할된 거래는 결제 금액 {total}원 안에서만 나눌 수 있습니다. 결제 금액을 바꾸려면 분할을 되돌린 뒤 고치세요"
        ));
    }
    Ok(Some((partner.id, partner_amount)))
}

/// 옮길 장부의 카테고리 고르기
pub enum CategoryChoice {
    /// 원래 카테고리와 이름·종류가 같은 항목, 없으면 미분류
    SameName,
    Set(Option<i32>),
}

/// 확정 거래 `id`에서 `amount`를 떼어 `ledger_id` 장부의 새 거래(자식)로 만든다. 원래 거래 금액은 그만큼 줄인다.
pub async fn split<C: ConnectionTrait>(
    db: &C,
    id: i32,
    ledger_id: i32,
    amount: i64,
    category: CategoryChoice,
) -> Result<transaction::Model, SplitError> {
    let tx = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| invalid("거래가 없습니다"))?;
    if tx.status != TxStatus::Confirmed {
        return Err(invalid("확정된 거래만 나눌 수 있습니다"));
    }
    if tx.split_parent_id.is_some() || group_of(db, &tx).await?.is_some() {
        return Err(invalid("이미 분할된 거래입니다. 분할을 되돌린 뒤 다시 나누세요"));
    }
    if tx.ledger_id == ledger_id {
        return Err(invalid("지금 장부가 아닌 다른 장부를 고르세요"));
    }
    ledger::Entity::find_by_id(ledger_id).one(db).await?.ok_or_else(|| invalid("장부가 없습니다"))?;
    check_part(tx.amount, amount).map_err(invalid)?;
    let category_id = match category {
        CategoryChoice::SameName => match tx.category_id {
            Some(cid) => {
                let categories: HashMap<i32, category::Model> =
                    category::Entity::find().all(db).await?.into_iter().map(|c| (c.id, c)).collect();
                categories.get(&cid).and_then(|c| same_category_in(&categories, ledger_id, c))
            }
            None => None,
        },
        CategoryChoice::Set(None) => None,
        CategoryChoice::Set(Some(cid)) => {
            let c = category::Entity::find_by_id(cid).one(db).await?.ok_or_else(|| invalid("카테고리가 없습니다"))?;
            if c.ledger_id != ledger_id {
                return Err(invalid(format!("'{}' 카테고리는 다른 장부의 카테고리입니다", c.name)));
            }
            Some(cid)
        }
    };
    let now = Utc::now();
    let mut parent: transaction::ActiveModel = tx.clone().into();
    parent.amount = Set(tx.amount - amount);
    parent.updated_at = Set(now);
    parent.update(db).await?;
    let child = transaction::ActiveModel {
        ledger_id: Set(ledger_id),
        date: Set(tx.date),
        amount: Set(amount),
        category_id: Set(category_id),
        payment_method_id: Set(tx.payment_method_id),
        description: Set(tx.description.clone()),
        memo: Set(tx.memo.clone()),
        member: Set(tx.member.clone()),
        status: Set(tx.status),
        installment_months: Set(tx.installment_months),
        exclude_from_performance: Set(tx.exclude_from_performance),
        // 반복 규칙 자리와 자동 입력 중복 방지 키는 부모에만 둔다
        recurring_rule_id: Set(None),
        recurring_month: Set(None),
        source: Set(tx.source),
        external_id: Set(None),
        raw_text: Set(tx.raw_text.clone()),
        split_parent_id: Set(Some(tx.id)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(child)
}

/// 묶음을 부모 한 건으로 합친다: 자식 금액을 부모에 더하고 자식을 지운다.
pub async fn merge<C: ConnectionTrait>(db: &C, group: SplitGroup) -> Result<transaction::Model, DbErr> {
    let total = group.total();
    let child_ids: Vec<i32> = group.children.iter().map(|c| c.id).collect();
    // 자식에는 혜택을 두지 않지만 남아 있으면 같이 지운다
    transaction_benefit::Entity::delete_many()
        .filter(transaction_benefit::Column::TransactionId.is_in(child_ids.clone()))
        .exec(db)
        .await?;
    transaction::Entity::delete_many().filter(transaction::Column::Id.is_in(child_ids)).exec(db).await?;
    let mut parent: transaction::ActiveModel = group.parent.into();
    parent.amount = Set(total);
    parent.updated_at = Set(Utc::now());
    parent.update(db).await
}

/// 분할을 되돌린다. `id`는 묶음의 어느 거래든 된다. 합친 거래(부모)를 돌려준다.
pub async fn unsplit<C: ConnectionTrait>(db: &C, id: i32) -> Result<transaction::Model, SplitError> {
    let tx = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| invalid("거래가 없습니다"))?;
    let group = group_of(db, &tx).await?.ok_or_else(|| invalid("분할된 거래가 아닙니다"))?;
    Ok(merge(db, group).await?)
}

/// 결제 속성(날짜·결제수단·할부·실적 제외·상태)을 `source`와 같은 묶음의 다른 거래에 맞춘다.
pub async fn sync_payment_fields<C: ConnectionTrait>(db: &C, source: &transaction::Model) -> Result<(), DbErr> {
    let Some(group) = group_of(db, source).await? else { return Ok(()) };
    for other in group.members().filter(|t| t.id != source.id) {
        if other.date == source.date
            && other.payment_method_id == source.payment_method_id
            && other.installment_months == source.installment_months
            && other.exclude_from_performance == source.exclude_from_performance
            && other.status == source.status
        {
            continue;
        }
        let mut active: transaction::ActiveModel = other.clone().into();
        active.date = Set(source.date);
        active.payment_method_id = Set(source.payment_method_id);
        active.installment_months = Set(source.installment_months);
        active.exclude_from_performance = Set(source.exclude_from_performance);
        active.status = Set(source.status);
        active.updated_at = Set(Utc::now());
        active.update(db).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use chrono::NaiveDate;

    use super::*;
    use crate::entity::enums::{CategoryKind, TxSource};

    async fn ledger(db: &sea_orm::DatabaseConnection, name: &str, sort_order: i32) -> i32 {
        ledger::ActiveModel {
            name: Set(name.into()),
            description: Set(None),
            is_default: Set(sort_order == 0),
            sort_order: Set(sort_order),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn category(db: &sea_orm::DatabaseConnection, ledger_id: i32, name: &str) -> i32 {
        category::ActiveModel {
            ledger_id: Set(ledger_id),
            name: Set(name.into()),
            kind: Set(CategoryKind::Expense),
            monthly_budget: Set(None),
            is_fixed: Set(false),
            excluded_from_spending: Set(false),
            color: Set(None),
            sort_order: Set(0),
            archived: Set(false),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    static NEXT: AtomicU32 = AtomicU32::new(0);

    async fn tx(db: &sea_orm::DatabaseConnection, ledger_id: i32, amount: i64, category_id: Option<i32>, status: TxStatus) -> transaction::Model {
        let now = Utc::now();
        transaction::ActiveModel {
            ledger_id: Set(ledger_id),
            date: Set(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()),
            amount: Set(amount),
            category_id: Set(category_id),
            payment_method_id: Set(None),
            description: Set("마트".into()),
            memo: Set(Some("메모".into())),
            member: Set(None),
            status: Set(status),
            installment_months: Set(1),
            exclude_from_performance: Set(false),
            recurring_rule_id: Set(None),
            recurring_month: Set(None),
            source: Set(TxSource::Api),
            external_id: Set(Some(format!("ext-{}", NEXT.fetch_add(1, Ordering::Relaxed)))),
            raw_text: Set(Some("승인 문자".into())),
            split_parent_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn get(db: &sea_orm::DatabaseConnection, id: i32) -> Option<transaction::Model> {
        transaction::Entity::find_by_id(id).one(db).await.unwrap()
    }

    #[test]
    fn part_must_be_inside_total() {
        assert!(check_part(20_000, 8_000).is_ok());
        assert!(check_part(20_000, 19_999).is_ok());
        assert!(check_part(20_000, 20_000).is_err());
        assert!(check_part(20_000, 0).is_err());
        assert!(check_part(20_000, -1_000).is_err());
        assert!(check_part(-5_000, -2_000).is_ok());
        assert!(check_part(-5_000, 2_000).is_err());
        assert!(check_part(0, 0).is_err());
    }

    #[tokio::test]
    async fn split_then_unsplit_restores_one_payment() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let mart = category(&db, home, "장보기").await;
        let pocket_mart = category(&db, pocket, "장보기").await;
        let t = tx(&db, home, 20_000, Some(mart), TxStatus::Confirmed).await;

        let child = split(&db, t.id, pocket, 8_000, CategoryChoice::SameName).await.unwrap();
        assert_eq!((child.ledger_id, child.amount, child.category_id), (pocket, 8_000, Some(pocket_mart)));
        assert_eq!(child.split_parent_id, Some(t.id));
        assert_eq!((child.date, child.memo.as_deref(), child.source), (t.date, Some("메모"), TxSource::Api));
        assert_eq!(child.external_id, None, "중복 방지 키는 부모에만");
        assert_eq!(get(&db, t.id).await.unwrap().amount, 12_000);

        // 이미 분할된 거래는 다시 나누지 않는다 (부모·자식 모두)
        assert!(split(&db, t.id, pocket, 1_000, CategoryChoice::SameName).await.is_err());
        assert!(split(&db, child.id, home, 1_000, CategoryChoice::SameName).await.is_err());

        // 자식 쪽에서 되돌려도 부모 한 건으로 합쳐진다
        let merged = unsplit(&db, child.id).await.unwrap();
        assert_eq!((merged.id, merged.amount, merged.ledger_id), (t.id, 20_000, home));
        assert!(get(&db, child.id).await.is_none());
        assert!(unsplit(&db, t.id).await.is_err(), "분할되지 않은 거래");
    }

    #[tokio::test]
    async fn split_rejects_invalid_requests() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let home_cat = category(&db, home, "외식").await;
        let planned = tx(&db, home, 10_000, None, TxStatus::Planned).await;
        let t = tx(&db, home, 10_000, Some(home_cat), TxStatus::Confirmed).await;
        assert!(split(&db, planned.id, pocket, 1_000, CategoryChoice::SameName).await.is_err(), "예정 거래");
        assert!(split(&db, t.id, home, 1_000, CategoryChoice::SameName).await.is_err(), "같은 장부");
        assert!(split(&db, t.id, pocket, 10_000, CategoryChoice::SameName).await.is_err(), "전액");
        assert!(split(&db, t.id, pocket, 1_000, CategoryChoice::Set(Some(home_cat))).await.is_err(), "다른 장부 카테고리");
        assert_eq!(get(&db, t.id).await.unwrap().amount, 10_000);
        // 옮길 장부에 같은 이름 항목이 없으면 미분류
        let c = split(&db, t.id, pocket, 1_000, CategoryChoice::SameName).await.unwrap();
        assert_eq!(c.category_id, None);
    }

    #[tokio::test]
    async fn rebalance_keeps_payment_total_and_syncs_payment_fields() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let t = tx(&db, home, 20_000, None, TxStatus::Confirmed).await;
        let child = split(&db, t.id, pocket, 8_000, CategoryChoice::SameName).await.unwrap();
        let parent = get(&db, t.id).await.unwrap();
        let group = group_of(&db, &child).await.unwrap().unwrap();
        assert_eq!(group.total(), 20_000);
        assert_eq!(rebalance(&group, child.id, 5_000), Ok(Some((parent.id, 15_000))));
        assert_eq!(rebalance(&group, parent.id, 19_000), Ok(Some((child.id, 1_000))));
        assert_eq!(rebalance(&group, parent.id, 12_000), Ok(None));
        assert!(rebalance(&group, parent.id, 20_000).is_err(), "짝이 0원");
        assert!(rebalance(&group, child.id, 25_000).is_err(), "짝이 음수");

        let mut moved: transaction::ActiveModel = child.clone().into();
        moved.date = Set(NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
        moved.installment_months = Set(3);
        let moved = moved.update(&db).await.unwrap();
        sync_payment_fields(&db, &moved).await.unwrap();
        let parent = get(&db, t.id).await.unwrap();
        assert_eq!((parent.date, parent.installment_months), (moved.date, 3));
    }

    #[tokio::test]
    async fn detects_split_members() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let a = tx(&db, home, 20_000, None, TxStatus::Confirmed).await;
        let b = tx(&db, home, 30_000, None, TxStatus::Confirmed).await;
        assert!(!any_split(&db, &[a.clone(), b.clone()]).await.unwrap());
        let child = split(&db, a.id, pocket, 8_000, CategoryChoice::SameName).await.unwrap();
        let a = get(&db, a.id).await.unwrap();
        assert!(any_split(&db, &[a, b.clone()]).await.unwrap(), "부모");
        assert!(any_split(&db, &[child]).await.unwrap(), "자식");
        assert!(!any_split(&db, &[b]).await.unwrap());
    }
}
