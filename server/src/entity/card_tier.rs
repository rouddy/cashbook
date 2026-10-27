use sea_orm::entity::prelude::*;

/// 전월실적 구간별 월 통합 혜택 한도 (예: 30만원 이상 → 1만원).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "card_tiers")]
#[graphql(name = "CardTier")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub payment_method_id: i32,
    pub min_spending: i64,
    pub benefit_limit: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
