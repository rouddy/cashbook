//! 반복 지출·수입·장부 이체 규칙. 저장하면 이번 달부터 예정 거래를 다시 만든다.

use async_graphql::{ComplexObject, Context, InputObject, Object, Result};
use chrono::{NaiveDate, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait};

use super::{config, db, lookup, trim_opt, user_error};
use crate::domain::dates::YearMonth;
use crate::domain::recurring::{self, occurs_in};
use crate::entity::{category, ledger, payment_method, recurring_rule};

#[ComplexObject]
impl recurring_rule::Model {
    async fn category(&self, ctx: &Context<'_>) -> Result<Option<category::Model>> {
        let Some(id) = self.category_id else { return Ok(None) };
        Ok(lookup(ctx).categories().await?.get(&id).cloned())
    }

    async fn payment_method(&self, ctx: &Context<'_>) -> Result<Option<payment_method::Model>> {
        let Some(id) = self.payment_method_id else { return Ok(None) };
        Ok(lookup(ctx).payment_methods().await?.get(&id).cloned())
    }

    /// 오늘 이후 다음 발생일
    async fn next_date(&self, ctx: &Context<'_>) -> Option<NaiveDate> {
        if !self.active {
            return None;
        }
        let today = config(ctx).today();
        let start = YearMonth::of(today);
        (0..=24)
            .map(|i| start.add_months(i))
            .filter(|m| occurs_in(self, *m))
            .map(|m| m.day(self.day_of_month.max(1) as u32))
            .find(|d| *d >= today)
    }
}

#[derive(Default)]
pub struct RecurringQuery;

#[Object]
impl RecurringQuery {
    /// 반복 규칙. `ledgerId`를 주면 그 장부의 규칙과 다른 장부에서 그 장부로 보내는 이체 규칙.
    async fn recurring_rules(&self, ctx: &Context<'_>, ledger_id: Option<i32>) -> Result<Vec<recurring_rule::Model>> {
        let mut q = recurring_rule::Entity::find();
        if let Some(id) = ledger_id {
            q = q.filter(
                Condition::any().add(recurring_rule::Column::LedgerId.eq(id)).add(recurring_rule::Column::TransferLedgerId.eq(id)),
            );
        }
        Ok(q.order_by_desc(recurring_rule::Column::Active)
            .order_by_asc(recurring_rule::Column::DayOfMonth)
            .order_by_asc(recurring_rule::Column::Id)
            .all(db(ctx))
            .await?)
    }
}

#[derive(InputObject)]
pub struct RecurringRuleInput {
    pub ledger_id: i32,
    pub description: String,
    pub amount: i64,
    pub category_id: Option<i32>,
    pub payment_method_id: Option<i32>,
    /// 1~31 (31 = 말일)
    pub day_of_month: i32,
    #[graphql(default = 1)]
    pub interval_months: i32,
    pub start_month: YearMonth,
    pub end_month: Option<YearMonth>,
    pub member: Option<String>,
    pub memo: Option<String>,
    /// 자동 입력 매칭 키워드 (쉼표로 여러 개)
    pub match_keyword: Option<String>,
    #[graphql(default = false)]
    pub exclude_from_performance: bool,
    /// 장부 이체 규칙이면 받는 장부 (항목·결제수단은 비운다)
    pub transfer_ledger_id: Option<i32>,
    #[graphql(default = true)]
    pub active: bool,
}

