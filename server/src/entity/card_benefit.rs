use sea_orm::entity::prelude::*;

/// 카드 혜택 항목 (예: 간편결제 10% 할인, 1만원 이상 결제 시).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "card_benefits")]
#[graphql(name = "CardBenefit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub payment_method_id: i32,
    pub name: String,
    /// 할인/적립률 (0.1 = 10%)
    pub rate: f64,
    /// 건당 최소 결제금액
    pub min_amount: Option<i64>,
    /// 건당 최대 혜택
    pub per_tx_max: Option<i64>,
    /// 항목별 월 혜택 한도
    pub monthly_cap: Option<i64>,
    /// 카드 통합 혜택 한도(전월실적 구간)에 포함되는지
    pub shares_card_limit: bool,
    /// 이 혜택을 받은 이용금액은 전월실적에서 제외
    pub excludes_performance: bool,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
