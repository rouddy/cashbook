use sea_orm::entity::prelude::*;

/// 반복 지출·수입·장부 이체 규칙. 매달 `day_of_month`에 예정(PLANNED) 거래를 만든다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "recurring_rules")]
#[graphql(name = "RecurringRule", complex)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub ledger_id: i32,
    pub description: String,
    pub amount: i64,
    pub category_id: Option<i32>,
    pub payment_method_id: Option<i32>,
    /// 1~31 (31이면 말일)
    pub day_of_month: i32,
    /// 반복 간격(개월): 1=매월, 12=매년
    pub interval_months: i32,
    /// 시작 월 (YYYY-MM)
    pub start_month: String,
    /// 종료 월 (YYYY-MM, 포함)
    pub end_month: Option<String>,
    pub member: Option<String>,
    pub memo: Option<String>,
    /// 자동 입력 매칭 키워드 (쉼표로 여러 개). 정했으면 가맹점(내용)에 하나는 있어야 이 규칙의 예정 항목을 확정한다
    pub match_keyword: Option<String>,
    /// 자동 입력 매칭 금액 범위 (양끝 포함, 한쪽만 정해도 된다). 정했으면 실제 금액이 이 안이어야 확정한다
    pub match_amount_min: Option<i64>,
    pub match_amount_max: Option<i64>,
    pub exclude_from_performance: bool,
    /// 장부 이체 규칙이면 받는 장부 (항목·결제수단 없음)
    pub transfer_ledger_id: Option<i32>,
    pub active: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
