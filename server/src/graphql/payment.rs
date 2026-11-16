//! 결제수단(카드) API: 목록/상세, 정산 일정, 청구서, 실적·혜택 리포트, 추가/수정/삭제.

use std::collections::HashSet;

use async_graphql::{ComplexObject, Context, InputObject, Object, Result, SimpleObject};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};

use super::{DeleteResult, config, db, lookup, trim_opt, user_error};
use crate::domain::billing::{BillingCycle, INSTALLMENT_LOOKBACK_DAYS};
use crate::domain::card_report::{CardMonthReport, CardReportInput, month_report};
use crate::domain::dates::YearMonth;
use crate::domain::funds::{CardDue, StatementInfo, statement_parts, statement_range};
use crate::entity::{
    account, card_benefit, card_tier,
    enums::{BenefitKind, PaymentKind, TxStatus},
    payment_method, recurring_rule, transaction,
};

fn month_label(offset: i32) -> &'static str {
    match offset {
        0 => "당월",
        -1 => "전월",
        -2 => "전전월",
        _ => "전전전월",
    }
}

/// "전월 12일 ~ 당월 11일" 형태의 이용기간 설명
pub fn usage_period_label(cycle: &BillingCycle) -> String {
    let c = cycle.closing_day as i32;
    let off = cycle.closing_month_offset;
    if c >= 31 {
        format!("{} 1일 ~ {} 말일", month_label(off), month_label(off))
    } else {
        format!("{} {}일 ~ {} {}일", month_label(off - 1), c + 1, month_label(off), c)
    }
}

