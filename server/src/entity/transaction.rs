use sea_orm::entity::prelude::*;

use super::enums::{TxSource, TxStatus};

/// 결제/지출/수입 내역. 지출 예정(PLANNED)도 같은 테이블에 둔다.
/// 금액은 원 단위 정수: 지출 카테고리면 양수=지출·음수=환불, 수입 카테고리면 양수=수입.
/// `transfer_ledger_id`가 있으면 장부 이체: `ledger_id` 장부에서 그 장부로 금액을 옮긴다(항목·결제수단 없음).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "transactions")]
#[graphql(name = "Transaction", complex)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
    pub ledger_id: i32,
    pub date: Date,
    pub amount: i64,
    pub category_id: Option<i32>,
    pub payment_method_id: Option<i32>,
    /// 내용 (가맹점 등)
    pub description: String,
    pub memo: Option<String>,
    /// 실제 사용한 사람 (예: 송화)
    pub member: Option<String>,
    pub status: TxStatus,
    /// 할부 개월 수 (일시불 = 1)
    pub installment_months: i32,
    /// 카드 전월실적에서 제외
    pub exclude_from_performance: bool,
    pub recurring_rule_id: Option<i32>,
    /// 반복 규칙으로 생성된 예정 항목의 대상 월 (YYYY-MM)
    pub recurring_month: Option<String>,
    pub source: TxSource,
    /// 자동 입력 중복 방지 키
    #[graphql(skip)]
    pub external_id: Option<String>,
    /// 자동 입력 원문 (카드 승인 문자 등)
    pub raw_text: Option<String>,
    /// 분할한 결제에서 다른 장부로 나눠 옮긴 거래면 원래 거래(부모) id.
    /// 부모와 자식들 금액의 합이 실제 결제 금액이다.
    pub split_parent_id: Option<i32>,
    /// 장부 이체면 받는 장부. 보내는 장부(`ledger_id`)에서는 나가는 돈, 받는 장부에서는 들어오는 돈이고
    /// 소비·수입 합계와 카드·여유자금 계산에는 넣지 않는다.
    pub transfer_ledger_id: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

super::user_owned!();
