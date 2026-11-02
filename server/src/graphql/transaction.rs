//! 거래(결제 내역) API.

use async_graphql::{ComplexObject, Context, Enum, InputObject, MaybeUndefined, Object, Result, SimpleObject};
use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Select, Set, TransactionTrait,
    sea_query::{Expr, Func},
};

use super::{config, db, lookup, trim_opt, user_error};
use crate::domain::benefits::stacking_conflict;
use crate::domain::billing::BillingCycle;
use crate::domain::dates::YearMonth;
use crate::domain::ingest::{self, IngestInput, IngestOutcome, Suggestion};
use crate::domain::sms::{self, ParsedMessage};
use crate::entity::{
    card_benefit, category,
    enums::{CategoryKind, TxSource, TxStatus},
    ledger, payment_method, transaction, transaction_benefit,
};

#[ComplexObject]
impl transaction::Model {
    async fn ledger(&self, ctx: &Context<'_>) -> Result<Option<ledger::Model>> {
        Ok(lookup(ctx).ledgers().await?.get(&self.ledger_id).cloned())
    }

    async fn category(&self, ctx: &Context<'_>) -> Result<Option<category::Model>> {
        let Some(id) = self.category_id else { return Ok(None) };
        Ok(lookup(ctx).categories().await?.get(&id).cloned())
    }

    async fn payment_method(&self, ctx: &Context<'_>) -> Result<Option<payment_method::Model>> {
        let Some(id) = self.payment_method_id else { return Ok(None) };
        Ok(lookup(ctx).payment_methods().await?.get(&id).cloned())
    }

    /// 적용한 카드 혜택 항목 id
    async fn benefit_ids(&self, ctx: &Context<'_>) -> Result<Vec<i32>> {
        Ok(lookup(ctx).tx_benefits().await?.get(&self.id).cloned().unwrap_or_default())
    }

    async fn benefits(&self, ctx: &Context<'_>) -> Result<Vec<card_benefit::Model>> {
        let lookup = lookup(ctx);
        let ids = lookup.tx_benefits().await?.get(&self.id).cloned().unwrap_or_default();
        let all = lookup.benefits().await?;
        Ok(ids.iter().filter_map(|id| all.get(id).cloned()).collect())
    }

    /// 수입 카테고리 거래인지
    async fn is_income(&self, ctx: &Context<'_>) -> Result<bool> {
        let Some(id) = self.category_id else { return Ok(false) };
        Ok(lookup(ctx).categories().await?.get(&id).is_some_and(|c| c.kind == CategoryKind::Income))
    }

    /// 신용카드 청구 결제월 (정산 일정이 있는 카드만)
    async fn statement_month(&self, ctx: &Context<'_>) -> Result<Option<YearMonth>> {
        let Some(id) = self.payment_method_id else { return Ok(None) };
        let pms = lookup(ctx).payment_methods().await?;
        Ok(pms.get(&id).and_then(BillingCycle::from_model).map(|c| c.statement_for(self.date)))
    }

    /// 실제로 돈이 빠져나가는 날 (신용카드는 결제일, 그 외는 거래일)
    async fn payment_date(&self, ctx: &Context<'_>) -> Result<NaiveDate> {
        let Some(id) = self.payment_method_id else { return Ok(self.date) };
        let pms = lookup(ctx).payment_methods().await?;
        Ok(match pms.get(&id).and_then(BillingCycle::from_model) {
            Some(c) => c.payment_date(c.statement_for(self.date)),
            None => self.date,
        })
    }
}

#[derive(InputObject, Default)]
pub struct TransactionFilter {
    /// 이 달의 거래 (from/to보다 우선)
    pub month: Option<YearMonth>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub ledger_id: Option<i32>,
    pub category_id: Option<i32>,
    /// true면 카테고리 없는 거래만
    pub uncategorized: Option<bool>,
    pub payment_method_id: Option<i32>,
    /// 기본: 확정 + 예정
    pub statuses: Option<Vec<TxStatus>>,
    pub member: Option<String>,
    /// 내용/메모 검색
    pub search: Option<String>,
    pub source: Option<TxSource>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq, Default)]