/// 카드의 청구서 계산에 필요한 거래 (최대 60개월 할부를 고려해 5년 전부터)
async fn card_transactions(
    db: &DatabaseConnection,
    payment_method_id: i32,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<transaction::Model>, sea_orm::DbErr> {
    transaction::Entity::find()
        .filter(transaction::Column::PaymentMethodId.eq(payment_method_id))
        .filter(transaction::Column::Status.ne(TxStatus::Skipped))
        .filter(transaction::Column::Date.gte(from - Duration::days(INSTALLMENT_LOOKBACK_DAYS)))
        .filter(transaction::Column::Date.lte(to))
        .order_by_asc(transaction::Column::Date)
        .order_by_asc(transaction::Column::Id)
        .all(db)
        .await
}

async fn report_for(ctx: &Context<'_>, pm: &payment_method::Model, month: YearMonth) -> Result<CardMonthReport> {
    let db = db(ctx);
    let lookup = lookup(ctx);
    let txs = transaction::Entity::find()
        .filter(transaction::Column::PaymentMethodId.eq(pm.id))
        .filter(transaction::Column::Date.gte(month.prev().first_day()))
        .filter(transaction::Column::Date.lte(month.last_day()))
        .all(db)
        .await?;
    let categories = lookup.categories().await?;
    let benefits = lookup.card_benefits_of(pm.id).await?;
    let tiers = lookup.tiers().await?;
    let tx_benefits = lookup.tx_benefits().await?;
    Ok(month_report(
        &CardReportInput {
            payment_method: pm,
            benefits: &benefits,
            tiers: tiers.get(&pm.id).map(Vec::as_slice).unwrap_or(&[]),
            categories: &categories,
            transactions: &txs,
            tx_benefits: &tx_benefits,
        },
        month,
    ))
}

#[ComplexObject]
impl payment_method::Model {
    /// 자동 입력 매칭용 별칭
    async fn aliases(&self) -> Vec<String> {
        self.alias_list()
    }

    async fn is_card(&self) -> bool {
        self.kind.is_card()
    }

    /// 신용카드 정산 일정(결제일·이용기간)이 입력되었는지
    async fn billing_configured(&self) -> bool {
        BillingCycle::from_model(self).is_some()
    }

    /// 이용기간 설명 (예: 전월 12일 ~ 당월 11일)
    async fn usage_period_label(&self) -> Option<String> {
        BillingCycle::from_model(self).map(|c| usage_period_label(&c))
    }

    /// 결제(출금) 계좌
    async fn account(&self, ctx: &Context<'_>) -> Result<Option<account::Model>> {
        let Some(id) = self.account_id else { return Ok(None) };
        Ok(lookup(ctx).accounts().await?.get(&id).cloned())
    }

    async fn benefits(&self, ctx: &Context<'_>) -> Result<Vec<card_benefit::Model>> {
        Ok(lookup(ctx).card_benefits_of(self.id).await?)
    }

    /// 전월실적 구간
    async fn tiers(&self, ctx: &Context<'_>) -> Result<Vec<card_tier::Model>> {
        Ok(lookup(ctx).tiers().await?.get(&self.id).cloned().unwrap_or_default())
    }

    async fn transaction_count(&self, ctx: &Context<'_>) -> Result<i64> {
        Ok(transaction::Entity::find()
            .filter(transaction::Column::PaymentMethodId.eq(self.id))
            .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
            .count(db(ctx))
            .await? as i64)
    }

    async fn last_used_on(&self, ctx: &Context<'_>) -> Result<Option<NaiveDate>> {
        Ok(transaction::Entity::find()
            .select_only()
            .column(transaction::Column::Date)
            .filter(transaction::Column::PaymentMethodId.eq(self.id))
            .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
            .order_by_desc(transaction::Column::Date)
            .into_tuple::<NaiveDate>()
            .one(db(ctx))
            .await?)
    }

    /// 결제월 범위의 청구서. 기본은 다음 결제 기준 3개월 전 ~ 2개월 뒤.
    async fn statements(
        &self,
        ctx: &Context<'_>,
        from: Option<YearMonth>,
        to: Option<YearMonth>,
    ) -> Result<Vec<StatementInfo>> {
        let Some(cycle) = BillingCycle::from_model(self) else { return Ok(vec![]) };
        let today = config(ctx).today();
        let next = cycle.next_statement(today);
        let from = from.unwrap_or(next.add_months(-3));
        let to = to.unwrap_or(next.add_months(2)).max(from);
        if from.months_until(to) > 60 {
            return Err(user_error("한 번에 60개월까지 조회할 수 있습니다"));
        }
        let txs = card_transactions(db(ctx), self.id, cycle.period(from).0, cycle.period(to).1).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        Ok(statement_range(self, &cycle, &refs, &categories, today, from, to))
    }

    /// 다음 결제 예정 청구서
    async fn next_payment(&self, ctx: &Context<'_>) -> Result<Option<StatementInfo>> {
        let Some(cycle) = BillingCycle::from_model(self) else { return Ok(None) };
        let today = config(ctx).today();
        let next = cycle.next_statement(today);
        let txs = card_transactions(db(ctx), self.id, cycle.period(next).0, cycle.period(next).1).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        Ok(statement_range(self, &cycle, &refs, &categories, today, next, next).pop())
    }

    /// 월별 실적·혜택 리포트
    async fn month_report(&self, ctx: &Context<'_>, month: YearMonth) -> Result<CardMonthReport> {
        report_for(ctx, self, month).await
    }
}

#[ComplexObject]
impl CardDue {
    async fn payment_method(&self, ctx: &Context<'_>) -> Result<Option<payment_method::Model>> {
        Ok(lookup(ctx).payment_methods().await?.get(&self.payment_method_id).cloned())
    }
}

/// 청구서에 들어가는 거래 한 회차
#[derive(SimpleObject)]
pub struct StatementLine {
    pub transaction: transaction::Model,
    /// 이번 청구서에 청구되는 금액 (할부면 회차 금액)
    pub amount: i64,
    pub installment_no: i32,
    pub installment_months: i32,
    pub planned: bool,
}

#[derive(SimpleObject)]
pub struct BillingSuggestion {
    pub payment_day: i32,
    pub closing_day: i32,
    pub closing_month_offset: i32,
    pub usage_period_label: String,
}

#[derive(Default)]
pub struct PaymentQuery;

#[Object]
impl PaymentQuery {
    /// 결제수단 목록. `cardsOnly`면 신용/체크카드만.
    async fn payment_methods(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = false)] include_archived: bool,
        #[graphql(default = false)] cards_only: bool,
    ) -> Result<Vec<payment_method::Model>> {
        let mut q = payment_method::Entity::find();
        if !include_archived {
            q = q.filter(payment_method::Column::Archived.eq(false));
        }
        if cards_only {
            q = q.filter(payment_method::Column::Kind.is_in([PaymentKind::CreditCard, PaymentKind::CheckCard]));
        }
        Ok(q.order_by_asc(payment_method::Column::Archived)
            .order_by_asc(payment_method::Column::SortOrder)
            .order_by_asc(payment_method::Column::Id)
            .all(db(ctx))
            .await?)
    }

    async fn payment_method(&self, ctx: &Context<'_>, id: i32) -> Result<Option<payment_method::Model>> {
        Ok(payment_method::Entity::find_by_id(id).one(db(ctx)).await?)
    }

    /// 카드별 월 실적·혜택 요약 (카드별 지출내역 화면). 보관된 카드는 그 달 거래가 있을 때만.
    async fn cards_monthly(&self, ctx: &Context<'_>, month: YearMonth) -> Result<Vec<CardMonthReport>> {
        let db = db(ctx);
        let used: HashSet<i32> = transaction::Entity::find()
            .select_only()
            .column(transaction::Column::PaymentMethodId)
            .filter(transaction::Column::Date.gte(month.first_day()))
            .filter(transaction::Column::Date.lte(month.last_day()))
            .filter(transaction::Column::PaymentMethodId.is_not_null())
            .distinct()
            .into_tuple::<Option<i32>>()
            .all(db)
            .await?
            .into_iter()
            .flatten()
            .collect();
        let mut cards: Vec<payment_method::Model> = payment_method::Entity::find()
            .filter(payment_method::Column::Kind.is_in([PaymentKind::CreditCard, PaymentKind::CheckCard]))
            .all(db)
            .await?
            .into_iter()
            .filter(|p| !p.archived || used.contains(&p.id))
            .collect();
        cards.sort_by_key(|p| (p.archived, p.sort_order, p.id));
        let mut out = Vec::with_capacity(cards.len());
        for pm in &cards {
            out.push(report_for(ctx, pm, month).await?);
        }
        Ok(out)
    }

    /// 결제월 `month` 청구서에 들어가는 거래들
    async fn statement_lines(
        &self,
        ctx: &Context<'_>,
        payment_method_id: i32,
        month: YearMonth,
    ) -> Result<Vec<StatementLine>> {
        let db = db(ctx);
        let pm = payment_method::Entity::find_by_id(payment_method_id)
            .one(db)
            .await?
            .ok_or_else(|| user_error("결제수단이 없습니다"))?;
        let Some(cycle) = BillingCycle::from_model(&pm) else {
            return Err(user_error("정산 일정이 입력되지 않은 카드입니다"));
        };
        let (start, end) = cycle.period(month);
        let txs = card_transactions(db, pm.id, start, end).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        let parts = statement_parts(&pm, &cycle, &refs, &categories, month);
        let mut lines: Vec<StatementLine> = parts
            .into_iter()
            .filter_map(|p| {
                let tx = txs.iter().find(|t| t.id == p.tx_id)?.clone();
                Some(StatementLine {
                    transaction: tx,
                    amount: p.amount,
                    installment_no: p.installment_no,
                    installment_months: p.installment_months,
                    planned: p.planned,
                })
            })
            .collect();
        lines.sort_by_key(|l| (l.transaction.date, l.transaction.id));
        Ok(lines)
    }

    /// 결제일만 알 때 카드사 일반 기준(결제일 약 14일 전 마감)으로 이용기간을 추정한다.
    async fn suggested_billing(&self, payment_day: i32) -> Result<BillingSuggestion> {
        if !(1..=31).contains(&payment_day) {
            return Err(user_error("결제일은 1~31 사이여야 합니다"));
        }
        let c = BillingCycle::suggested(payment_day as u32);
        Ok(BillingSuggestion {
            payment_day,
            closing_day: c.closing_day as i32,
            closing_month_offset: c.closing_month_offset,
            usage_period_label: usage_period_label(&c),
        })
    }
}

