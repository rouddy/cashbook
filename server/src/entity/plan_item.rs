use sea_orm::entity::prelude::*;

use super::enums::PlanItemKind;

/// 지출계획의 월 고정 항목: 수입(월급), 배분(용돈, 저축, 적금 등), 구성원 용돈.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "plan_items")]
#[graphql(name = "PlanItem")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub ledger_id: i32,
    pub kind: PlanItemKind,
    pub name: String,
    pub amount: i64,
    /// MEMBER_ALLOWANCE 항목의 구성원
    pub member: Option<String>,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
