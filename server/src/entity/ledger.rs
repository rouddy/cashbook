use sea_orm::entity::prelude::*;

/// 장부 (예: 가계부, 용돈). 카테고리와 지출계획은 장부별로 따로 관리한다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "ledgers")]
#[graphql(name = "Ledger")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
    /// 계정 안에서 겹치지 않는다
    pub name: String,
    pub description: Option<String>,
    /// 자동 입력 등에서 장부를 지정하지 않았을 때 쓰는 기본 장부
    pub is_default: bool,
    pub sort_order: i32,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

super::user_owned!();