async fn validate(ctx: &Context<'_>, input: &RecurringRuleInput) -> Result<()> {
    let db = db(ctx);
    if input.description.trim().is_empty() {
        return Err(user_error("내용을 입력하세요"));
    }
    if !(1..=31).contains(&input.day_of_month) {
        return Err(user_error("날짜는 1~31 사이여야 합니다"));
    }
    if !(1..=24).contains(&input.interval_months) {
        return Err(user_error("반복 간격은 1~24개월입니다"));
    }
    if input.end_month.is_some_and(|e| e < input.start_month) {
        return Err(user_error("종료 월이 시작 월보다 빠릅니다"));
    }
    ledger::Entity::find_by_id(input.ledger_id).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
    if let Some(to) = input.transfer_ledger_id {
        if to == input.ledger_id {
            return Err(user_error("받는 장부는 보내는 장부와 달라야 합니다"));
        }
        ledger::Entity::find_by_id(to).one(db).await?.ok_or_else(|| user_error("받는 장부가 없습니다"))?;
        if input.category_id.is_some() || input.payment_method_id.is_some() {
            return Err(user_error("장부 이체에는 항목·결제수단을 지정하지 않습니다"));
        }
    }
    if let Some(cid) = input.category_id {
        let c = category::Entity::find_by_id(cid).one(db).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?;
        if c.ledger_id != input.ledger_id {
            return Err(user_error("카테고리가 다른 장부에 속해 있습니다"));
        }
    }
    if let Some(pid) = input.payment_method_id {
        payment_method::Entity::find_by_id(pid).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
    }
    Ok(())
}

fn apply(active: &mut recurring_rule::ActiveModel, input: RecurringRuleInput) {
    active.ledger_id = Set(input.ledger_id);
    active.description = Set(input.description.trim().to_string());
    active.amount = Set(input.amount);
    active.category_id = Set(input.category_id);
    active.payment_method_id = Set(input.payment_method_id);
    active.day_of_month = Set(input.day_of_month);
    active.interval_months = Set(input.interval_months);
    active.start_month = Set(input.start_month.to_string());
    active.end_month = Set(input.end_month.map(|m| m.to_string()));
    active.member = Set(trim_opt(input.member));
    active.memo = Set(trim_opt(input.memo));
    active.match_keyword = Set(trim_opt(input.match_keyword));
    active.exclude_from_performance = Set(input.exclude_from_performance && input.transfer_ledger_id.is_none());
    active.transfer_ledger_id = Set(input.transfer_ledger_id);
    active.active = Set(input.active);
    active.updated_at = Set(Utc::now());
}

#[derive(Default)]
pub struct RecurringMutation;

#[Object]
impl RecurringMutation {
    async fn create_recurring_rule(&self, ctx: &Context<'_>, input: RecurringRuleInput) -> Result<recurring_rule::Model> {
        validate(ctx, &input).await?;
        let db = db(ctx);
        let cfg = config(ctx);
        let txn = db.begin().await?;
        let mut active = recurring_rule::ActiveModel { created_at: Set(Utc::now()), ..Default::default() };
        apply(&mut active, input);
        let saved = active.insert(&txn).await?;
        recurring::regenerate(&txn, saved.id, cfg.today(), cfg.plan_months_ahead).await?;
        txn.commit().await?;
        Ok(saved)
    }

    async fn update_recurring_rule(&self, ctx: &Context<'_>, id: i32, input: RecurringRuleInput) -> Result<recurring_rule::Model> {
        validate(ctx, &input).await?;
        let db = db(ctx);
        let cfg = config(ctx);
        let found = recurring_rule::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("반복 규칙이 없습니다"))?;
        let txn = db.begin().await?;
        let mut active: recurring_rule::ActiveModel = found.into();
        apply(&mut active, input);
        let saved = active.update(&txn).await?;
        recurring::regenerate(&txn, saved.id, cfg.today(), cfg.plan_months_ahead).await?;
        txn.commit().await?;
        Ok(saved)
    }

    /// 규칙 삭제. 이번 달 이후의 예정 항목도 지운다 (확정된 거래는 남는다).
    async fn delete_recurring_rule(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let db = db(ctx);
        let txn = db.begin().await?;
        recurring::remove_future_planned(&txn, id, config(ctx).today()).await?;
        let res = recurring_rule::Entity::delete_by_id(id).exec(&txn).await?;
        txn.commit().await?;
        Ok(res.rows_affected > 0)
    }

    /// 반복 규칙의 예정 거래를 지금 채운다 (서버가 주기적으로도 실행한다). 만든 개수를 돌려준다.
    async fn generate_planned(&self, ctx: &Context<'_>) -> Result<i32> {
        let cfg = config(ctx);
        Ok(recurring::ensure_planned(db(ctx), cfg.today(), cfg.plan_months_ahead, None).await? as i32)
    }
}
