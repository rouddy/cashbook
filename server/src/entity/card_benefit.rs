use sea_orm::entity::prelude::*;

use super::enums::BenefitKind;

/// 카드 혜택 항목 (예: 간편결제 10% 할인, 1만원 이상 결제 시).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "card_benefits")]
#[graphql(name = "CardBenefit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub payment_method_id: i32,
    pub name: String,
    /// 할인/포인트 적립/마일리지 적립. 건당 최대·항목 월한도는 이 종류의 단위(원·P·마일)다.
    pub kind: BenefitKind,
    /// 이용금액 1원당 혜택 (할인·포인트는 0.1 = 10%, 마일리지는 1/1500 = 1,500원당 1마일)
    pub rate: f64,
    /// 건당 최소 결제금액
    pub min_amount: Option<i64>,
    /// 건당 최대 혜택
    pub per_tx_max: Option<i64>,
    /// 항목별 월 혜택 한도
    pub monthly_cap: Option<i64>,
    /// 카드 통합 혜택 한도(전월실적 구간)에서 차감되는지. 아니고 `monthly_cap`도 없으면 한도 없이 받는다.
    pub shares_card_limit: bool,
    /// 이 혜택을 받은 이용금액은 전월실적에서 제외
    pub excludes_performance: bool,
    /// 다른 혜택과 함께 받을 수 있는지 (예: 간편결제 추가 할인). 아닌 혜택은 거래당 하나만 고른다.
    pub stackable: bool,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
