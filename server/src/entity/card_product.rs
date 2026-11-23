use sea_orm::entity::prelude::*;

/// 카드 상품 (예: 신한 Deep Dream). 전월실적 목표·구간·혜택·연회비는 계정과 관계없이 함께 쓰고,
/// 각 계정의 카드(`payment_methods.card_product_id`)가 상품을 가리킨다.
/// 누구나 만들 수 있고, 고치거나 지우는 건 만든 계정과 관리자만 한다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "card_products")]
#[graphql(name = "CardProduct", complex)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 카드사
    pub issuer: Option<String>,
    /// 상품명
    pub name: String,
    pub annual_fee: Option<i64>,
    /// 전월실적 목표 금액 (구간이 없을 때의 목표)
    pub monthly_target: Option<i64>,
    pub memo: Option<String>,
    /// 만든 계정. 계정이 지워지면 비고, 그때는 관리자만 고친다.
    #[graphql(skip)]
    pub created_by: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

impl Model {
    /// "카드사 상품명"
    pub fn display_name(&self) -> String {
        match self.issuer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(issuer) if !self.name.starts_with(issuer) => format!("{issuer} {}", self.name),
            _ => self.name.clone(),
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
