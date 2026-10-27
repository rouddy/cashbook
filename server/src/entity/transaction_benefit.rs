use sea_orm::entity::prelude::*;

/// 거래에 적용한 카드 혜택 항목 (다대다).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "transaction_benefits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub transaction_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub card_benefit_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