#[derive(InputObject, Clone)]
pub struct CardBenefitInput {
    /// 기존 혜택을 수정할 때 id
    pub id: Option<i32>,
    pub name: String,
    #[graphql(default)]
    pub kind: BenefitKind,
    /// 이용금액 1원당 혜택 (할인·포인트 0.1 = 10%, 마일리지 1/1500 = 1,500원당 1마일)
    pub rate: f64,
    pub min_amount: Option<i64>,
    pub per_tx_max: Option<i64>,
    pub monthly_cap: Option<i64>,
    #[graphql(default = true)]
    pub shares_card_limit: bool,
    #[graphql(default = false)]
    pub excludes_performance: bool,
    /// 다른 혜택과 함께 받을 수 있는 혜택
    #[graphql(default = false)]
    pub stackable: bool,
}

#[derive(InputObject, Clone)]
pub struct CardTierInput {
    pub min_spending: i64,
    pub benefit_limit: i64,
}

#[derive(InputObject)]
pub struct PaymentMethodInput {
    pub name: String,
    pub kind: PaymentKind,
    pub issuer: Option<String>,
    pub product_name: Option<String>,
    pub last4: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub account_id: Option<i32>,
    pub payment_day: Option<i32>,
    pub closing_day: Option<i32>,
    pub closing_month_offset: Option<i32>,
    pub monthly_target: Option<i64>,
    pub annual_fee: Option<i64>,
    /// 유효기간 YYYY-MM
    pub expires_on: Option<String>,
    pub color: Option<String>,
    pub memo: Option<String>,
    pub archived: Option<bool>,
    /// 주면 혜택 목록을 이 내용으로 맞춘다 (id 있는 항목은 수정, 없는 항목은 추가, 빠진 항목은 삭제)
    pub benefits: Option<Vec<CardBenefitInput>>,
    /// 주면 전월실적 구간을 이 내용으로 바꾼다
    pub tiers: Option<Vec<CardTierInput>>,
}

