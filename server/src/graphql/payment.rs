//! 결제수단(카드) API: 목록/상세, 정산 일정, 청구서, 실적·혜택 리포트, 추가/수정/삭제.
//! 가족카드(`parent_id`)는 본인 카드의 혜택·구간·목표를 쓰고 실적을 본인 카드와 합쳐 계산한다.
//! 종류·결제 계좌·정산 일정은 본인 카드 것을 복사해 두고 본인 카드를 고칠 때 같이 맞춘다.
//! 가족카드 대금은 본인 카드 청구서에 합쳐 청구되므로 본인 카드 청구서는 가족카드 이용까지, 가족카드 청구서는 그 카드 이용분만 담는다.

use std::collections::{HashMap, HashSet};

use async_graphql::{ComplexObject, Context, InputObject, Object, Result, SimpleObject};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait, sea_query::Query,
};

use super::{DeleteResult, config, db, lookup, trim_opt, user_error};
use crate::domain::billing::{BillingCycle, INSTALLMENT_LOOKBACK_DAYS};
use crate::domain::card_report::{CardMonthReport, CardReportInput, month_report};
use crate::domain::dates::YearMonth;
use crate::domain::funds::{CardDue, StatementInfo, statement_parts, statement_range};
use crate::entity::{
    account, card_benefit, card_tier,
    enums::{BenefitKind, PaymentKind, TxStatus},
    payment_method, recurring_rule, recurring_rule_benefit, transaction, transaction_benefit,
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

/// 청구서에 넣는 카드: 본인 카드는 가족카드까지(가족카드 대금은 본인 카드로 청구), 가족카드는 자기 자신만.
async fn statement_card_ids(ctx: &Context<'_>, pm: &payment_method::Model) -> Result<Vec<i32>, DbErr> {
    let mut ids = vec![pm.id];
    if pm.parent_id.is_none() {
        let pms = lookup(ctx).payment_methods().await?;
        ids.extend(pms.values().filter(|p| p.parent_id == Some(pm.id)).map(|p| p.id));
    }
    Ok(ids)
}

/// 카드의 청구서 계산에 필요한 거래 (최대 60개월 할부를 고려해 5년 전부터). `card_ids`는 `statement_card_ids`.
async fn card_transactions(
    db: &DatabaseConnection,
    card_ids: Vec<i32>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<transaction::Model>, sea_orm::DbErr> {
    transaction::Entity::find()
        .filter(transaction::Column::PaymentMethodId.is_in(card_ids))
        .filter(transaction::Column::Status.ne(TxStatus::Skipped))
        .filter(transaction::Column::Date.gte(from - Duration::days(INSTALLMENT_LOOKBACK_DAYS)))
        .filter(transaction::Column::Date.lte(to))
        .order_by_asc(transaction::Column::Date)
        .order_by_asc(transaction::Column::Id)
        .all(db)
        .await
}

/// 실적·혜택을 함께 쓰는 묶음: (본인 카드, 본인 카드와 그 가족카드들의 id). 가족카드가 없으면 자기 자신만.
fn card_group(pms: &HashMap<i32, payment_method::Model>, pm: &payment_method::Model) -> (payment_method::Model, Vec<i32>) {
    let head = pm.parent_id.and_then(|id| pms.get(&id)).unwrap_or(pm).clone();
    let mut ids: Vec<i32> = pms.values().filter(|p| p.parent_id == Some(head.id)).map(|p| p.id).collect();
    ids.push(head.id);
    if !ids.contains(&pm.id) {
        ids.push(pm.id);
    }
    ids.sort_unstable();
    (head, ids)
}

async fn report_for(ctx: &Context<'_>, pm: &payment_method::Model, month: YearMonth) -> Result<CardMonthReport> {
    let db = db(ctx);
    let lookup = lookup(ctx);
    let (head, group_ids) = card_group(&*lookup.payment_methods().await?, pm);
    let txs = transaction::Entity::find()
        .filter(transaction::Column::PaymentMethodId.is_in(group_ids.clone()))
        .filter(transaction::Column::Date.gte(month.prev().first_day()))
        .filter(transaction::Column::Date.lte(month.last_day()))
        .all(db)
        .await?;
    let categories = lookup.categories().await?;
    let benefits = lookup.card_benefits_of(head.id).await?;
    let tiers = lookup.tiers().await?;
    let tx_benefits = lookup.tx_benefits().await?;
    Ok(month_report(
        &CardReportInput {
            payment_method: pm,
            monthly_target: head.monthly_target,
            group_ids: &group_ids,
            benefits: &benefits,
            tiers: tiers.get(&head.id).map(Vec::as_slice).unwrap_or(&[]),
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

    /// 카드 혜택. 가족카드는 본인 카드의 혜택 (거래에 고르는 혜택도 이것)
    async fn benefits(&self, ctx: &Context<'_>) -> Result<Vec<card_benefit::Model>> {
        Ok(lookup(ctx).card_benefits_of(self.benefit_card_id()).await?)
    }

    /// 전월실적 구간. 가족카드는 본인 카드의 구간
    async fn tiers(&self, ctx: &Context<'_>) -> Result<Vec<card_tier::Model>> {
        Ok(lookup(ctx).tiers().await?.get(&self.benefit_card_id()).cloned().unwrap_or_default())
    }

    /// 가족카드의 본인 카드
    async fn parent(&self, ctx: &Context<'_>) -> Result<Option<payment_method::Model>> {
        let Some(id) = self.parent_id else { return Ok(None) };
        Ok(lookup(ctx).payment_methods().await?.get(&id).cloned())
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
    /// 본인 카드는 가족카드 이용을 합친 금액, 가족카드는 그 카드 이용분(본인 카드 청구서에 합산됨).
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
        let ids = statement_card_ids(ctx, self).await?;
        let txs = card_transactions(db(ctx), ids, cycle.period(from).0, cycle.period(to).1).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        Ok(statement_range(&cycle, &refs, &categories, today, from, to))
    }

    /// 다음 결제 예정 청구서 (본인 카드는 가족카드 이용 포함)
    async fn next_payment(&self, ctx: &Context<'_>) -> Result<Option<StatementInfo>> {
        let Some(cycle) = BillingCycle::from_model(self) else { return Ok(None) };
        let today = config(ctx).today();
        let next = cycle.next_statement(today);
        let ids = statement_card_ids(ctx, self).await?;
        let txs = card_transactions(db(ctx), ids, cycle.period(next).0, cycle.period(next).1).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        Ok(statement_range(&cycle, &refs, &categories, today, next, next).pop())
    }

    /// 월별 실적·혜택 리포트 (가족카드 묶음은 실적·혜택을 합쳐서, 이용금액은 이 카드만)
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
    /// 가족카드는 실적을 본인 카드 리포트에 합치므로 `includeFamily`일 때만 넣는다 (이용금액은 카드별).
    async fn cards_monthly(
        &self,
        ctx: &Context<'_>,
        month: YearMonth,
        #[graphql(default = false)] include_family: bool,
    ) -> Result<Vec<CardMonthReport>> {
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
            .filter(|p| include_family || p.parent_id.is_none())
            .collect();
        cards.sort_by_key(|p| (p.archived, p.sort_order, p.id));
        let mut out = Vec::with_capacity(cards.len());
        for pm in &cards {
            out.push(report_for(ctx, pm, month).await?);
        }
        Ok(out)
    }

    /// 결제월 `month` 청구서에 들어가는 거래들. 본인 카드는 가족카드 거래도 담는다 (거래의 `paymentMethodId`로 구분)
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
        let ids = statement_card_ids(ctx, &pm).await?;
        let txs = card_transactions(db, ids, start, end).await?;
        let refs: Vec<&transaction::Model> = txs.iter().collect();
        let categories = lookup(ctx).categories().await?;
        let parts = statement_parts(&cycle, &refs, &categories, month);
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
    /// 가족카드면 본인 카드 id. 종류·결제 계좌·정산 일정은 본인 카드를 따르고, 실적 목표·혜택·구간은 본인 카드 것을 쓴다
    /// (이 카드의 값은 무시하고 비운다). 수정할 때 빼면 가족카드가 아니게 된다.
    pub parent_id: Option<i32>,
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

async fn has_family<C: ConnectionTrait>(db: &C, id: i32) -> Result<bool, DbErr> {
    Ok(payment_method::Entity::find().filter(payment_method::Column::ParentId.eq(id)).count(db).await? > 0)
}

/// 가족카드 입력을 본인 카드에 맞춘다: 종류·결제 계좌·정산 일정은 본인 카드 것, 실적 목표·혜택·구간은 비운다.
/// `found`는 수정하는 카드(추가면 None).
async fn apply_parent<C: ConnectionTrait>(
    db: &C,
    found: Option<&payment_method::Model>,
    input: &mut PaymentMethodInput,
) -> Result<()> {
    let Some(pid) = input.parent_id else { return Ok(()) };
    if found.is_some_and(|f| f.id == pid) {
        return Err(user_error("자기 자신을 본인 카드로 고를 수 없습니다"));
    }
    let parent = payment_method::Entity::find_by_id(pid).one(db).await?.ok_or_else(|| user_error("본인 카드가 없습니다"))?;
    if parent.parent_id.is_some() {
        return Err(user_error(format!("'{}'은(는) 가족카드라 본인 카드로 고를 수 없습니다", parent.name)));
    }
    if !parent.kind.is_card() {
        return Err(user_error("본인 카드는 신용카드나 체크카드여야 합니다"));
    }
    if parent.archived && found.is_none_or(|f| f.parent_id != Some(pid)) {
        return Err(user_error(format!("보관된 카드 '{}'에는 가족카드를 추가할 수 없습니다", parent.name)));
    }
    if let Some(f) = found
        && has_family(db, f.id).await?
    {
        return Err(user_error("가족카드가 있는 카드는 가족카드로 바꿀 수 없습니다"));
    }
    input.kind = parent.kind;
    input.account_id = parent.account_id;
    input.payment_day = parent.payment_day;
    input.closing_day = parent.closing_day;
    input.closing_month_offset = parent.closing_month_offset;
    input.monthly_target = None;
    input.benefits = Some(vec![]);
    input.tiers = Some(vec![]);
    if trim_opt(input.issuer.clone()).is_none() {
        input.issuer = parent.issuer;
    }
    Ok(())
}

/// 본인 카드의 종류·결제 계좌·정산 일정을 가족카드에 복사한다.
async fn sync_family<C: ConnectionTrait>(db: &C, parent: &payment_method::Model) -> Result<(), DbErr> {
    let family = payment_method::Entity::find().filter(payment_method::Column::ParentId.eq(parent.id)).all(db).await?;
    for f in family {
        let mut active: payment_method::ActiveModel = f.into();
        active.kind = Set(parent.kind);
        active.account_id = Set(parent.account_id);
        active.payment_day = Set(parent.payment_day);
        active.closing_day = Set(parent.closing_day);
        active.closing_month_offset = Set(parent.closing_month_offset);
        active.update(db).await?;
    }
    Ok(())
}

/// 카드의 거래·반복 규칙에 붙은 혜택 중 지금 혜택 카드(가족카드면 본인 카드)의 것이 아닌 것을 지운다.
/// 가족카드로 바꾸거나 본인 카드를 바꾸거나 가족카드에서 풀 때 쓴다.
async fn prune_foreign_benefits<C: ConnectionTrait>(db: &C, pm: &payment_method::Model) -> Result<(), DbErr> {
    let valid: Vec<i32> = card_benefit::Entity::find()
        .select_only()
        .column(card_benefit::Column::Id)
        .filter(card_benefit::Column::PaymentMethodId.eq(pm.benefit_card_id()))
        .into_tuple()
        .all(db)
        .await?;
    let txs = Query::select()
        .column(transaction::Column::Id)
        .from(transaction::Entity)
        .and_where(transaction::Column::PaymentMethodId.eq(pm.id))
        .to_owned();
    let mut q = transaction_benefit::Entity::delete_many().filter(transaction_benefit::Column::TransactionId.in_subquery(txs));
    if !valid.is_empty() {
        q = q.filter(transaction_benefit::Column::CardBenefitId.is_not_in(valid.clone()));
    }
    q.exec(db).await?;
    let rules = Query::select()
        .column(recurring_rule::Column::Id)
        .from(recurring_rule::Entity)
        .and_where(recurring_rule::Column::PaymentMethodId.eq(pm.id))
        .to_owned();
    let mut q = recurring_rule_benefit::Entity::delete_many()
        .filter(recurring_rule_benefit::Column::RecurringRuleId.in_subquery(rules));
    if !valid.is_empty() {
        q = q.filter(recurring_rule_benefit::Column::CardBenefitId.is_not_in(valid));
    }
    q.exec(db).await?;
    Ok(())
}

/// 결제수단을 보관한다. 앞으로 생길 예정 항목이 보관된 카드로 잡히지 않게 반복 규칙은 끄고, 이미 만든 예정분도 지운다.
async fn archive<C: ConnectionTrait>(db: &C, pm: payment_method::Model, today: NaiveDate) -> Result<(), DbErr> {
    let id = pm.id;
    let mut active: payment_method::ActiveModel = pm.into();
    active.archived = Set(true);
    active.update(db).await?;
    let rule_ids: Vec<i32> = recurring_rule::Entity::find()
        .filter(recurring_rule::Column::PaymentMethodId.eq(id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();
    recurring_rule::Entity::update_many()
        .col_expr(recurring_rule::Column::Active, false.into())
        .filter(recurring_rule::Column::PaymentMethodId.eq(id))
        .exec(db)
        .await?;
    for rule_id in rule_ids {
        crate::domain::recurring::remove_future_planned(db, rule_id, today).await?;
    }
    Ok(())
}

async fn save_children<C: ConnectionTrait>(
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
        let mut input = input;
        apply_parent(db, None, &mut input).await?;
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
            parent_id: Set(input.parent_id),
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
        let mut input = input;
        apply_parent(db, Some(&found), &mut input).await?;
        if !input.kind.is_card() && has_family(db, id).await? {
            return Err(user_error("가족카드가 있는 카드는 카드가 아닌 종류로 바꿀 수 없습니다"));
        }
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
        let old_parent = found.parent_id;
        let mut active: payment_method::ActiveModel = found.into();
        active.parent_id = Set(input.parent_id);
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
        if saved.parent_id != old_parent {
            prune_foreign_benefits(&txn, &saved).await?;
        }
        sync_family(&txn, &saved).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 거래가 있는 결제수단은 보관 처리하고(내역 유지), 없으면 지운다.
    /// 가족카드가 있는 본인 카드도 지우지 않고 보관하며, 가족카드도 함께 보관한다.
    async fn delete_payment_method(&self, ctx: &Context<'_>, id: i32) -> Result<DeleteResult> {
        let db = db(ctx);
        let found = payment_method::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
        let used = transaction::Entity::find().filter(transaction::Column::PaymentMethodId.eq(id)).count(db).await?;
        let family = payment_method::Entity::find().filter(payment_method::Column::ParentId.eq(id)).all(db).await?;
        let result = if used > 0 || !family.is_empty() {
            let today = config(ctx).today();
            let txn = db.begin().await?;
            archive(&txn, found, today).await?;
            for f in family.into_iter().filter(|f| !f.archived) {
                archive(&txn, f, today).await?;
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
        if let Some(pid) = found.parent_id
            && let Some(parent) = payment_method::Entity::find_by_id(pid).one(db).await?
            && parent.archived
        {
            return Err(user_error(format!("본인 카드 '{}'가 보관되어 있습니다. 본인 카드를 먼저 다시 사용하세요", parent.name)));
        }
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
    use serde_json::{Value, json};

    use super::*;
    use crate::entity::ledger;

    async fn gql(schema: &crate::graphql::CashbookSchema, query: &str) -> Value {
        let res = schema.execute(query).await;
        assert!(res.errors.is_empty(), "{query}: {:?}", res.errors);
        res.data.into_json().unwrap()
    }

    async fn gql_error(schema: &crate::graphql::CashbookSchema, query: &str) -> String {
        let res = schema.execute(query).await;
        res.errors.first().unwrap_or_else(|| panic!("{query}: 오류가 나야 합니다")).message.clone()
    }

    #[tokio::test]
    async fn family_card_shares_parent_performance_and_benefits() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger::ActiveModel {
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
        let id = |v: &Value| v.as_i64().unwrap();
        let fields = "id kind parentId issuer paymentDay closingDay closingMonthOffset monthlyTarget annualFee benefits { id } tiers { minSpending }";

        let parent = gql(
            &schema,
            &format!(
                r#"mutation {{ createPaymentMethod(input: {{name: "본인", kind: CREDIT_CARD, issuer: "신한", paymentDay: 25, closingDay: 11,
                closingMonthOffset: 0, monthlyTarget: 300000, benefits: [{{name: "10%", rate: 0.1, sharesCardLimit: false}}],
                tiers: [{{minSpending: 300000, benefitLimit: 10000}}]}}) {{ {fields} }} }}"#
            ),
        )
        .await["createPaymentMethod"]
            .clone();
        let p = id(&parent["id"]);
        let benefit = id(&parent["benefits"][0]["id"]);

        // 종류·정산 일정·카드사는 본인 카드를 따르고, 실적 목표·혜택·구간은 본인 카드 것을 쓴다. 연회비는 따로
        let family = gql(
            &schema,
            &format!(
                r#"mutation {{ createPaymentMethod(input: {{parentId: {p}, name: "가족", kind: CHECK_CARD, last4: "1234", annualFee: 5000,
                monthlyTarget: 999, benefits: [{{name: "무시", rate: 0.5}}], tiers: [{{minSpending: 1, benefitLimit: 1}}]}}) {{ {fields} }} }}"#
            ),
        )
        .await["createPaymentMethod"]
            .clone();
        let f = id(&family["id"]);
        assert_eq!(family["kind"], "CREDIT_CARD");
        assert_eq!(family["parentId"], p);
        assert_eq!(family["issuer"], "신한");
        assert_eq!((family["paymentDay"].clone(), family["closingDay"].clone(), family["closingMonthOffset"].clone()), (json!(25), json!(11), json!(0)));
        assert_eq!(family["monthlyTarget"], Value::Null);
        assert_eq!(family["annualFee"], 5000);
        assert_eq!(family["benefits"], parent["benefits"]);
        assert_eq!(family["tiers"], json!([{"minSpending": 300000}]));
        assert_eq!(card_benefit::Entity::find().count(&db).await.unwrap(), 1, "가족카드는 혜택을 따로 두지 않는다");

        // 가족카드 거래에 본인 카드 혜택을 고를 수 있다
        let tx = |pm: i64, amount: i64| {
            format!(
                r#"mutation {{ createTransaction(input: {{ledgerId: {home}, date: "2026-11-05", amount: {amount}, paymentMethodId: {pm},
                description: "마트", benefitIds: [{benefit}]}}) {{ id benefitIds }} }}"#
            )
        };
        gql(&schema, &tx(p, 100_000)).await;
        let ft = gql(&schema, &tx(f, 250_000)).await["createTransaction"].clone();
        assert_eq!(ft["benefitIds"], json!([benefit]));

        // 실적·혜택은 합쳐서, 이용금액은 카드별. 전월실적 목록에는 가족카드를 넣지 않는다
        let report = "paymentMethodId spending performance target discountTotal";
        let monthly = gql(&schema, &format!(r#"{{ cardsMonthly(month: "2026-11") {{ {report} }} }}"#)).await;
        assert_eq!(
            monthly["cardsMonthly"],
            json!([{"paymentMethodId": p, "spending": 100000, "performance": 350000, "target": 300000, "discountTotal": 35000}])
        );
        let all = gql(&schema, &format!(r#"{{ cardsMonthly(month: "2026-11", includeFamily: true) {{ {report} }} }}"#)).await;
        assert_eq!(
            all["cardsMonthly"][1],
            json!({"paymentMethodId": f, "spending": 250000, "performance": 350000, "target": 300000, "discountTotal": 35000})
        );

        // 가족카드 대금은 본인 카드 청구서에 합쳐진다: 본인 카드 청구서는 합친 금액, 가족카드 청구서는 그 카드 이용분.
        // 여유자금 카드대금 목록에는 본인 카드만 (두 번 세지 않는다)
        let bill = |pm: i64| {
            format!(r#"{{ paymentMethod(id: {pm}) {{ statements(from: "2026-11", to: "2026-11") {{ confirmedAmount }} }} statementLines(paymentMethodId: {pm}, month: "2026-11") {{ amount transaction {{ paymentMethodId }} }} }}"#)
        };
        let parent_bill = gql(&schema, &bill(p)).await;
        assert_eq!(parent_bill["paymentMethod"]["statements"], json!([{"confirmedAmount": 350000}]));
        assert_eq!(
            parent_bill["statementLines"],
            json!([{"amount": 100000, "transaction": {"paymentMethodId": p}}, {"amount": 250000, "transaction": {"paymentMethodId": f}}])
        );
        let family_bill = gql(&schema, &bill(f)).await;
        assert_eq!(family_bill["paymentMethod"]["statements"], json!([{"confirmedAmount": 250000}]));
        let funds = gql(&schema, "{ availableFunds { cards { paymentMethodId statements { month confirmedAmount } } } }").await;
        let due: Vec<&Value> = funds["availableFunds"]["cards"].as_array().unwrap().iter().filter(|c| [p, f].contains(&id(&c["paymentMethodId"]))).collect();
        assert_eq!(due.len(), 1);
        assert_eq!(id(&due[0]["paymentMethodId"]), p);

        // 가족카드 아래 가족카드, 가족카드가 있는 카드를 가족카드로·카드 아닌 종류로 바꾸기는 막는다
        let msg = gql_error(&schema, &format!(r#"mutation {{ createPaymentMethod(input: {{parentId: {f}, name: "손자", kind: CREDIT_CARD}}) {{ id }} }}"#)).await;
        assert!(msg.contains("가족카드라"), "{msg}");
        let other = id(&gql(&schema, r#"mutation { createPaymentMethod(input: {name: "다른", kind: CREDIT_CARD}) { id } }"#).await["createPaymentMethod"]["id"]);
        let msg = gql_error(&schema, &format!(r#"mutation {{ updatePaymentMethod(id: {p}, input: {{parentId: {other}, name: "본인", kind: CREDIT_CARD}}) {{ id }} }}"#)).await;
        assert!(msg.contains("가족카드가 있는 카드"), "{msg}");
        let msg = gql_error(&schema, &format!(r#"mutation {{ updatePaymentMethod(id: {p}, input: {{name: "본인", kind: CASH}}) {{ id }} }}"#)).await;
        assert!(msg.contains("카드가 아닌 종류"), "{msg}");

        // 본인 카드의 정산 일정을 고치면 가족카드도 따라간다 (혜택은 그대로 두려고 목록을 빼고 보낸다)
        gql(
            &schema,
            &format!(
                r#"mutation {{ updatePaymentMethod(id: {p}, input: {{name: "본인", kind: CREDIT_CARD, issuer: "신한", paymentDay: 14, closingDay: 31,
                closingMonthOffset: -1, monthlyTarget: 300000}}) {{ id }} }}"#
            ),
        )
        .await;
        let read = |pm: i64| format!("{{ paymentMethod(id: {pm}) {{ archived paymentDay closingDay closingMonthOffset benefits {{ id }} }} }}");
        let synced = gql(&schema, &read(f)).await["paymentMethod"].clone();
        assert_eq!((synced["paymentDay"].clone(), synced["closingDay"].clone(), synced["closingMonthOffset"].clone()), (json!(14), json!(31), json!(-1)));
        assert_eq!(synced["benefits"], json!([{"id": benefit}]));

        // 본인 카드를 지우면(거래가 있어 보관) 가족카드도 보관된다. 가족카드만 먼저 다시 쓸 수는 없다
        let del = gql(&schema, &format!("mutation {{ deletePaymentMethod(id: {p}) {{ deleted archived }} }}")).await;
        assert_eq!(del["deletePaymentMethod"], json!({"deleted": false, "archived": true}));
        assert_eq!(gql(&schema, &read(f)).await["paymentMethod"]["archived"], true);
        let msg = gql_error(&schema, &format!("mutation {{ restorePaymentMethod(id: {f}) {{ id }} }}")).await;
        assert!(msg.contains("본인 카드"), "{msg}");
        gql(&schema, &format!("mutation {{ restorePaymentMethod(id: {p}) {{ id }} }}")).await;
        gql(&schema, &format!("mutation {{ restorePaymentMethod(id: {f}) {{ id }} }}")).await;

        // 가족카드에서 풀면 본인 카드 혜택 지정은 지운다
        gql(&schema, &format!(r#"mutation {{ updatePaymentMethod(id: {f}, input: {{name: "가족", kind: CREDIT_CARD}}) {{ id }} }}"#)).await;
        let t = id(&ft["id"]);
        let after = gql(&schema, &format!("{{ transaction(id: {t}) {{ benefitIds }} }}")).await;
        assert_eq!(after["transaction"]["benefitIds"], json!([]));

        // 거래가 없어도 가족카드가 있는 본인 카드는 지우지 않고 보관한다
        let lonely = id(&gql(&schema, r#"mutation { createPaymentMethod(input: {name: "새 본인", kind: CREDIT_CARD}) { id } }"#).await["createPaymentMethod"]["id"]);
        let kid = id(&gql(&schema, &format!(r#"mutation {{ createPaymentMethod(input: {{parentId: {lonely}, name: "새 가족", kind: CREDIT_CARD}}) {{ id }} }}"#)).await["createPaymentMethod"]["id"]);
        let del = gql(&schema, &format!("mutation {{ deletePaymentMethod(id: {lonely}) {{ deleted archived }} }}")).await;
        assert_eq!(del["deletePaymentMethod"], json!({"deleted": false, "archived": true}));
        assert_eq!(gql(&schema, &read(kid)).await["paymentMethod"]["archived"], true);
        let msg = gql_error(&schema, &format!(r#"mutation {{ createPaymentMethod(input: {{parentId: {lonely}, name: "또 가족", kind: CREDIT_CARD}}) {{ id }} }}"#)).await;
        assert!(msg.contains("보관된 카드"), "{msg}");
    }

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
