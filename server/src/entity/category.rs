use sea_orm::entity::prelude::*;

use super::enums::CategoryKind;

/// 지출/수입 항목 (예: 장보기, 외식, 기타수익).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "categories")]
#[graphql(name = "Category")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub ledger_id: i32,
    pub name: String,
    pub kind: CategoryKind,
    /// 월 예산 (없으면 예산 미설정)
    pub monthly_budget: Option<i64>,
    /// 고정비 (대출 상환, 보험료 등)
    pub is_fixed: bool,
    /// 월 소비 합계에서 제외 (경조사비·여행처럼 별도 적립금에서 쓰는 항목)
    pub excluded_from_spending: bool,
    pub color: Option<String>,
    pub sort_order: i32,
    pub archived: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
