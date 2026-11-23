use sea_orm::entity::prelude::*;

use super::enums::AccountKind;

/// 은행 계좌와 잔고. 잔고를 바꿀 때마다 `account_balances`에 이력이 남는다.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "accounts")]
#[graphql(name = "Account", complex)]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
    pub name: String,
    pub bank: Option<String>,
    pub kind: AccountKind,
    pub account_number: Option<String>,
    pub balance: i64,
    /// 잔고 기준일. 이 날 이후 결제되는 카드대금/출금은 아직 잔고에 반영되지 않은 것으로 본다.
    pub balance_date: Option<Date>,
    /// 여유자금 계산에 포함
    pub include_in_available: bool,
    pub interest_rate: Option<f64>,
    pub maturity_date: Option<Date>,
    pub memo: Option<String>,
    pub sort_order: i32,
    pub archived: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

super::user_owned!();
