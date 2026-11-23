//! 월 요약(이번 달 지출 vs 평균, 지출계획 대비 월잔액)과 앱 정보.

use async_graphql::{Context, Object, Result, SimpleObject};
use chrono::NaiveDate;
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter};

use super::{config, db, lookup, uid, user_error};
use crate::domain::dates::YearMonth;
use crate::domain::summary::{MonthlySummary, SummaryContext, monthly_summary};
use crate::entity::{
    OfUser, enums::TxStatus, plan_item, transaction};

#[derive(SimpleObject)]
pub struct AppInfo {
    pub version: String,
    pub today: NaiveDate,
    pub timezone: String,
    pub auth_enabled: bool,
    /// 반복 규칙으로 이번 달 + N개월치 예정 거래를 만든다
    pub plan_months_ahead: i32,
}

#[derive(Default)]
pub struct SummaryQuery;

#[Object]
impl SummaryQuery {
    async fn app_info(&self, ctx: &Context<'_>) -> AppInfo {
        let cfg = config(ctx);
        AppInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            today: cfg.today(),
            timezone: cfg.timezone.to_string(),
            auth_enabled: cfg.auth_enabled(),
            plan_months_ahead: cfg.plan_months_ahead as i32,
        }
    }

    /// 월 요약. `ledgerId`를 비우면 전체 장부 합계 (지출계획/월잔액/장부 이체는 장부를 지정해야 계산).
    async fn monthly_summary(
        &self,
        ctx: &Context<'_>,
        month: YearMonth,
        ledger_id: Option<i32>,
        #[graphql(default = 6)] average_months: i32,
        #[graphql(default = 12)] history_months: i32,
    ) -> Result<MonthlySummary> {
        let uid = uid(ctx)?;
        if !(1..=36).contains(&average_months) || !(1..=60).contains(&history_months) {
            return Err(user_error("평균 기간은 1~36개월, 추이 기간은 1~60개월입니다"));
        }
        let db = db(ctx);
        let lookback = average_months.max(history_months);
        let start = month.add_months(-lookback).first_day();
        let mut q = transaction::Entity::find()
            .of_user(uid)
            .filter(transaction::Column::Date.gte(start))
            .filter(transaction::Column::Date.lte(month.last_day()))
            .filter(transaction::Column::Status.ne(TxStatus::Skipped));
        if let Some(id) = ledger_id {
            // 그 장부의 거래와 다른 장부에서 받은 이체
            q = q.filter(Condition::any().add(transaction::Column::LedgerId.eq(id)).add(transaction::Column::TransferLedgerId.eq(id)));
        }
        let txs = q.all(db).await?;
        let plan_items = match ledger_id {
            Some(id) => plan_item::Entity::find().of_user(uid).filter(plan_item::Column::LedgerId.eq(id)).all(db).await?,
            None => vec![],
        };
        let lookup = lookup(ctx);
        let categories = lookup.categories().await?;
        let payment_methods = lookup.payment_methods().await?;
        Ok(monthly_summary(
            &SummaryContext { categories: &categories, payment_methods: &payment_methods, ledger_id, plan_items: &plan_items },
            month,
            average_months as u32,
            history_months as u32,
            &txs,
        ))
    }
}
