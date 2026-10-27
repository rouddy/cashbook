use sea_orm::entity::prelude::*;

use super::enums::PaymentKind;

/// 결제수단. 카드(신용/체크)와 현금·계좌이체 등을 함께 관리한다.
/// 신용카드는 `payment_day`/`closing_day`/`closing_month_offset`으로 정산 일정을 정의한다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "payment_methods")]
#[graphql(name = "PaymentMethod", complex)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub kind: PaymentKind,
    /// 카드사/은행
    pub issuer: Option<String>,
    pub product_name: Option<String>,
    /// 카드번호 끝 4자리. 자동 입력 문자에서 카드를 찾을 때 쓴다.
    pub last4: Option<String>,
    /// 쉼표로 구분한 별칭. GraphQL에는 `aliases` 목록으로 노출한다.
    #[graphql(skip)]
    pub aliases: Option<String>,
    /// 결제(출금) 계좌
    pub account_id: Option<i32>,
    /// 결제일 (1~31)
    pub payment_day: Option<i32>,
    /// 이용기간 마감일 (1~31, 31이면 말일)
    pub closing_day: Option<i32>,
    /// 마감월: 결제월 기준 0=당월, -1=전월, -2=전전월
    pub closing_month_offset: Option<i32>,
    /// 전월실적 목표 금액
    pub monthly_target: Option<i64>,
    pub annual_fee: Option<i64>,
    /// 유효기간 (YYYY-MM)
    pub expires_on: Option<String>,
    pub color: Option<String>,
    pub memo: Option<String>,
    pub sort_order: i32,
    pub archived: bool,
    pub created_at: DateTimeUtc,
}

impl Model {
    pub fn alias_list(&self) -> Vec<String> {
        self.aliases
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