pub enum TransactionSort {
    #[default]
    DateAsc,
    DateDesc,
    AmountDesc,
}

#[derive(SimpleObject)]
pub struct TransactionPage {
    pub items: Vec<transaction::Model>,
    pub total_count: i64,
    /// 필터 결과 전체의 확정 지출 합계 (지출 카테고리·미분류, 환불 차감)
    pub confirmed_total: i64,
    /// 필터 결과 전체의 예정 지출 합계
    pub planned_total: i64,
    /// 필터 결과 전체의 확정 수입 합계 (수입 카테고리)
    pub income_total: i64,
    /// 필터 결과 전체의 예정 수입 합계
    pub planned_income_total: i64,
}

fn apply_filter(mut q: Select<transaction::Entity>, f: &TransactionFilter) -> Select<transaction::Entity> {
    use transaction::Column as C;
    if let Some(m) = f.month {
        q = q.filter(C::Date.gte(m.first_day())).filter(C::Date.lte(m.last_day()));
    } else {
        if let Some(from) = f.from {
            q = q.filter(C::Date.gte(from));
        }
        if let Some(to) = f.to {
            q = q.filter(C::Date.lte(to));
        }
    }
    if let Some(id) = f.ledger_id {
        q = q.filter(C::LedgerId.eq(id));
    }
    if f.uncategorized == Some(true) {
        q = q.filter(C::CategoryId.is_null());
    } else if let Some(id) = f.category_id {
        q = q.filter(C::CategoryId.eq(id));
    }
    if let Some(id) = f.payment_method_id {
        q = q.filter(C::PaymentMethodId.eq(id));
    }
    let statuses = f.statuses.clone().unwrap_or_else(|| vec![TxStatus::Confirmed, TxStatus::Planned]);
    q = q.filter(C::Status.is_in(statuses));
    if let Some(m) = f.member.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        q = q.filter(C::Member.eq(m));
    }
    if let Some(s) = f.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        q = q.filter(
            Condition::any()
                .add(Expr::cust_with_values("instr(description, ?) > 0", [s]))
                .add(Expr::cust_with_values("instr(coalesce(memo, ''), ?) > 0", [s])),
        );
    }
    if let Some(src) = f.source {
        q = q.filter(C::Source.eq(src));
    }
    q
}

#[derive(Default)]
pub struct TransactionQuery;

#[Object]
impl TransactionQuery {
    /// 거래 목록. 기본 정렬은 날짜순(같은 날은 입력순).
    async fn transactions(
        &self,
        ctx: &Context<'_>,
        #[graphql(default)] filter: TransactionFilter,
        #[graphql(default)] sort: TransactionSort,
        #[graphql(default = 0)] offset: u64,
        #[graphql(default = 1000)] limit: u64,
    ) -> Result<TransactionPage> {
        let db = db(ctx);
        let base = apply_filter(transaction::Entity::find(), &filter);
        let total_count = base.clone().count(db).await? as i64;
        // 수입 카테고리는 지출 합계와 섞지 않는다
        let sums: Vec<(String, Option<i32>, Option<i64>)> = base
            .clone()
            .select_only()
            .column(transaction::Column::Status)
            .column(transaction::Column::CategoryId)
            .column_as(Expr::from(Func::sum(Expr::col(transaction::Column::Amount))), "total")
            .group_by(transaction::Column::Status)
            .group_by(transaction::Column::CategoryId)
            .into_tuple()
            .all(db)
            .await?;
        let categories = lookup(ctx).categories().await?;
        let is_income = |cid: &Option<i32>| cid.and_then(|id| categories.get(&id)).is_some_and(|c| c.kind == CategoryKind::Income);
        let total_of = |status: &str, income: bool| {
            sums.iter()
                .filter(|(st, cid, _)| st == status && is_income(cid) == income)
                .filter_map(|(_, _, v)| *v)
                .sum::<i64>()
        };
        let ordered = match sort {
            TransactionSort::DateAsc => base.order_by_asc(transaction::Column::Date).order_by_asc(transaction::Column::Id),
            TransactionSort::DateDesc => base.order_by_desc(transaction::Column::Date).order_by_desc(transaction::Column::Id),
            TransactionSort::AmountDesc => base.order_by_desc(transaction::Column::Amount).order_by_desc(transaction::Column::Date),
        };
        let items = ordered.offset(offset).limit(limit.min(5000)).all(db).await?;
        Ok(TransactionPage {
            items,
            total_count,
            confirmed_total: total_of("CONFIRMED", false),
            planned_total: total_of("PLANNED", false),
            income_total: total_of("CONFIRMED", true),
            planned_income_total: total_of("PLANNED", true),
        })
    }