struct Validated {
    name: String,
    last4: Option<String>,
    aliases: Option<String>,
    billing: (Option<i32>, Option<i32>, Option<i32>),
    expires_on: Option<String>,
}

fn validate(input: &PaymentMethodInput) -> Result<Validated> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(user_error("이름을 입력하세요"));
    }
    let last4 = trim_opt(input.last4.clone());
    if let Some(l) = &last4
        && (l.len() != 4 || !l.chars().all(|c| c.is_ascii_digit())) {
            return Err(user_error("카드번호 끝자리는 숫자 4자리입니다"));
        }
    let aliases = input.aliases.as_ref().map(|a| {
        a.iter().map(|s| s.trim().replace(',', " ")).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(",")
    });
    let billing = (input.payment_day, input.closing_day, input.closing_month_offset);
    match billing {
        (None, None, None) => {}
        (Some(p), Some(c), Some(o)) => {
            if input.kind != PaymentKind::CreditCard {
                return Err(user_error("정산 일정은 신용카드에만 입력합니다"));
            }
            let cycle = BillingCycle {
                payment_day: u32::try_from(p).map_err(|_| user_error("결제일이 올바르지 않습니다"))?,
                closing_day: u32::try_from(c).map_err(|_| user_error("마감일이 올바르지 않습니다"))?,
                closing_month_offset: o,
            };
            cycle.validate().map_err(user_error)?;
        }
        _ => return Err(user_error("결제일, 마감일, 마감월을 모두 입력하세요")),
    }
    let expires_on = trim_opt(input.expires_on.clone());
    if let Some(e) = &expires_on
        && YearMonth::parse(e).is_none() {
            return Err(user_error("유효기간은 YYYY-MM 형식입니다"));
        }
    if let Some(benefits) = &input.benefits {
        for b in benefits {
            if b.name.trim().is_empty() {
                return Err(user_error("혜택 이름을 입력하세요"));
            }
            if !(0.0..=1.0).contains(&b.rate) {
                return Err(user_error("혜택률은 0~1 사이(예: 0.1 = 10%)입니다"));
            }
        }
    }
    Ok(Validated { name, last4, aliases: aliases.filter(|a| !a.is_empty()), billing, expires_on })
}

