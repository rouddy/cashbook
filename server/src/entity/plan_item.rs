use sea_orm::entity::prelude::*;

use super::enums::PlanItemKind;

/// 지출계획의 월 고정 항목: 배분(저축 등), 구성원 용돈.
/// 수입은 수입 항목(정기수익)의 반복 규칙으로, 다른 장부로 보내는 돈은 장부 이체 반복 규칙으로 예정에 넣는다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "plan_items")]
#[graphql(name = "PlanItem")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
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

super::user_owned!();