    async fn transaction(&self, ctx: &Context<'_>, id: i32) -> Result<Option<transaction::Model>> {
        Ok(transaction::Entity::find_by_id(id).one(db(ctx)).await?)
    }

    /// 입력 중인 내용과 비슷한 과거 가맹점과 그때의 분류 (자주 쓴 순)
    async fn description_suggestions(
        &self,
        ctx: &Context<'_>,
        query: String,
        ledger_id: Option<i32>,
        #[graphql(default = 8)] limit: u64,
    ) -> Result<Vec<Suggestion>> {
        let db = db(ctx);
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let mut select = transaction::Entity::find()
            .select_only()
            .column(transaction::Column::Description)
            .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
            .filter(Expr::cust_with_values("instr(description, ?) > 0", [q]));
        if let Some(id) = ledger_id {
            select = select.filter(transaction::Column::LedgerId.eq(id));
        }
        let descriptions: Vec<String> = select
            .group_by(transaction::Column::Description)
            .order_by_desc(Expr::cust("count(*)"))
            .limit(limit.min(30))
            .into_tuple()
            .all(db)
            .await?;
        let mut out = Vec::new();
        for d in descriptions {
            if let Some(mut s) = ingest::suggest(db, &d, None).await? {
                s.description = d;
                out.push(s);
            }
        }
        Ok(out)
    }

    /// 내용(가맹점)으로 장부/카테고리 추천
    async fn suggest_category(
        &self,
        ctx: &Context<'_>,
        description: String,
        payment_method_id: Option<i32>,
    ) -> Result<Option<Suggestion>> {
        Ok(ingest::suggest(db(ctx), &description, payment_method_id).await?)
    }

    /// 카드 승인 문자 해석 미리보기 (저장하지 않음)
    async fn parse_message(&self, ctx: &Context<'_>, text: String) -> ParsedMessage {
        sms::parse(&text, config(ctx).today())
    }
}

#[derive(InputObject, Clone)]
pub struct TransactionInput {
    /// 비우면 카테고리의 장부, 그것도 없으면 기본 장부
    pub ledger_id: Option<i32>,
    pub date: NaiveDate,
    pub amount: i64,
    pub category_id: Option<i32>,
    pub payment_method_id: Option<i32>,
    pub description: String,
    pub memo: Option<String>,
    pub member: Option<String>,
    #[graphql(default_with = "TxStatus::Confirmed")]
    pub status: TxStatus,
    #[graphql(default = 1)]
    pub installment_months: i32,
    #[graphql(default = false)]
    pub exclude_from_performance: bool,
    pub benefit_ids: Option<Vec<i32>>,
}

#[derive(InputObject, Default)]
pub struct TransactionPatch {
    pub ledger_id: Option<i32>,
    pub date: Option<NaiveDate>,
    pub amount: Option<i64>,
    pub category_id: MaybeUndefined<i32>,
    pub payment_method_id: MaybeUndefined<i32>,
    pub description: Option<String>,
    pub memo: MaybeUndefined<String>,
    pub member: MaybeUndefined<String>,
    pub status: Option<TxStatus>,
    pub installment_months: Option<i32>,
    pub exclude_from_performance: Option<bool>,
    /// 주면 혜택 항목을 이 목록으로 바꾼다
    pub benefit_ids: Option<Vec<i32>>,
}