async fn save_children<C: sea_orm::ConnectionTrait>(
    db: &C,
    pm_id: i32,
    benefits: Option<Vec<CardBenefitInput>>,
    tiers: Option<Vec<CardTierInput>>,
) -> Result<()> {
    if let Some(list) = benefits {
        let existing = card_benefit::Entity::find()
            .filter(card_benefit::Column::PaymentMethodId.eq(pm_id))
            .all(db)
            .await?;
        let keep: HashSet<i32> = list.iter().filter_map(|b| b.id).collect();
        let remove: Vec<i32> = existing.iter().map(|b| b.id).filter(|id| !keep.contains(id)).collect();
        if !remove.is_empty() {
            card_benefit::Entity::delete_many().filter(card_benefit::Column::Id.is_in(remove)).exec(db).await?;
        }
        for (i, b) in list.into_iter().enumerate() {
            let mut active = match b.id.and_then(|id| existing.iter().find(|e| e.id == id)) {
                Some(e) => card_benefit::ActiveModel::from(e.clone()),
                None => card_benefit::ActiveModel { payment_method_id: Set(pm_id), ..Default::default() },
            };
            active.name = Set(b.name.trim().to_string());
            active.kind = Set(b.kind);
            active.rate = Set(b.rate);
            active.min_amount = Set(b.min_amount.filter(|v| *v > 0));
            active.per_tx_max = Set(b.per_tx_max.filter(|v| *v > 0));
            active.monthly_cap = Set(b.monthly_cap.filter(|v| *v >= 0));
            active.shares_card_limit = Set(b.shares_card_limit);
            active.excludes_performance = Set(b.excludes_performance);
            active.stackable = Set(b.stackable);
            active.sort_order = Set(i as i32);
            active.save(db).await?;
        }
    }
    if let Some(list) = tiers {
        card_tier::Entity::delete_many().filter(card_tier::Column::PaymentMethodId.eq(pm_id)).exec(db).await?;
        for t in list {
            card_tier::ActiveModel {
                payment_method_id: Set(pm_id),
                min_spending: Set(t.min_spending),
                benefit_limit: Set(t.benefit_limit),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct PaymentMutation;

#[Object]
impl PaymentMutation {
    /// 카드/결제수단 추가
    async fn create_payment_method(&self, ctx: &Context<'_>, input: PaymentMethodInput) -> Result<payment_method::Model> {
        let db = db(ctx);
        let v = validate(&input)?;
        if payment_method::Entity::find().filter(payment_method::Column::Name.eq(v.name.as_str())).one(db).await?.is_some() {
            return Err(user_error("같은 이름의 결제수단이 이미 있습니다"));
        }
        let count = payment_method::Entity::find().count(db).await?;
        let txn = db.begin().await?;
        let saved = payment_method::ActiveModel {
            name: Set(v.name),
            kind: Set(input.kind),
            issuer: Set(trim_opt(input.issuer)),
            product_name: Set(trim_opt(input.product_name)),
            last4: Set(v.last4),
            aliases: Set(v.aliases),
            account_id: Set(input.account_id),
            payment_day: Set(v.billing.0),
            closing_day: Set(v.billing.1),
            closing_month_offset: Set(v.billing.2),
            monthly_target: Set(input.monthly_target.filter(|t| *t > 0)),
            annual_fee: Set(input.annual_fee),
            expires_on: Set(v.expires_on),
            color: Set(trim_opt(input.color)),
            memo: Set(trim_opt(input.memo)),
            sort_order: Set(count as i32),
            archived: Set(input.archived.unwrap_or(false)),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        save_children(&txn, saved.id, input.benefits, input.tiers).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 카드/결제수단 수정 (폼 전체를 보낸다)
    async fn update_payment_method(
        &self,
        ctx: &Context<'_>,
        id: i32,
        input: PaymentMethodInput,
    ) -> Result<payment_method::Model> {
        let db = db(ctx);
        let found = payment_method::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
        let v = validate(&input)?;
        if let Some(other) = payment_method::Entity::find()
            .filter(payment_method::Column::Name.eq(v.name.as_str()))
            .filter(payment_method::Column::Id.ne(id))
            .one(db)
            .await?
        {
            return Err(user_error(format!("'{}' 이름은 이미 쓰고 있습니다", other.name)));
        }
        let txn = db.begin().await?;
        let mut active: payment_method::ActiveModel = found.into();
        active.name = Set(v.name);
        active.kind = Set(input.kind);
        active.issuer = Set(trim_opt(input.issuer));
        active.product_name = Set(trim_opt(input.product_name));
        active.last4 = Set(v.last4);
        if input.aliases.is_some() {
            active.aliases = Set(v.aliases);
        }
        active.account_id = Set(input.account_id);
        active.payment_day = Set(v.billing.0);
        active.closing_day = Set(v.billing.1);
        active.closing_month_offset = Set(v.billing.2);
        active.monthly_target = Set(input.monthly_target.filter(|t| *t > 0));
        active.annual_fee = Set(input.annual_fee);
        active.expires_on = Set(v.expires_on);
        active.color = Set(trim_opt(input.color));
        active.memo = Set(trim_opt(input.memo));
        if let Some(a) = input.archived {
            active.archived = Set(a);
        }
        let saved = active.update(&txn).await?;
        save_children(&txn, saved.id, input.benefits, input.tiers).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 거래가 있는 결제수단은 보관 처리하고(내역 유지), 없으면 지운다.
    async fn delete_payment_method(&self, ctx: &Context<'_>, id: i32) -> Result<DeleteResult> {
        let db = db(ctx);
        let found = payment_method::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
        let used = transaction::Entity::find().filter(transaction::Column::PaymentMethodId.eq(id)).count(db).await?;
        let result = if used > 0 {
            let txn = db.begin().await?;
            let mut active: payment_method::ActiveModel = found.into();
            active.archived = Set(true);
            active.update(&txn).await?;
            // 앞으로 생길 예정 항목이 보관된 카드로 잡히지 않게 반복 규칙은 끄고, 이미 만든 예정분도 지운다.
            let rule_ids: Vec<i32> = recurring_rule::Entity::find()
                .filter(recurring_rule::Column::PaymentMethodId.eq(id))
                .all(&txn)
                .await?
                .into_iter()
                .map(|r| r.id)
                .collect();
            recurring_rule::Entity::update_many()
                .col_expr(recurring_rule::Column::Active, false.into())
                .filter(recurring_rule::Column::PaymentMethodId.eq(id))
                .exec(&txn)
                .await?;
            let today = config(ctx).today();
            for rule_id in rule_ids {
                crate::domain::recurring::remove_future_planned(&txn, rule_id, today).await?;
            }
            txn.commit().await?;
            DeleteResult { deleted: false, archived: true }
        } else {
            payment_method::Entity::delete_by_id(id).exec(db).await?;
            DeleteResult { deleted: true, archived: false }
        };
        lookup(ctx).invalidate();
        Ok(result)
    }

    /// 보관된 결제수단을 다시 사용
    async fn restore_payment_method(&self, ctx: &Context<'_>, id: i32) -> Result<payment_method::Model> {
        let db = db(ctx);
        let found = payment_method::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
        let mut active: payment_method::ActiveModel = found.into();
        active.archived = Set(false);
        let saved = active.update(db).await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn reorder_payment_methods(&self, ctx: &Context<'_>, ids: Vec<i32>) -> Result<bool> {
        let db = db(ctx);
        let txn = db.begin().await?;
        for (i, id) in ids.iter().enumerate() {
            payment_method::Entity::update_many()
                .col_expr(payment_method::Column::SortOrder, (i as i32).into())
                .filter(Condition::all().add(payment_method::Column::Id.eq(*id)))
                .exec(&txn)
                .await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        let c = BillingCycle { payment_day: 25, closing_day: 11, closing_month_offset: 0 };
        assert_eq!(usage_period_label(&c), "전월 12일 ~ 당월 11일");
        let c = BillingCycle { payment_day: 14, closing_day: 31, closing_month_offset: -1 };
        assert_eq!(usage_period_label(&c), "전월 1일 ~ 전월 말일");
        let c = BillingCycle { payment_day: 1, closing_day: 18, closing_month_offset: -1 };
        assert_eq!(usage_period_label(&c), "전전월 19일 ~ 전월 18일");
    }
}
