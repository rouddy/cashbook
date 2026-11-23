//! 로그인 계정: Google 계정으로 로그인하면 계정을 찾거나 만든다.
//!
//! 1. 같은 Google 계정(`sub`)이 있으면 그 계정 (이메일·이름·사진을 새로 고친다)
//! 2. 확인된 이메일이 `CASHBOOK_OWNER_EMAIL`이고 기존 데이터 계정(id 1)을 아직 아무도 넘겨받지 않았으면 그 계정을 넘겨받는다
//! 3. 아니면 새 계정을 만들고 기본 장부와 항목을 넣어 준다

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set, TransactionSession, TransactionTrait,
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::entity::{category, enums::CategoryKind, ledger, user};

/// Google OpenID Connect userinfo 응답 중 쓰는 값
#[derive(Deserialize, Debug, Clone, Default)]
pub struct GoogleProfile {
    pub sub: String,
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug)]
pub enum LoginError {
    /// 사용자에게 보여줄 이유
    Rejected(&'static str),
    Db(DbErr),
}

impl From<DbErr> for LoginError {
    fn from(e: DbErr) -> Self {
        LoginError::Db(e)
    }
}

/// 새 계정에 넣어 주는 장부와 항목 (나중에 고치거나 지울 수 있다)
pub const DEFAULT_LEDGER: &str = "가계부";
const DEFAULT_EXPENSES: [&str; 7] = ["식비", "생활용품", "교통", "주거·통신", "의료", "문화·여가", "기타"];
const DEFAULT_INCOMES: [&str; 2] = ["정기수익", "기타수익"];

fn clean(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// 새 계정의 기본 장부와 항목
async fn seed<C: ConnectionTrait>(db: &C, user_id: i32, now: DateTime<Utc>) -> Result<(), DbErr> {
    let book = ledger::ActiveModel {
        user_id: Set(user_id),
        name: Set(DEFAULT_LEDGER.into()),
        description: Set(None),
        is_default: Set(true),
        sort_order: Set(0),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let kinds = DEFAULT_EXPENSES.iter().map(|n| (n, CategoryKind::Expense)).chain(DEFAULT_INCOMES.iter().map(|n| (n, CategoryKind::Income)));
    for (i, (name, kind)) in kinds.enumerate() {
        category::ActiveModel {
            user_id: Set(user_id),
            ledger_id: Set(book.id),
            name: Set(name.to_string()),
            kind: Set(kind),
            monthly_budget: Set(None),
            is_fixed: Set(false),
            color: Set(None),
            sort_order: Set(i as i32),
            archived: Set(false),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

/// 프로필 값으로 계정 정보를 새로 고친다 (로그인할 때마다)
fn refresh(active: &mut user::ActiveModel, p: &GoogleProfile, email: Option<String>, now: DateTime<Utc>) {
    active.google_sub = Set(Some(p.sub.clone()));
    if email.is_some() {
        active.email = Set(email);
    }
    if let Some(name) = clean(p.name.as_deref()) {
        active.name = Set(Some(name));
    }
    active.picture = Set(clean(p.picture.as_deref()));
    active.last_login_at = Set(Some(now));
}

/// Google 프로필로 로그인할 계정 (위 순서)
pub async fn login_google<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    config: &AppConfig,
    p: &GoogleProfile,
) -> Result<user::Model, LoginError> {
    if p.sub.trim().is_empty() {
        return Err(LoginError::Rejected("Google 계정 정보를 받지 못했습니다"));
    }
    let now = Utc::now();
    let email = clean(p.email.as_deref()).map(|e| e.to_lowercase()).filter(|_| p.email_verified);
    if let Some(found) = user::Entity::find().filter(user::Column::GoogleSub.eq(p.sub.as_str())).one(db).await? {
        let mut active: user::ActiveModel = found.into();
        refresh(&mut active, p, email, now);
        return Ok(active.update(db).await?);
    }
    let Some(email) = email else {
        return Err(LoginError::Rejected("이메일이 확인된 Google 계정으로 로그인하세요"));
    };
    let txn = db.begin().await?;
    if config.owner_email.as_deref() == Some(email.as_str())
        && let Some(legacy) = user::Entity::find_by_id(user::LEGACY_USER_ID).one(&txn).await?
        && legacy.google_sub.is_none()
    {
        let mut active: user::ActiveModel = legacy.into();
        refresh(&mut active, p, Some(email), now);
        let claimed = active.update(&txn).await?;
        txn.commit().await?;
        return Ok(claimed);
    }
    let mut active = user::ActiveModel { created_at: Set(now), ..Default::default() };
    refresh(&mut active, p, Some(email), now);
    let created = active.insert(&txn).await?;
    seed(&txn, created.id, now).await?;
    txn.commit().await?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use sea_orm::{EntityTrait, PaginatorTrait};

    use super::*;
    use crate::entity::OfUser;

    fn profile(sub: &str, email: &str) -> GoogleProfile {
        GoogleProfile {
            sub: sub.into(),
            email: Some(email.into()),
            email_verified: true,
            name: Some(format!("{sub} 이름")),
            picture: Some("https://example.com/p.png".into()),
        }
    }

    #[tokio::test]
    async fn owner_claims_existing_data_others_get_new_accounts() {
        let db = crate::db::connect_in_memory().await;
        let mut cfg = AppConfig::for_tests();
        cfg.owner_email = Some("owner@example.com".into());

        // 다른 사람이 먼저 로그인해도 기존 데이터 계정을 가져가지 않는다
        let stranger = login_google(&db, &cfg, &profile("s1", "someone@example.com")).await.unwrap();
        assert_ne!(stranger.id, user::LEGACY_USER_ID);
        let books = ledger::Entity::find().of_user(stranger.id).all(&db).await.unwrap();
        assert_eq!(books.iter().map(|l| (l.name.as_str(), l.is_default)).collect::<Vec<_>>(), vec![(DEFAULT_LEDGER, true)]);
        assert_eq!(category::Entity::find().of_user(stranger.id).count(&db).await.unwrap(), 9);

        // 소유자 이메일(대소문자 무시)은 기존 데이터 계정을 넘겨받는다. 장부를 새로 만들지 않는다
        let owner = login_google(&db, &cfg, &profile("o1", "Owner@Example.com")).await.unwrap();
        assert_eq!(owner.id, user::LEGACY_USER_ID);
        assert_eq!(owner.google_sub.as_deref(), Some("o1"));
        assert_eq!(owner.email.as_deref(), Some("owner@example.com"));
        assert_eq!(ledger::Entity::find().of_user(owner.id).count(&db).await.unwrap(), 0);

        // 다시 로그인하면 같은 계정 (이메일이 바뀌어도 Google 계정 id로 찾는다)
        let again = login_google(&db, &cfg, &profile("o1", "new@example.com")).await.unwrap();
        assert_eq!((again.id, again.email.as_deref()), (owner.id, Some("new@example.com")));

        // 이미 넘겨받았으면 같은 이메일의 다른 Google 계정은 새 계정
        let other = login_google(&db, &cfg, &profile("o2", "owner@example.com")).await.unwrap();
        assert!(other.id != owner.id && other.id != stranger.id);
        assert_eq!(user::Entity::find().count(&db).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn unverified_email_cannot_sign_up() {
        let db = crate::db::connect_in_memory().await;
        let mut cfg = AppConfig::for_tests();
        cfg.owner_email = Some("owner@example.com".into());
        let mut p = profile("x", "owner@example.com");
        p.email_verified = false;
        assert!(matches!(login_google(&db, &cfg, &p).await, Err(LoginError::Rejected(_))));
        let legacy = user::Entity::find_by_id(user::LEGACY_USER_ID).one(&db).await.unwrap().unwrap();
        assert_eq!(legacy.google_sub, None, "확인되지 않은 이메일로 기존 데이터를 넘겨받지 않는다");
    }
}
