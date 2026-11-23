//! 반복 지출·수입·장부 이체 규칙. 저장하면 이번 달부터 예정 거래를 다시 만든다.

use async_graphql::{ComplexObject, Context, InputObject, Object, Result};
use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};

use super::transaction::card_benefits_of;
use super::{config, db, lookup, trim_opt, uid, user_error};
use crate::domain::dates::YearMonth;
use crate::domain::recurring::{self, occurs_in};
use crate::entity::{
    OfUser, category, ledger, payment_method, recurring_rule, recurring_rule_benefit};

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

    /// 예정 거래에 붙일 카드 혜택 id (결제수단 카드의 혜택)
    async fn benefit_ids(&self, ctx: &Context<'_>) -> Result<Vec<i32>> {
        Ok(lookup(ctx).rule_benefits().await?.get(&self.id).cloned().unwrap_or_default())
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
        let uid = uid(ctx)?;
        let mut q = recurring_rule::Entity::find().of_user(uid);
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
    /// 자동 입력 매칭 키워드 (쉼표로 여러 개). 정했으면 가맹점에 하나는 있어야 확정한다
    pub match_keyword: Option<String>,
    /// 자동 입력 매칭 금액 범위 (양끝 포함, 한쪽만 정해도 된다). 둘을 같게 하면 그 금액일 때만 확정한다
    pub match_amount_min: Option<i64>,
    pub match_amount_max: Option<i64>,
    #[graphql(default = false)]
    pub exclude_from_performance: bool,
    /// 장부 이체 규칙이면 받는 장부 (항목·결제수단은 비운다)
    pub transfer_ledger_id: Option<i32>,
    #[graphql(default = true)]
    pub active: bool,
    /// 예정 거래에 붙일 결제수단 카드의 혜택 id. 주지 않으면 그대로 두되 결제수단이 바뀌면 비운다.
    /// 다른 카드의 혜택은 버리고, 중복 적용이 아닌 혜택은 하나만.
    pub benefit_ids: Option<Vec<i32>>,
}

async fn validate(ctx: &Context<'_>, input: &RecurringRuleInput) -> Result<()> {
    let uid = uid(ctx)?;
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
    if let (Some(min), Some(max)) = (input.match_amount_min, input.match_amount_max)
        && min > max {
            return Err(user_error("매칭 금액 범위의 시작이 끝보다 큽니다"));
        }
    if input.transfer_ledger_id.is_none()
        && (input.match_amount_min.is_some_and(|m| input.amount < m) || input.match_amount_max.is_some_and(|m| input.amount > m))
    {
        return Err(user_error("금액이 매칭 금액 범위 밖입니다"));
    }
    ledger::Entity::find_by_id(input.ledger_id).of_user(uid).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
    if let Some(to) = input.transfer_ledger_id {
        if to == input.ledger_id {
            return Err(user_error("받는 장부는 보내는 장부와 달라야 합니다"));
        }
        ledger::Entity::find_by_id(to).of_user(uid).one(db).await?.ok_or_else(|| user_error("받는 장부가 없습니다"))?;
        if input.category_id.is_some() || input.payment_method_id.is_some() {
            return Err(user_error("장부 이체에는 항목·결제수단을 지정하지 않습니다"));
        }
    }
    if let Some(cid) = input.category_id {
        let c = category::Entity::find_by_id(cid).of_user(uid).one(db).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?;
        if c.ledger_id != input.ledger_id {
            return Err(user_error("카테고리가 다른 장부에 속해 있습니다"));
        }
    }
    if let Some(pid) = input.payment_method_id {
        payment_method::Entity::find_by_id(pid).of_user(uid).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
    }
    Ok(())
}