async fn default_ledger_id<C: ConnectionTrait>(db: &C) -> Result<i32> {
    let ledgers = ledger::Entity::find().all(db).await?;
    ledgers
        .iter()
        .find(|l| l.is_default)
        .or_else(|| ledgers.iter().min_by_key(|l| (l.sort_order, l.id)))
        .map(|l| l.id)
        .ok_or_else(|| user_error("장부를 먼저 만드세요"))
}

/// 장부·카테고리·결제수단 조합을 확인하고 장부 id를 정한다.
async fn resolve_ledger<C: ConnectionTrait>(db: &C, ledger_id: Option<i32>, category_id: Option<i32>) -> Result<i32> {
    let category = match category_id {
        Some(id) => Some(category::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?),
        None => None,
    };
    match (ledger_id, category) {
        (Some(l), Some(c)) if c.ledger_id != l => Err(user_error(format!("'{}' 카테고리는 다른 장부의 카테고리입니다", c.name))),
        (Some(l), _) => {
            ledger::Entity::find_by_id(l).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
            Ok(l)
        }
        (None, Some(c)) => Ok(c.ledger_id),
        (None, None) => default_ledger_id(db).await,
    }
}

async fn check_payment_method<C: ConnectionTrait>(db: &C, id: Option<i32>) -> Result<()> {
    if let Some(id) = id {
        payment_method::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("결제수단이 없습니다"))?;
    }
    Ok(())
}

