use sea_orm::entity::prelude::*;

/// 반복 규칙에 미리 골라 둔 카드 혜택 항목 (다대다). 규칙이 예정 거래를 만들 때 `transaction_benefits`로 옮긴다.
/// 규칙의 결제수단 카드 혜택만 담고, 결제수단이 바뀌면 비운다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "recurring_rule_benefits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub recurring_rule_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub card_benefit_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