/// 규칙의 혜택을 바꾼다 (결제수단이 없으면 비운다). 예정 거래는 뒤이어 `regenerate`가 다시 만든다.
async fn set_rule_benefits<C: ConnectionTrait>(db: &C, rule: &recurring_rule::Model, ids: &[i32]) -> Result<()> {
    let valid = card_benefits_of(db, rule.user_id, rule.payment_method_id, ids).await?;
    recurring_rule_benefit::Entity::delete_many()
        .filter(recurring_rule_benefit::Column::RecurringRuleId.eq(rule.id))
        .exec(db)
        .await?;
    for b in valid {
        recurring_rule_benefit::ActiveModel { recurring_rule_id: Set(rule.id), card_benefit_id: Set(b.id) }.insert(db).await?;
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
    // 장부 이체는 자동 입력으로 확정하지 않으므로 매칭 조건을 두지 않는다
    let is_transfer = input.transfer_ledger_id.is_some();
    active.match_keyword = Set(trim_opt(input.match_keyword).filter(|_| !is_transfer));
    active.match_amount_min = Set(input.match_amount_min.filter(|_| !is_transfer));
    active.match_amount_max = Set(input.match_amount_max.filter(|_| !is_transfer));
    active.exclude_from_performance = Set(input.exclude_from_performance && !is_transfer);
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
        let benefit_ids = input.benefit_ids.clone().unwrap_or_default();
        let mut active =
            recurring_rule::ActiveModel { user_id: Set(uid(ctx)?), created_at: Set(Utc::now()), ..Default::default() };
        apply(&mut active, input);
        let saved = active.insert(&txn).await?;
        set_rule_benefits(&txn, &saved, &benefit_ids).await?;
        recurring::regenerate(&txn, saved.id, cfg.today(), cfg.plan_months_ahead).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn update_recurring_rule(&self, ctx: &Context<'_>, id: i32, input: RecurringRuleInput) -> Result<recurring_rule::Model> {
        let uid = uid(ctx)?;
        validate(ctx, &input).await?;
        let db = db(ctx);
        let cfg = config(ctx);
        let found = recurring_rule::Entity::find_by_id(id).of_user(uid).one(db).await?.ok_or_else(|| user_error("반복 규칙이 없습니다"))?;
        let txn = db.begin().await?;
        let benefit_ids = input.benefit_ids.clone();
        let old_payment_method = found.payment_method_id;
        let mut active: recurring_rule::ActiveModel = found.into();
        apply(&mut active, input);
        let saved = active.update(&txn).await?;
        // 혜택은 카드에 딸려 있으므로 결제수단을 바꾸면 새 카드에서 쓸 수 없는 혜택은 버린다
        // (본인 카드 ↔ 가족카드는 혜택이 같아 그대로, 새로 고른 목록을 주면 그것으로)
        if let Some(ids) = benefit_ids {
            set_rule_benefits(&txn, &saved, &ids).await?;
        } else if saved.payment_method_id != old_payment_method {
            let keep: Vec<i32> = recurring_rule_benefit::Entity::find()
                .filter(recurring_rule_benefit::Column::RecurringRuleId.eq(saved.id))
                .all(&txn)
                .await?
                .into_iter()
                .map(|b| b.card_benefit_id)
                .collect();
            set_rule_benefits(&txn, &saved, &keep).await?;
        }
        recurring::regenerate(&txn, saved.id, cfg.today(), cfg.plan_months_ahead).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 규칙 삭제. 이번 달 이후의 예정 항목도 지운다 (확정된 거래는 남는다).
    async fn delete_recurring_rule(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        recurring_rule::Entity::find_by_id(id).of_user(uid).one(db).await?.ok_or_else(|| user_error("반복 규칙이 없습니다"))?;
        let txn = db.begin().await?;
        recurring::remove_future_planned(&txn, id, config(ctx).today()).await?;
        let res = recurring_rule::Entity::delete_many().of_user(uid).filter(recurring_rule::Column::Id.eq(id)).exec(&txn).await?;
        txn.commit().await?;
        Ok(res.rows_affected > 0)
    }

    /// 반복 규칙의 예정 거래를 지금 채운다 (서버가 주기적으로도 실행한다). 만든 개수를 돌려준다.
    async fn generate_planned(&self, ctx: &Context<'_>) -> Result<i32> {
        let cfg = config(ctx);
        Ok(recurring::ensure_planned(db(ctx), cfg.today(), cfg.plan_months_ahead, None).await? as i32)
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::PaginatorTrait;
    use serde_json::{Value, json};

    use super::*;

    async fn gql(schema: &crate::graphql::CashbookSchema, query: &str) -> Value {
        let res = schema.execute(crate::graphql::as_user(query, 1)).await;
        assert!(res.errors.is_empty(), "{query}: {:?}", res.errors);
        res.data.into_json().unwrap()
    }

    #[tokio::test]
    async fn rule_benefits_follow_card_and_go_to_planned() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger::ActiveModel {
            user_id: Set(1),
            name: Set("가계부".into()),
            description: Set(None),
            is_default: Set(true),
            sort_order: Set(0),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let schema = crate::graphql::build_schema(db.clone(), crate::config::AppConfig::for_tests());
        let card = |name: &str, benefits: &str| {
            format!(r#"mutation {{ createPaymentMethod(input: {{name: "{name}", kind: CREDIT_CARD, benefits: [{benefits}]}}) {{ id benefits {{ id }} }} }}"#)
        };
        let a = gql(&schema, &card("A", r#"{name: "10%", rate: 0.1}, {name: "5%", rate: 0.05}, {name: "+1%", rate: 0.01, stackable: true}"#)).await;
        let b = gql(&schema, &card("B", r#"{name: "B 2%", rate: 0.02}"#)).await;
        let id = |v: &Value| v.as_i64().unwrap();
        let (pm_a, pm_b) = (id(&a["createPaymentMethod"]["id"]), id(&b["createPaymentMethod"]["id"]));
        let ab: Vec<i64> = a["createPaymentMethod"]["benefits"].as_array().unwrap().iter().map(|x| id(&x["id"])).collect();
        let b1 = id(&b["createPaymentMethod"]["benefits"][0]["id"]);
        let input = |pm: i64, benefits: Option<&[i64]>| {
            let benefits = benefits.map(|ids| format!(", benefitIds: {ids:?}")).unwrap_or_default();
            format!(r#"{{ledgerId: {home}, description: "구독", amount: 9900, paymentMethodId: {pm}, dayOfMonth: 5, startMonth: "2026-01"{benefits}}}"#)
        };
        let state = |rule: i64| {
            let schema = schema.clone();
            async move {
                let r = gql(
                    &schema,
                    "{ recurringRules { id benefitIds } transactions(filter: {statuses: [PLANNED]}) { items { recurringRuleId benefitIds } } }",
                )
                .await;
                let rule_benefits = r["recurringRules"].as_array().unwrap().iter().find(|x| id(&x["id"]) == rule).unwrap()["benefitIds"].clone();
                let planned: Vec<Value> = r["transactions"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|t| t["recurringRuleId"].as_i64() == Some(rule))
                    .map(|t| t["benefitIds"].clone())
                    .collect();
                assert!(!planned.is_empty(), "예정 거래가 있어야 합니다");
                assert!(planned.iter().all(|p| *p == rule_benefits), "예정 거래 혜택 {planned:?} = 규칙 혜택 {rule_benefits}");
                rule_benefits
            }
        };

        // 다른 카드의 혜택은 버리고, 예정 거래에 그대로 붙는다
        let created = gql(
            &schema,
            &format!("mutation {{ createRecurringRule(input: {}) {{ id }} }}", input(pm_a, Some(&[ab[0], ab[2], b1]))),
        )
        .await;
        let rule = id(&created["createRecurringRule"]["id"]);
        assert_eq!(state(rule).await, json!([ab[0], ab[2]]));

        // 중복 적용이 아닌 혜택 둘은 함께 고를 수 없다
        let res = schema
            .execute(crate::graphql::as_user(
                &format!("mutation {{ updateRecurringRule(id: {rule}, input: {}) {{ id }} }}", input(pm_a, Some(&[ab[0], ab[1]]))),
                1,
            ))
            .await;
        assert!(res.errors[0].message.contains("함께 받을 수 없습니다"), "{:?}", res.errors);
        assert_eq!(state(rule).await, json!([ab[0], ab[2]]));

        // 혜택 목록 없이 고치면 같은 카드일 때는 그대로, 카드가 바뀌면 비운다
        let update = |pm: i64, benefits: Option<&[i64]>| format!("mutation {{ updateRecurringRule(id: {rule}, input: {}) {{ id }} }}", input(pm, benefits));
        gql(&schema, &update(pm_a, None)).await;
        assert_eq!(state(rule).await, json!([ab[0], ab[2]]));
        gql(&schema, &update(pm_b, None)).await;
        assert_eq!(state(rule).await, json!([]));
        gql(&schema, &update(pm_b, Some(&[b1]))).await;
        assert_eq!(state(rule).await, json!([b1]));

        // 규칙을 지우면 골라 둔 혜택도 지워진다
        gql(&schema, &format!("mutation {{ deleteRecurringRule(id: {rule}) }}")).await;
        assert_eq!(recurring_rule_benefit::Entity::find().count(&db).await.unwrap(), 0);
    }
}