/// 거래의 혜택 항목을 바꾼다. 다른 카드의 혜택 id는 버린다.
/// 중복 적용 혜택이 아닌 것을 둘 이상 고르면 거절한다.
async fn set_benefits<C: ConnectionTrait>(db: &C, tx_id: i32, payment_method_id: Option<i32>, ids: &[i32]) -> Result<()> {
    transaction_benefit::Entity::delete_many()
        .filter(transaction_benefit::Column::TransactionId.eq(tx_id))
        .exec(db)
        .await?;
    let Some(pid) = payment_method_id else { return Ok(()) };
    if ids.is_empty() {
        return Ok(());
    }
    let valid = card_benefit::Entity::find()
        .filter(card_benefit::Column::PaymentMethodId.eq(pid))
        .filter(card_benefit::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await?;
    if let Some(names) = stacking_conflict(&valid) {
        let names: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
        return Err(user_error(format!(
            "{} 혜택은 함께 받을 수 없습니다. 다른 혜택과 함께 받는 혜택이면 카드 설정에서 '중복 적용'으로 표시하세요",
            names.join(", ")
        )));
    }
    for b in valid {
        transaction_benefit::ActiveModel { transaction_id: Set(tx_id), card_benefit_id: Set(b.id) }.insert(db).await?;
    }
    Ok(())
}

async fn create_one<C: ConnectionTrait>(db: &C, input: TransactionInput) -> Result<transaction::Model> {
    let description = input.description.trim().to_string();
    if description.is_empty() {
        return Err(user_error("내용을 입력하세요"));
    }
    if input.status == TxStatus::Skipped {
        return Err(user_error("새 거래는 확정 또는 예정 상태여야 합니다"));
    }
    let ledger_id = resolve_ledger(db, input.ledger_id, input.category_id).await?;
    check_payment_method(db, input.payment_method_id).await?;
    let now = Utc::now();
    let saved = transaction::ActiveModel {
        ledger_id: Set(ledger_id),
        date: Set(input.date),
        amount: Set(input.amount),
        category_id: Set(input.category_id),
        payment_method_id: Set(input.payment_method_id),
        description: Set(description.chars().take(200).collect()),
        memo: Set(trim_opt(input.memo)),
        member: Set(trim_opt(input.member)),
        status: Set(input.status),
        installment_months: Set(input.installment_months.clamp(1, 60)),
        exclude_from_performance: Set(input.exclude_from_performance),
        recurring_rule_id: Set(None),
        recurring_month: Set(None),
        source: Set(TxSource::Manual),
        external_id: Set(None),
        raw_text: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    if let Some(ids) = &input.benefit_ids {
        set_benefits(db, saved.id, saved.payment_method_id, ids).await?;
    }
    Ok(saved)
}

#[derive(Default)]
pub struct TransactionMutation;

#[Object]
impl TransactionMutation {
    /// 결제 내역 입력
    async fn create_transaction(&self, ctx: &Context<'_>, input: TransactionInput) -> Result<transaction::Model> {
        let db = db(ctx);
        let txn = db.begin().await?;
        let saved = create_one(&txn, input).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 여러 건을 한 번에 입력 (하나라도 실패하면 모두 취소)
    async fn create_transactions(&self, ctx: &Context<'_>, inputs: Vec<TransactionInput>) -> Result<Vec<transaction::Model>> {
        if inputs.len() > 500 {
            return Err(user_error("한 번에 500건까지 입력할 수 있습니다"));
        }
        let db = db(ctx);
        let txn = db.begin().await?;
        let mut out = Vec::with_capacity(inputs.len());
        for (i, input) in inputs.into_iter().enumerate() {
            out.push(create_one(&txn, input).await.map_err(|e| user_error(format!("{}번째 행: {}", i + 1, e.message)))?);
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(out)
    }

    /// 바꿀 필드만 보낸다 (null을 보내면 비운다)
    async fn update_transaction(&self, ctx: &Context<'_>, id: i32, input: TransactionPatch) -> Result<transaction::Model> {
        let db = db(ctx);
        let found = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("거래가 없습니다"))?;
        let txn = db.begin().await?;
        let mut category_id = found.category_id;
        let mut ledger_id = found.ledger_id;
        // 장부/카테고리를 바꾸는 요청일 때만 둘의 짝을 맞춘다 (메모만 고칠 때 장부가 바뀌지 않게)
        let category_changed = !matches!(input.category_id, MaybeUndefined::Undefined);
        if input.ledger_id.is_some() || category_changed {
            match input.category_id {
                MaybeUndefined::Value(v) => category_id = Some(v),
                MaybeUndefined::Null => category_id = None,
                MaybeUndefined::Undefined => {}
            }
            ledger_id = input.ledger_id.unwrap_or(found.ledger_id);
            if let Some(cid) = category_id {
                let cat = category::Entity::find_by_id(cid).one(&txn).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?;
                if cat.ledger_id != ledger_id {
                    if input.ledger_id.is_some() && !category_changed {
                        // 장부만 옮기면 이전 장부의 카테고리는 비운다
                        category_id = None;
                    } else if input.ledger_id.is_some() {
                        return Err(user_error(format!("'{}' 카테고리는 다른 장부의 카테고리입니다", cat.name)));
                    } else {
                        // 카테고리만 바꾸면 그 카테고리의 장부로 옮긴다
                        ledger_id = cat.ledger_id;
                    }
                }
            }
            if input.ledger_id.is_some() {
                ledger::Entity::find_by_id(ledger_id).one(&txn).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
            }
        }
        let mut payment_method_id = found.payment_method_id;
        match input.payment_method_id {
            MaybeUndefined::Value(v) => payment_method_id = Some(v),
            MaybeUndefined::Null => payment_method_id = None,
            MaybeUndefined::Undefined => {}
        }
        check_payment_method(&txn, payment_method_id).await?;
        let pm_changed = payment_method_id != found.payment_method_id;

        let mut active: transaction::ActiveModel = found.clone().into();
        active.ledger_id = Set(ledger_id);
        active.category_id = Set(category_id);
        active.payment_method_id = Set(payment_method_id);
        if let Some(d) = input.date {
            active.date = Set(d);
        }
        if let Some(a) = input.amount {
            active.amount = Set(a);
        }
        if let Some(d) = input.description.map(|d| d.trim().to_string()) {
            if d.is_empty() {
                return Err(user_error("내용을 비울 수 없습니다"));
            }
            active.description = Set(d.chars().take(200).collect());
        }
        match input.memo {
            MaybeUndefined::Value(v) => active.memo = Set(trim_opt(Some(v))),
            MaybeUndefined::Null => active.memo = Set(None),
            MaybeUndefined::Undefined => {}
        }
        match input.member {
            MaybeUndefined::Value(v) => active.member = Set(trim_opt(Some(v))),
            MaybeUndefined::Null => active.member = Set(None),
            MaybeUndefined::Undefined => {}
        }
        if let Some(s) = input.status {
            active.status = Set(s);
        }
        if let Some(n) = input.installment_months {
            active.installment_months = Set(n.clamp(1, 60));
        }
        if let Some(v) = input.exclude_from_performance {
            active.exclude_from_performance = Set(v);
        }
        active.updated_at = Set(Utc::now());
        let saved = active.update(&txn).await?;
        if let Some(ids) = &input.benefit_ids {
            set_benefits(&txn, saved.id, saved.payment_method_id, ids).await?;
        } else if pm_changed {
            let keep: Vec<i32> = transaction_benefit::Entity::find()
                .filter(transaction_benefit::Column::TransactionId.eq(saved.id))
                .all(&txn)
                .await?
                .into_iter()
                .map(|b| b.card_benefit_id)
                .collect();
            set_benefits(&txn, saved.id, saved.payment_method_id, &keep).await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 삭제. 반복 규칙이 이번 달 이후에 만든 항목은 다시 생기지 않도록 '건너뜀'으로 바꾼다.
    async fn delete_transaction(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let db = db(ctx);
        let found = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("거래가 없습니다"))?;
        // 반복 규칙이 이번 달 이후에 만든 항목은 지우면 다시 생기므로 '건너뜀'으로 자리를 남긴다.
        let this_month = YearMonth::of(config(ctx).today()).to_string();
        let keeps_slot = found.recurring_rule_id.is_some()
            && found.recurring_month.as_deref().is_some_and(|m| m >= this_month.as_str());
        if keeps_slot {
            if found.status != TxStatus::Skipped {
                let mut active: transaction::ActiveModel = found.into();
                active.status = Set(TxStatus::Skipped);
                active.updated_at = Set(Utc::now());
                active.update(db).await?;
            }
        } else {
            transaction::Entity::delete_by_id(id).exec(db).await?;
        }
        lookup(ctx).invalidate();
        Ok(true)
    }

    /// 예정 거래를 확정 (날짜/금액을 실제 값으로 바꿀 수 있다)
    async fn confirm_transaction(
        &self,
        ctx: &Context<'_>,
        id: i32,
        date: Option<NaiveDate>,
        amount: Option<i64>,
    ) -> Result<transaction::Model> {
        let db = db(ctx);
        let found = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("거래가 없습니다"))?;
        let mut active: transaction::ActiveModel = found.into();
        active.status = Set(TxStatus::Confirmed);
        if let Some(d) = date {
            active.date = Set(d);
        }
        if let Some(a) = amount {
            active.amount = Set(a);
        }
        active.updated_at = Set(Utc::now());
        Ok(active.update(db).await?)
    }

    /// 예정 거래를 이번에는 건너뜀
    async fn skip_transaction(&self, ctx: &Context<'_>, id: i32) -> Result<transaction::Model> {
        set_status(ctx, id, TxStatus::Skipped).await
    }

    /// 건너뛴 거래를 다시 예정으로
    async fn restore_transaction(&self, ctx: &Context<'_>, id: i32) -> Result<transaction::Model> {
        set_status(ctx, id, TxStatus::Planned).await
    }

    /// 자동 입력 (REST `POST /api/ingest`와 같은 처리)
    async fn ingest_transaction(&self, ctx: &Context<'_>, input: IngestInput) -> Result<IngestOutcome> {
        let db = db(ctx);
        let txn = db.begin().await?;
        let out = ingest::ingest(&txn, input, config(ctx).today()).await.map_err(|e| match e {
            ingest::IngestError::Invalid(msg) => user_error(msg),
            ingest::IngestError::Db(e) => e.into(),
        })?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(out)
    }
}

async fn set_status(ctx: &Context<'_>, id: i32, status: TxStatus) -> Result<transaction::Model> {
    let db = db(ctx);
    let found = transaction::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("거래가 없습니다"))?;
    let mut active: transaction::ActiveModel = found.into();
    active.status = Set(status);
    active.updated_at = Set(Utc::now());
    Ok(active.update(db).await?)
}
