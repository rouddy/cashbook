//! DB에 문자열로 저장되고 GraphQL에는 enum으로 노출되는 값들.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 카테고리 종류. 수입 카테고리의 거래는 양수가 수입이다.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CategoryKind {
    #[sea_orm(string_value = "EXPENSE")]
    Expense,
    #[sea_orm(string_value = "INCOME")]
    Income,
}

/// 결제수단 종류.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentKind {
    /// 신용카드: 이용기간/결제일에 따라 결제 예정 금액을 계산한다.
    #[sea_orm(string_value = "CREDIT_CARD")]
    CreditCard,
    /// 체크카드: 연결 계좌에서 바로 빠져나간다.
    #[sea_orm(string_value = "CHECK_CARD")]
    CheckCard,
    #[sea_orm(string_value = "CASH")]
    Cash,
    #[sea_orm(string_value = "BANK_TRANSFER")]
    BankTransfer,
    #[sea_orm(string_value = "OTHER")]
    Other,
}

impl PaymentKind {
    pub fn is_card(self) -> bool {
        matches!(self, PaymentKind::CreditCard | PaymentKind::CheckCard)
    }
}

/// 은행 계좌 종류.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountKind {
    /// 입출금
    #[sea_orm(string_value = "CHECKING")]
    Checking,
    /// 적금/청약
    #[sea_orm(string_value = "SAVINGS")]
    Savings,
    /// 예금
    #[sea_orm(string_value = "DEPOSIT")]
    Deposit,
    #[sea_orm(string_value = "INVESTMENT")]
    Investment,
    #[sea_orm(string_value = "CASH")]
    Cash,
    #[sea_orm(string_value = "OTHER")]
    Other,
}

/// 거래 상태.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TxStatus {
    /// 실제로 결제/입금된 거래
    #[sea_orm(string_value = "CONFIRMED")]
    Confirmed,
    /// 지출(수입) 예정
    #[sea_orm(string_value = "PLANNED")]
    Planned,
    /// 반복 규칙으로 생긴 예정 항목을 이번 달은 건너뛴 경우. 다시 생성되지 않도록 남겨둔다.
    #[sea_orm(string_value = "SKIPPED")]
    Skipped,
}

/// 거래가 만들어진 경로.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TxSource {
    #[sea_orm(string_value = "MANUAL")]
    Manual,
    #[sea_orm(string_value = "IMPORT")]
    Import,
    /// 자동 입력 API(`/api/ingest`, `ingestTransaction`)
    #[sea_orm(string_value = "API")]
    Api,
    /// 반복 규칙으로 생성된 예정 항목
    #[sea_orm(string_value = "RECURRING")]
    Recurring,
}

/// 지출계획의 고정 항목 종류.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanItemKind {
    /// 월 수입 (예: 월급)
    #[sea_orm(string_value = "INCOME")]
    Income,
    /// 월 배분 (예: 용돈, 적금, 경조사비 적립). 월잔액에서 뺀다.
    #[sea_orm(string_value = "ALLOCATION")]
    Allocation,
    /// 구성원 용돈. 그 사람이 쓴 돈은 이 장부에 구성원 표시로 기록되므로 월잔액에서 따로 빼지 않고
    /// 구성원 지출과 비교만 한다 (예: 송화 용돈 50만원 vs 송화 표시 지출).
    #[sea_orm(string_value = "MEMBER_ALLOWANCE")]
    MemberAllowance,
}

/// 카드 혜택 종류. 혜택액 단위가 할인·포인트는 원(1P = 1원), 마일리지는 마일이다.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    EnumIter,
    DeriveActiveEnum,
    async_graphql::Enum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BenefitKind {
    /// 청구 할인
    #[default]
    #[sea_orm(string_value = "DISCOUNT")]
    Discount,
    /// 포인트 적립
    #[sea_orm(string_value = "POINT")]
    Point,
    /// 마일리지 적립
    #[sea_orm(string_value = "MILEAGE")]
    Mileage,
}

/// API 토큰 권한.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, async_graphql::Enum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TokenScope {
    /// 자동 입력(`/api/ingest`)만
    #[sea_orm(string_value = "INGEST")]
    Ingest,
    /// GraphQL 전체 (읽기/쓰기)
    #[sea_orm(string_value = "FULL")]
    Full,
}
