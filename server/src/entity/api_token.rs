use sea_orm::entity::prelude::*;

/// API 토큰 (자동 입력·GraphQL 모두). 원문은 발급 시 한 번만 보여주고 SHA-256 해시만 저장한다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "api_tokens")]
#[graphql(name = "ApiToken")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
    pub name: String,
    #[sea_orm(unique)]
    #[graphql(skip)]
    pub token_hash: String,
    /// 토큰 앞부분 (목록 표시용)
    pub token_prefix: String,
    pub created_at: DateTimeUtc,
    pub last_used_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

super::user_owned!();
