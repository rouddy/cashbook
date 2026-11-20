//! 거래(결제 내역) API.

use async_graphql::{ComplexObject, Context, Enum, InputObject, MaybeUndefined, Object, Result, SimpleObject};
use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Select, Set, TransactionTrait,
    sea_query::Expr,
};

use super::{config, db, lookup, trim_opt, user_error};
use crate::db::sql;
use crate::domain::benefits::stacking_conflict;
use crate::domain::billing::BillingCycle;
use crate::domain::dates::YearMonth;
use crate::domain::ingest::{self, IngestInput, IngestOutcome, Suggestion};
use crate::domain::sms::{self, ParsedMessage};
use crate::domain::split::{self, CategoryChoice, SplitError};
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

    /// 적용한 카드 혜택 항목 id (분할해 옮긴 거래는 원래 거래의 혜택)
    async fn benefit_ids(&self, ctx: &Context<'_>) -> Result<Vec<i32>> {
        Ok(lookup(ctx).tx_benefits().await?.get(&benefit_owner(self)).cloned().unwrap_or_default())
    }

    async fn benefits(&self, ctx: &Context<'_>) -> Result<Vec<card_benefit::Model>> {
        let lookup = lookup(ctx);
        let ids = lookup.tx_benefits().await?.get(&benefit_owner(self)).cloned().unwrap_or_default();
        let all = lookup.benefits().await?;
        Ok(ids.iter().filter_map(|id| all.get(id).cloned()).collect())
    }

    /// 같은 결제를 나눈 다른 장부의 거래들 (분할하지 않았으면 빈 목록). 이 거래와 금액을 합치면 결제 금액이다.
    async fn split_with(&self, ctx: &Context<'_>) -> Result<Vec<transaction::Model>> {
        let groups = lookup(ctx).split_groups().await?;
        let group = groups.get(&self.split_parent_id.unwrap_or(self.id));
        Ok(group.map(|g| g.iter().filter(|t| t.id != self.id).cloned().collect()).unwrap_or_default())
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

/// 카드 혜택 지정을 두는 거래: 분할해 옮긴 거래(자식)는 원래 거래(부모)
fn benefit_owner(tx: &transaction::Model) -> i32 {
    tx.split_parent_id.unwrap_or(tx.id)
}

fn split_error(e: SplitError) -> async_graphql::Error {
    match e {
        SplitError::Invalid(msg) => user_error(msg),
        SplitError::Db(e) => e.into(),
    }
}

const SPLIT_LOCKED: &str = "분할된 거래입니다. 분할을 되돌린 뒤 바꾸세요";

#[derive(InputObject, Default)]
pub struct TransactionFilter {
    /// 이 달의 거래 (from/to보다 우선)
    pub month: Option<YearMonth>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// 이 장부의 거래와 다른 장부에서 이 장부로 받은 이체
    pub ledger_id: Option<i32>,
    pub category_id: Option<i32>,
    /// true면 카테고리 없는 거래만 (장부 이체 제외)
    pub uncategorized: Option<bool>,
    /// true면 장부 이체만, false면 장부 이체 빼고
    pub transfers: Option<bool>,
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
    /// 장부를 정한 필터에서 그 장부가 받은 이체 합계 (확정). 지출·수입 합계에는 이체를 넣지 않는다
    pub transfer_in_total: i64,
    pub planned_transfer_in_total: i64,
    /// 장부를 정한 필터에서 그 장부가 보낸 이체 합계 (확정)
    pub transfer_out_total: i64,
    pub planned_transfer_out_total: i64,
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
        q = q.filter(Condition::any().add(C::LedgerId.eq(id)).add(C::TransferLedgerId.eq(id)));
    }
    if f.uncategorized == Some(true) {
        q = q.filter(C::CategoryId.is_null()).filter(C::TransferLedgerId.is_null());
    } else if let Some(id) = f.category_id {
        q = q.filter(C::CategoryId.eq(id));
    }
    match f.transfers {
        Some(true) => q = q.filter(C::TransferLedgerId.is_not_null()),
        Some(false) => q = q.filter(C::TransferLedgerId.is_null()),
        None => {}
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
        // 수입 카테고리는 지출 합계와 섞지 않고, 장부 이체는 지출·수입 어느 쪽에도 넣지 않는다
        let backend = db.get_database_backend();
        let sums: Vec<(String, Option<i32>, Option<i64>)> = base
            .clone()
            .filter(transaction::Column::TransferLedgerId.is_null())
            .select_only()
            .column(transaction::Column::Status)
            .column(transaction::Column::CategoryId)
            .column_as(sql::sum_i64(backend, Expr::col(transaction::Column::Amount)), "total")
            .group_by(transaction::Column::Status)
            .group_by(transaction::Column::CategoryId)
            .into_tuple()
            .all(db)
            .await?;
        // (상태, 보낸 장부, 받은 장부, 합계)
        let transfer_sums: Vec<(String, i32, Option<i32>, Option<i64>)> = match filter.ledger_id {
            Some(_) => {
                base.clone()
                    .filter(transaction::Column::TransferLedgerId.is_not_null())
                    .select_only()
                    .column(transaction::Column::Status)
                    .column(transaction::Column::LedgerId)
                    .column(transaction::Column::TransferLedgerId)
                    .column_as(sql::sum_i64(backend, Expr::col(transaction::Column::Amount)), "total")
                    .group_by(transaction::Column::Status)
                    .group_by(transaction::Column::LedgerId)
                    .group_by(transaction::Column::TransferLedgerId)
                    .into_tuple()
                    .all(db)
                    .await?
            }
            None => vec![],
        };
        let transfer_of = |status: &str, incoming: bool| {
            transfer_sums
                .iter()
                .filter(|(st, from, to, _)| {
                    st == status && if incoming { *to == filter.ledger_id } else { Some(*from) == filter.ledger_id }
                })
                .filter_map(|(_, _, _, v)| *v)
                .sum::<i64>()
        };
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
            transfer_in_total: transfer_of("CONFIRMED", true),
            planned_transfer_in_total: transfer_of("PLANNED", true),
            transfer_out_total: transfer_of("CONFIRMED", false),
            planned_transfer_out_total: transfer_of("PLANNED", false),
        })
    }

    async fn transaction(&self, ctx: &Context<'_>, id: i32) -> Result<Option<transaction::Model>> {
        Ok(transaction::Entity::find_by_id(id).one(db(ctx)).await?)
    }

    /// 입력 중인 내용과 비슷한 과거 가맹점과 그때의 분류 (자주 쓴 순).
    /// `since`가 있으면 그날 이후 거래만 본다. `byCategory`면 같은 내용이라도 장부·항목이 다르면 따로 돌려주고,
    /// 아니면 내용마다 가장 많이 쓴 분류 하나만 돌려준다.
    async fn description_suggestions(
        &self,
        ctx: &Context<'_>,
        query: String,
        ledger_id: Option<i32>,
        #[graphql(default = 8)] limit: u64,
        since: Option<NaiveDate>,
        #[graphql(default = false)] by_category: bool,
    ) -> Result<Vec<Suggestion>> {
        let db = db(ctx);
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let limit = limit.min(30);
        if by_category {
            return Ok(ingest::suggestions_by_category(db, q, ledger_id, since, limit).await?);
        }
        let mut select = transaction::Entity::find()
            .select_only()
            .column(transaction::Column::Description)
            .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
            .filter(Expr::cust_with_values("instr(description, ?) > 0", [q]));
        if let Some(id) = ledger_id {
            select = select.filter(transaction::Column::LedgerId.eq(id));
        }
        if let Some(d) = since {
            select = select.filter(transaction::Column::Date.gte(d));
        }
        let descriptions: Vec<String> = select
            .group_by(transaction::Column::Description)
            // 횟수가 같으면 최근에 쓴 것, 그다음 이름순 (DB와 관계없이 같은 결과)
            .order_by_desc(Expr::cust("count(*)"))
            .order_by_desc(Expr::cust("max(date)"))
            .order_by_asc(transaction::Column::Description)
            .limit(limit)
            .into_tuple()
            .all(db)
            .await?;
        let mut out = Vec::new();
        for d in descriptions {
            if let Some(mut s) = ingest::suggest_since(db, &d, None, since).await? {
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
    /// 장부 이체면 받는 장부 (항목·결제수단은 비운다)
    pub transfer_ledger_id: Option<i32>,
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
    /// 장부 이체로 바꾸면(받는 장부) 항목·결제수단·혜택을 비운다. null이면 이체를 푼다.
    /// 이체에 `categoryId`만 주면 이체를 풀고 그 항목의 거래가 된다.
    pub transfer_ledger_id: MaybeUndefined<i32>,
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

/// 장부 이체의 받는 장부 확인
async fn check_transfer_target<C: ConnectionTrait>(db: &C, from: i32, to: i32) -> Result<()> {
    if from == to {
        return Err(user_error("받는 장부는 보내는 장부와 달라야 합니다"));
    }
    ledger::Entity::find_by_id(to).one(db).await?.ok_or_else(|| user_error("받는 장부가 없습니다"))?;
    Ok(())
}

const TRANSFER_FIELDS: &str = "장부 이체에는 항목·결제수단을 지정하지 않습니다";

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

#[derive(SimpleObject, Debug, PartialEq, Eq)]
pub struct MoveTransactionsResult {
    /// 장부를 옮긴 거래 수 (이미 그 장부에 있던 거래는 세지 않는다)
    pub moved: i32,
    /// 옮길 장부에 같은 항목이 없어 미분류가 된 거래 수
    pub uncategorized: i32,
}

/// 여러 거래를 다른 장부로 옮긴다. 카테고리는 장부별이라, 옮길 장부에 이름·종류가 같은
/// 사용 중인 카테고리가 있으면 그것으로 바꾸고 없으면 비운다. 혜택·결제수단은 그대로 둔다.
async fn move_to_ledger<C: ConnectionTrait>(db: &C, ids: &[i32], ledger_id: i32) -> Result<MoveTransactionsResult> {
    if ids.len() > 5000 {
        return Err(user_error("한 번에 5000건까지 옮길 수 있습니다"));
    }
    ledger::Entity::find_by_id(ledger_id).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let found = transaction::Entity::find().filter(transaction::Column::Id.is_in(ids.clone())).all(db).await?;
    if found.len() != ids.len() {
        return Err(user_error("없는 거래가 있습니다. 목록을 새로 고친 뒤 다시 고르세요"));
    }
    if split::any_split(db, &found).await? {
        return Err(user_error("분할된 거래는 옮길 수 없습니다. 분할을 되돌린 뒤 옮기세요"));
    }
    // 옮길 장부로 보내는 이체를 옮기면 보내는 장부와 받는 장부가 같아진다
    if let Some(t) = found.iter().find(|t| t.transfer_ledger_id == Some(ledger_id)) {
        return Err(user_error(format!("'{}'은(는) 그 장부로 보내는 이체라 옮길 수 없습니다", t.description)));
    }
    let categories: std::collections::HashMap<i32, category::Model> =
        category::Entity::find().all(db).await?.into_iter().map(|c| (c.id, c)).collect();
    let now = Utc::now();
    let mut out = MoveTransactionsResult { moved: 0, uncategorized: 0 };
    for tx in found.into_iter().filter(|t| t.ledger_id != ledger_id) {
        let category_id = tx.category_id.and_then(|id| categories.get(&id)).and_then(|c| split::same_category_in(&categories, ledger_id, c));
        if tx.category_id.is_some() && category_id.is_none() {
            out.uncategorized += 1;
        }
        let mut active: transaction::ActiveModel = tx.into();
        active.ledger_id = Set(ledger_id);
        active.category_id = Set(category_id);
        active.updated_at = Set(now);
        active.update(db).await?;
        out.moved += 1;
    }
    Ok(out)
}

#[derive(SimpleObject, Debug, PartialEq, Eq)]
pub struct DeleteTransactionsResult {
    /// 지운 결제 수 (분할한 결제는 나눈 쪽까지 한 건)
    pub deleted: i32,
    /// 다시 생기지 않도록 지우는 대신 '건너뜀'으로 남긴 반복 항목 수
    pub skipped: i32,
}

/// 거래들을 지운다. 분할한 결제는 합친 뒤 결제 전체를 지우고, 반복 규칙이 `this_month` 이후에 만든 항목은
/// 지우면 `ensure_planned`가 다시 만들므로 '건너뜀'으로 자리를 남긴다.
async fn delete_payments<C: ConnectionTrait>(db: &C, ids: &[i32], this_month: YearMonth) -> Result<DeleteTransactionsResult> {
    if ids.len() > 5000 {
        return Err(user_error("한 번에 5000건까지 삭제할 수 있습니다"));
    }
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let found = transaction::Entity::find()
        .filter(transaction::Column::Id.is_in(ids.clone()))
        .order_by_asc(transaction::Column::Id)
        .all(db)
        .await?;
    if found.len() != ids.len() {
        return Err(user_error(if ids.len() == 1 { "거래가 없습니다" } else { "없는 거래가 있습니다. 목록을 새로 고친 뒤 다시 고르세요" }));
    }
    let this_month = this_month.to_string();
    let now = Utc::now();
    // 앞에서 묶음째 지운 분할 거래
    let mut done = std::collections::HashSet::new();
    let mut out = DeleteTransactionsResult { deleted: 0, skipped: 0 };
    for tx in found {
        if done.contains(&tx.id) {
            continue;
        }
        let tx = match split::group_of(db, &tx).await? {
            Some(group) => {
                done.extend(group.members().map(|t| t.id));
                split::merge(db, group).await?
            }
            None => tx,
        };
        let keeps_slot =
            tx.recurring_rule_id.is_some() && tx.recurring_month.as_deref().is_some_and(|m| m >= this_month.as_str());
        if keeps_slot {
            if tx.status != TxStatus::Skipped {
                let mut active: transaction::ActiveModel = tx.into();
                active.status = Set(TxStatus::Skipped);
                active.updated_at = Set(now);
                active.update(db).await?;
            }
            out.skipped += 1;
        } else {
            transaction::Entity::delete_by_id(tx.id).exec(db).await?;
            out.deleted += 1;
        }
    }
    Ok(out)
}

async fn create_one<C: ConnectionTrait>(db: &C, input: TransactionInput) -> Result<transaction::Model> {
    let description = input.description.trim().to_string();
    if description.is_empty() {
        return Err(user_error("내용을 입력하세요"));
    }
    if input.status == TxStatus::Skipped {
        return Err(user_error("새 거래는 확정 또는 예정 상태여야 합니다"));
    }
    let transfer = input.transfer_ledger_id;
    if transfer.is_some() && (input.category_id.is_some() || input.payment_method_id.is_some()) {
        return Err(user_error(TRANSFER_FIELDS));
    }
    let ledger_id = resolve_ledger(db, input.ledger_id, input.category_id).await?;
    if let Some(to) = transfer {
        check_transfer_target(db, ledger_id, to).await?;
    }
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
        installment_months: Set(if transfer.is_some() { 1 } else { input.installment_months.clamp(1, 60) }),
        exclude_from_performance: Set(input.exclude_from_performance && transfer.is_none()),
        recurring_rule_id: Set(None),
        recurring_month: Set(None),
        transfer_ledger_id: Set(transfer),
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
        let group = split::group_of(&txn, &found).await?;
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
        if group.is_some() && ledger_id != found.ledger_id {
            return Err(user_error("분할된 거래는 다른 장부로 옮길 수 없습니다. 분할을 되돌린 뒤 옮기세요"));
        }
        let mut payment_method_id = found.payment_method_id;
        match input.payment_method_id {
            MaybeUndefined::Value(v) => payment_method_id = Some(v),
            MaybeUndefined::Null => payment_method_id = None,
            MaybeUndefined::Undefined => {}
        }
        // 장부 이체: 받는 장부를 주면 이체로, 이체에 항목만 주면 이체를 풀고 그 항목의 거래로
        let category_given = matches!(input.category_id, MaybeUndefined::Value(_));
        let transfer_ledger_id = match input.transfer_ledger_id {
            MaybeUndefined::Value(to) => Some(to),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined if category_given => None,
            MaybeUndefined::Undefined => found.transfer_ledger_id,
        };
        if let Some(to) = transfer_ledger_id {
            if category_given || matches!(input.payment_method_id, MaybeUndefined::Value(_)) {
                return Err(user_error(TRANSFER_FIELDS));
            }
            if group.is_some() {
                return Err(user_error(SPLIT_LOCKED));
            }
            check_transfer_target(&txn, ledger_id, to).await?;
            category_id = None;
            payment_method_id = None;
        }
        check_payment_method(&txn, payment_method_id).await?;
        let pm_changed = payment_method_id != found.payment_method_id;

        let mut active: transaction::ActiveModel = found.clone().into();
        active.ledger_id = Set(ledger_id);
        active.category_id = Set(category_id);
        active.payment_method_id = Set(payment_method_id);
        active.transfer_ledger_id = Set(transfer_ledger_id);
        if let Some(d) = input.date {
            active.date = Set(d);
        }
        if let Some(a) = input.amount {
            // 분할한 결제는 합계가 그대로이도록 짝 거래가 차액을 맡는다
            if let Some(g) = &group
                && let Some((partner_id, partner_amount)) = split::rebalance(g, found.id, a).map_err(user_error)?
                && let Some(partner) = g.members().find(|t| t.id == partner_id)
            {
                let mut p: transaction::ActiveModel = partner.clone().into();
                p.amount = Set(partner_amount);
                p.updated_at = Set(Utc::now());
                p.update(&txn).await?;
            }
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
            if group.is_some() && s != TxStatus::Confirmed {
                return Err(user_error(SPLIT_LOCKED));
            }
            active.status = Set(s);
        }
        if let Some(n) = input.installment_months {
            active.installment_months = Set(n.clamp(1, 60));
        }
        if let Some(v) = input.exclude_from_performance {
            active.exclude_from_performance = Set(v);
        }
        if transfer_ledger_id.is_some() {
            active.installment_months = Set(1);
            active.exclude_from_performance = Set(false);
        }
        active.updated_at = Set(Utc::now());
        let saved = active.update(&txn).await?;
        if group.is_some() {
            split::sync_payment_fields(&txn, &saved).await?;
        }
        let owner = benefit_owner(&saved);
        if let Some(ids) = &input.benefit_ids {
            set_benefits(&txn, owner, saved.payment_method_id, ids).await?;
        } else if pm_changed {
            let keep: Vec<i32> = transaction_benefit::Entity::find()
                .filter(transaction_benefit::Column::TransactionId.eq(owner))
                .all(&txn)
                .await?
                .into_iter()
                .map(|b| b.card_benefit_id)
                .collect();
            set_benefits(&txn, owner, saved.payment_method_id, &keep).await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 여러 거래를 다른 장부로 옮긴다 (하나라도 실패하면 모두 취소).
    /// 옮길 장부에 이름·종류가 같은 카테고리가 있으면 그것으로, 없으면 미분류로 둔다.
    async fn move_transactions(&self, ctx: &Context<'_>, ids: Vec<i32>, ledger_id: i32) -> Result<MoveTransactionsResult> {
        let db = db(ctx);
        let txn = db.begin().await?;
        let out = move_to_ledger(&txn, &ids, ledger_id).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(out)
    }

    /// 확정 거래에서 `amount`만큼 떼어 다른 장부의 거래로 나눈다 (한 결제를 두 장부에). 나눈 거래를 돌려준다.
    /// `categoryId`를 빼면 옮길 장부에서 이름·종류가 같은 항목, null이면 미분류.
    async fn split_transaction(
        &self,
        ctx: &Context<'_>,
        id: i32,
        ledger_id: i32,
        amount: i64,
        category_id: MaybeUndefined<i32>,
    ) -> Result<transaction::Model> {
        let category = match category_id {
            MaybeUndefined::Undefined => CategoryChoice::SameName,
            MaybeUndefined::Null => CategoryChoice::Set(None),
            MaybeUndefined::Value(v) => CategoryChoice::Set(Some(v)),
        };
        let db = db(ctx);
        let txn = db.begin().await?;
        let child = split::split(&txn, id, ledger_id, amount, category).await.map_err(split_error)?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(child)
    }

    /// 분할을 되돌려 원래 거래 한 건으로 합친다 (묶음의 어느 거래 id든 된다). 합친 거래를 돌려준다.
    async fn unsplit_transaction(&self, ctx: &Context<'_>, id: i32) -> Result<transaction::Model> {
        let db = db(ctx);
        let txn = db.begin().await?;
        let merged = split::unsplit(&txn, id).await.map_err(split_error)?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(merged)
    }

    /// 삭제. 반복 규칙이 이번 달 이후에 만든 항목은 다시 생기지 않도록 '건너뜀'으로 바꾼다.
    /// 분할한 결제는 한 건으로 합친 뒤 결제 전체를 지운다.
    async fn delete_transaction(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let db = db(ctx);
        let txn = db.begin().await?;
        delete_payments(&txn, &[id], YearMonth::of(config(ctx).today())).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(true)
    }

    /// 여러 거래 삭제 (하나라도 실패하면 모두 취소). 규칙은 `deleteTransaction`과 같고,
    /// 분할한 결제는 묶음의 거래를 여럿 골라도 한 번만 지운다.
    async fn delete_transactions(&self, ctx: &Context<'_>, ids: Vec<i32>) -> Result<DeleteTransactionsResult> {
        let db = db(ctx);
        let txn = db.begin().await?;
        let out = delete_payments(&txn, &ids, YearMonth::of(config(ctx).today())).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(out)
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
        if split::group_of(db, &found).await?.is_some() {
            return Err(user_error(SPLIT_LOCKED));
        }
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
    if split::group_of(db, &found).await?.is_some() {
        return Err(user_error(SPLIT_LOCKED));
    }
    let mut active: transaction::ActiveModel = found.into();
    active.status = Set(status);
    active.updated_at = Set(Utc::now());
    Ok(active.update(db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ledger(db: &sea_orm::DatabaseConnection, name: &str, sort_order: i32) -> i32 {
        ledger::ActiveModel {
            name: Set(name.into()),
            description: Set(None),
            is_default: Set(sort_order == 0),
            sort_order: Set(sort_order),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn category(db: &sea_orm::DatabaseConnection, ledger_id: i32, name: &str, kind: CategoryKind, archived: bool) -> i32 {
        category::ActiveModel {
            ledger_id: Set(ledger_id),
            name: Set(name.into()),
            kind: Set(kind),
            monthly_budget: Set(None),
            is_fixed: Set(false),
            color: Set(None),
            sort_order: Set(0),
            archived: Set(archived),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn tx(db: &sea_orm::DatabaseConnection, ledger_id: i32, category_id: Option<i32>) -> i32 {
        let input = TransactionInput {
            ledger_id: Some(ledger_id),
            date: NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            amount: 12_000,
            category_id,
            payment_method_id: None,
            description: "점심".into(),
            memo: None,
            member: None,
            status: TxStatus::Confirmed,
            installment_months: 1,
            exclude_from_performance: false,
            benefit_ids: None,
            transfer_ledger_id: None,
        };
        create_one(db, input).await.unwrap().id
    }

    #[tokio::test]
    async fn moves_to_other_ledger_and_maps_same_categories() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let meal = category(&db, home, "외식", CategoryKind::Expense, false).await;
        let pocket_meal = category(&db, pocket, "외식", CategoryKind::Expense, false).await;
        let grocery = category(&db, home, "장보기", CategoryKind::Expense, false).await;
        category(&db, pocket, "장보기", CategoryKind::Expense, true).await; // 보관된 카테고리로는 옮기지 않는다
        let bonus = category(&db, home, "보너스", CategoryKind::Income, false).await;
        category(&db, pocket, "보너스", CategoryKind::Expense, false).await; // 종류가 다르면 다른 항목
        let a = tx(&db, home, Some(meal)).await;
        let b = tx(&db, home, Some(grocery)).await;
        let c = tx(&db, home, Some(bonus)).await;
        let d = tx(&db, home, None).await;
        let already = tx(&db, pocket, Some(pocket_meal)).await;

        let out = move_to_ledger(&db, &[a, b, c, d, already, a], pocket).await.unwrap();
        assert_eq!(out, MoveTransactionsResult { moved: 4, uncategorized: 2 });
        let get = |id| {
            let db = db.clone();
            async move { transaction::Entity::find_by_id(id).one(&db).await.unwrap().unwrap() }
        };
        let moved_a = get(a).await;
        assert_eq!((moved_a.ledger_id, moved_a.category_id), (pocket, Some(pocket_meal)));
        assert_eq!(get(b).await.category_id, None);
        assert_eq!(get(c).await.category_id, None);
        assert_eq!(get(d).await.ledger_id, pocket);
        assert_eq!(get(already).await.category_id, Some(pocket_meal));
    }

    async fn gql(schema: &crate::graphql::CashbookSchema, query: &str) -> serde_json::Value {
        let res = schema.execute(query).await;
        assert!(res.errors.is_empty(), "{query}: {:?}", res.errors);
        res.data.into_json().unwrap()
    }

    async fn gql_error(schema: &crate::graphql::CashbookSchema, query: &str) -> String {
        let res = schema.execute(query).await;
        res.errors.first().unwrap_or_else(|| panic!("{query}: 오류가 나야 합니다")).message.clone()
    }

    #[tokio::test]
    async fn split_parts_stay_one_payment_through_edits() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let schema = crate::graphql::build_schema(db.clone(), crate::config::AppConfig::for_tests());
        let card = gql(
            &schema,
            r#"mutation { createPaymentMethod(input: {name: "카드", kind: CREDIT_CARD, benefits: [{name: "10%", rate: 0.1}]}) { id benefits { id } } }"#,
        )
        .await;
        let pm = card["createPaymentMethod"]["id"].as_i64().unwrap();
        let benefit = card["createPaymentMethod"]["benefits"][0]["id"].as_i64().unwrap();
        let created = gql(
            &schema,
            &format!(
                r#"mutation {{ createTransaction(input: {{ledgerId: {home}, date: "2026-11-03", amount: 20000, paymentMethodId: {pm}, description: "마트", benefitIds: [{benefit}]}}) {{ id }} }}"#
            ),
        )
        .await;
        let p = created["createTransaction"]["id"].as_i64().unwrap();
        let split = gql(&schema, &format!("mutation {{ splitTransaction(id: {p}, ledgerId: {pocket}, amount: 8000) {{ id amount ledgerId splitParentId benefitIds }} }}")).await;
        let c = split["splitTransaction"]["id"].as_i64().unwrap();
        assert_eq!(split["splitTransaction"]["splitParentId"], p);
        assert_eq!(split["splitTransaction"]["benefitIds"], serde_json::json!([benefit]), "혜택은 원래 거래 것");
        let read = |id: i64| format!("{{ transaction(id: {id}) {{ amount date ledgerId benefitIds splitWith {{ id amount ledgerId }} }} }}");
        let parent = gql(&schema, &read(p)).await;
        assert_eq!(parent["transaction"]["amount"], 12000);
        assert_eq!(parent["transaction"]["splitWith"], serde_json::json!([{"id": c, "amount": 8000, "ledgerId": pocket}]));

        // 한쪽 금액을 고치면 다른 쪽이 차액을 맡고, 결제 속성은 같이 바뀐다
        gql(&schema, &format!("mutation {{ updateTransaction(id: {c}, input: {{amount: 5000}}) {{ id }} }}")).await;
        gql(&schema, &format!(r#"mutation {{ updateTransaction(id: {p}, input: {{date: "2026-11-05"}}) {{ id }} }}"#)).await;
        let child = gql(&schema, &read(c)).await;
        assert_eq!(child["transaction"]["date"], "2026-11-05");
        assert_eq!(gql(&schema, &read(p)).await["transaction"]["amount"], 15000);
        assert!(gql_error(&schema, &format!("mutation {{ updateTransaction(id: {c}, input: {{amount: 20000}}) {{ id }} }}")).await.contains("분할"));

        // 자식에서 혜택을 고치면 원래 거래에 저장된다
        gql(&schema, &format!("mutation {{ updateTransaction(id: {c}, input: {{benefitIds: []}}) {{ id }} }}")).await;
        assert_eq!(gql(&schema, &read(p)).await["transaction"]["benefitIds"], serde_json::json!([]));

        // 장부 옮기기·상태 바꾸기는 분할을 되돌린 뒤에만
        for q in [
            format!("mutation {{ updateTransaction(id: {c}, input: {{ledgerId: {home}}}) {{ id }} }}"),
            format!("mutation {{ moveTransactions(ids: [{p}], ledgerId: {pocket}) {{ moved }} }}"),
            format!("mutation {{ updateTransaction(id: {p}, input: {{status: PLANNED}}) {{ id }} }}"),
            format!("mutation {{ skipTransaction(id: {p}) {{ id }} }}"),
        ] {
            assert!(gql_error(&schema, &q).await.contains("분할"), "{q}");
        }

        // 한쪽을 지우면 결제 전체가 지워진다
        gql(&schema, &format!("mutation {{ deleteTransaction(id: {c}) }}")).await;
        for id in [p, c] {
            assert!(gql(&schema, &read(id)).await["transaction"].is_null());
        }
    }

    #[tokio::test]
    async fn split_payment_is_one_card_transaction() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let schema = crate::graphql::build_schema(db.clone(), crate::config::AppConfig::for_tests());
        // 1만원 이상 10%, 건당 최대 1,500원
        let card = gql(
            &schema,
            r#"mutation { createPaymentMethod(input: {name: "카드", kind: CREDIT_CARD, benefits: [{name: "10%", rate: 0.1, minAmount: 10000, perTxMax: 1500}]}) { id benefits { id } } }"#,
        )
        .await;
        let pm = card["createPaymentMethod"]["id"].as_i64().unwrap();
        let benefit = card["createPaymentMethod"]["benefits"][0]["id"].as_i64().unwrap();
        let created = gql(
            &schema,
            &format!(
                r#"mutation {{ createTransaction(input: {{ledgerId: {home}, date: "2026-11-03", amount: 20000, paymentMethodId: {pm}, description: "마트", benefitIds: [{benefit}]}}) {{ id }} }}"#
            ),
        )
        .await;
        let p = created["createTransaction"]["id"].as_i64().unwrap();
        let report = format!(r#"{{ paymentMethod(id: {pm}) {{ monthReport(month: "2026-11") {{ spending transactionCount discountTotal transactionBenefits {{ transactionId amount }} }} }} }}"#);
        let before = gql(&schema, &report).await;
        // 가계부 9,000 + 용돈 11,000: 따로 계산하면 9,000원은 최소 금액 미달이지만 결제는 20,000원 한 건
        gql(&schema, &format!("mutation {{ splitTransaction(id: {p}, ledgerId: {pocket}, amount: 11000) {{ id }} }}")).await;
        let after = gql(&schema, &report).await;
        assert_eq!(after, before);
        let r = &after["paymentMethod"]["monthReport"];
        assert_eq!((r["spending"].as_i64(), r["transactionCount"].as_i64(), r["discountTotal"].as_i64()), (Some(20000), Some(1), Some(1500)));
        assert_eq!(r["transactionBenefits"], serde_json::json!([{"transactionId": p, "amount": 1500}]));
    }

    #[tokio::test]
    async fn deletes_many_payments_at_once() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let cfg = crate::config::AppConfig::for_tests();
        let this_month = YearMonth::of(cfg.today());
        let schema = crate::graphql::build_schema(db.clone(), cfg);
        let plain = tx(&db, home, None).await;
        let keep = tx(&db, home, None).await;
        let p = tx(&db, home, None).await;
        let split = gql(&schema, &format!("mutation {{ splitTransaction(id: {p}, ledgerId: {pocket}, amount: 2000) {{ id }} }}")).await;
        let c = split["splitTransaction"]["id"].as_i64().unwrap();
        gql(
            &schema,
            &format!(
                r#"mutation {{ createRecurringRule(input: {{ledgerId: {home}, description: "구독", amount: 9900, dayOfMonth: 1, startMonth: "{this_month}"}}) {{ id }} }}"#
            ),
        )
        .await;
        let planned = gql(&schema, r#"{ transactions(filter: {search: "구독", statuses: [PLANNED]}) { items { id } } }"#).await;
        let recurring = planned["transactions"]["items"][0]["id"].as_i64().unwrap();
        let read = |id: i64| format!("{{ transaction(id: {id}) {{ status }} }}");

        // 없는 거래가 섞이면 아무것도 지우지 않는다
        let stale = format!("mutation {{ deleteTransactions(ids: [{plain}, {}]) {{ deleted }} }}", keep + 1000);
        assert!(gql_error(&schema, &stale).await.contains("없는 거래"));
        assert_eq!(gql(&schema, &read(plain.into())).await["transaction"]["status"], "CONFIRMED");

        // 분할 묶음은 양쪽을 다 골라도 결제 한 건, 반복 항목은 건너뜀으로 남는다
        let out = gql(&schema, &format!("mutation {{ deleteTransactions(ids: [{c}, {plain}, {p}, {recurring}, {plain}]) {{ deleted skipped }} }}")).await;
        assert_eq!(out["deleteTransactions"], serde_json::json!({"deleted": 2, "skipped": 1}));
        for id in [plain.into(), p.into(), c] {
            assert!(gql(&schema, &read(id)).await["transaction"].is_null(), "{id}");
        }
        assert_eq!(gql(&schema, &read(recurring)).await["transaction"]["status"], "SKIPPED");
        assert_eq!(gql(&schema, &read(keep.into())).await["transaction"]["status"], "CONFIRMED");
    }

    #[tokio::test]
    async fn description_suggestions_within_period_split_by_category() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let cafe = category(&db, home, "카페", CategoryKind::Expense, false).await;
        let dinner = category(&db, home, "회식", CategoryKind::Expense, false).await;
        let schema = crate::graphql::build_schema(db.clone(), crate::config::AppConfig::for_tests());
        for (date, amount, description, category) in [
            ("2026-04-01", 6000, "스타벅스 역삼", Some(cafe)),
            ("2026-05-01", 5000, "스타벅스", Some(dinner)),
            ("2026-05-02", 5000, "스타벅스", Some(dinner)),
            ("2026-09-01", 3000, "스타벅스", None),
            ("2026-10-01", 4500, "스타벅스", Some(cafe)),
            ("2026-10-15", 80000, "스타벅스", Some(dinner)),
            ("2026-11-01", 5500, "스타벅스", Some(cafe)),
        ] {
            let category = category.map_or("null".to_string(), |id| id.to_string());
            gql(
                &schema,
                &format!(
                    r#"mutation {{ createTransaction(input: {{ledgerId: {home}, date: "{date}", amount: {amount}, categoryId: {category}, description: "{description}"}}) {{ id }} }}"#
                ),
            )
            .await;
        }
        let fields = "description categoryId count lastAmount lastDate";

        // 기간 안에서 (내용, 항목)마다 따로: 많이 쓴 순, 같으면 최근 순. 기간 밖 거래는 횟수에도 넣지 않는다
        let out = gql(&schema, &format!(r#"{{ descriptionSuggestions(query: "스타", since: "2026-08-20", byCategory: true) {{ {fields} }} }}"#)).await;
        assert_eq!(
            out["descriptionSuggestions"],
            serde_json::json!([
                {"description": "스타벅스", "categoryId": cafe, "count": 2, "lastAmount": 5500, "lastDate": "2026-11-01"},
                {"description": "스타벅스", "categoryId": dinner, "count": 1, "lastAmount": 80000, "lastDate": "2026-10-15"},
                {"description": "스타벅스", "categoryId": null, "count": 1, "lastAmount": 3000, "lastDate": "2026-09-01"},
            ])
        );

        // 항목별로 나누지 않으면 내용마다 기간 안에서 가장 많이 쓴 분류 하나
        let out = gql(&schema, &format!(r#"{{ descriptionSuggestions(query: "스타", since: "2026-08-20") {{ {fields} }} }}"#)).await;
        assert_eq!(
            out["descriptionSuggestions"],
            serde_json::json!([{"description": "스타벅스", "categoryId": cafe, "count": 2, "lastAmount": 5500, "lastDate": "2026-11-01"}])
        );

        // 기간이 없으면 전체 내역
        let out = gql(&schema, r#"{ descriptionSuggestions(query: "스타", byCategory: true) { description categoryId count } }"#).await;
        assert_eq!(
            out["descriptionSuggestions"],
            serde_json::json!([
                {"description": "스타벅스", "categoryId": dinner, "count": 3},
                {"description": "스타벅스", "categoryId": cafe, "count": 2},
                {"description": "스타벅스", "categoryId": null, "count": 1},
                {"description": "스타벅스 역삼", "categoryId": cafe, "count": 1},
            ])
        );
    }

    #[tokio::test]
    async fn ledger_transfers_move_money_between_ledgers() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let meal = category(&db, home, "외식", CategoryKind::Expense, false).await;
        let cfg = crate::config::AppConfig::for_tests();
        let this_month = YearMonth::of(cfg.today());
        let schema = crate::graphql::build_schema(db.clone(), cfg);
        tx(&db, home, Some(meal)).await; // 가계부 지출 12,000

        // 항목·결제수단을 같이 주거나 같은 장부로 보내면 거절
        for bad in [
            format!(r#"ledgerId: {home}, categoryId: {meal}, transferLedgerId: {pocket}"#),
            format!(r#"ledgerId: {home}, transferLedgerId: {home}"#),
        ] {
            let q = format!(r#"mutation {{ createTransaction(input: {{{bad}, date: "2026-11-01", amount: 1, description: "x"}}) {{ id }} }}"#);
            assert!(gql_error(&schema, &q).await.contains("장부"), "{q}");
        }
        let created = gql(
            &schema,
            &format!(
                r#"mutation {{ createTransaction(input: {{ledgerId: {home}, date: "2026-11-01", amount: 200000, description: "재홍 용돈", transferLedgerId: {pocket}}}) {{ id }} }}"#
            ),
        )
        .await;
        let t = created["createTransaction"]["id"].as_i64().unwrap();

        // 보낸 장부·받은 장부 양쪽 목록에 보이고, 지출·수입 합계와 따로 센다
        let page = |ledger: i32, extra: &str| {
            format!(
                r#"{{ transactions(filter: {{month: "2026-11", ledgerId: {ledger}{extra}}}) {{ totalCount confirmedTotal incomeTotal transferInTotal transferOutTotal items {{ id }} }} }}"#
            )
        };
        let out = gql(&schema, &page(home, "")).await;
        assert_eq!(
            out["transactions"],
            serde_json::json!({"totalCount": 2, "confirmedTotal": 12000, "incomeTotal": 0, "transferInTotal": 0, "transferOutTotal": 200000,
                "items": out["transactions"]["items"]})
        );
        let into = gql(&schema, &page(pocket, "")).await;
        assert_eq!(into["transactions"]["items"], serde_json::json!([{"id": t}]));
        assert_eq!((into["transactions"]["confirmedTotal"].as_i64(), into["transactions"]["transferInTotal"].as_i64()), (Some(0), Some(200000)));
        // 미분류에는 이체가 섞이지 않는다
        assert_eq!(gql(&schema, &page(home, ", uncategorized: true")).await["transactions"]["totalCount"], 0);
        assert_eq!(gql(&schema, &page(home, ", transfers: true")).await["transactions"]["totalCount"], 1);

        // 받는 장부로 옮기기·분할은 안 된다
        assert!(gql_error(&schema, &format!("mutation {{ moveTransactions(ids: [{t}], ledgerId: {pocket}) {{ moved }} }}")).await.contains("이체"));
        assert!(
            gql_error(&schema, &format!("mutation {{ splitTransaction(id: {t}, ledgerId: {pocket}, amount: 1000) {{ id }} }}"))
                .await
                .contains("이체")
        );
        // 받은 장부가 있으면 장부를 지울 수 없다
        assert!(gql_error(&schema, &format!("mutation {{ deleteLedger(id: {pocket}) }}")).await.contains("받은 이체"));

        // 항목을 고르면 이체가 풀리고, 받는 장부를 고르면 항목이 비워진다
        let read = format!("{{ transaction(id: {t}) {{ ledgerId categoryId transferLedgerId }} }}");
        gql(&schema, &format!("mutation {{ updateTransaction(id: {t}, input: {{categoryId: {meal}}}) {{ id }} }}")).await;
        assert_eq!(gql(&schema, &read).await["transaction"], serde_json::json!({"ledgerId": home, "categoryId": meal, "transferLedgerId": null}));
        gql(&schema, &format!("mutation {{ updateTransaction(id: {t}, input: {{transferLedgerId: {pocket}}}) {{ id }} }}")).await;
        assert_eq!(gql(&schema, &read).await["transaction"], serde_json::json!({"ledgerId": home, "categoryId": null, "transferLedgerId": pocket}));
        // 보내는 장부를 받는 장부와 같게 바꿀 수 없다
        assert!(gql_error(&schema, &format!("mutation {{ updateTransaction(id: {t}, input: {{ledgerId: {pocket}}}) {{ id }} }}")).await.contains("받는 장부"));

        // 반복 이체 규칙은 예정 이체를 만들고, 받는 장부의 규칙 목록에도 보인다
        let rule = gql(
            &schema,
            &format!(
                r#"mutation {{ createRecurringRule(input: {{ledgerId: {home}, description: "용돈", amount: 200000, dayOfMonth: 1, startMonth: "{this_month}", transferLedgerId: {pocket}}}) {{ id }} }}"#
            ),
        )
        .await;
        let rule_id = rule["createRecurringRule"]["id"].as_i64().unwrap();
        let rules = gql(&schema, &format!("{{ recurringRules(ledgerId: {pocket}) {{ id transferLedgerId }} }}")).await;
        assert_eq!(rules["recurringRules"], serde_json::json!([{"id": rule_id, "transferLedgerId": pocket}]));
        let planned = gql(
            &schema,
            &format!(r#"{{ transactions(filter: {{month: "{this_month}", ledgerId: {pocket}, statuses: [PLANNED]}}) {{ items {{ ledgerId transferLedgerId amount }} }} }}"#),
        )
        .await;
        assert_eq!(planned["transactions"]["items"], serde_json::json!([{"ledgerId": home, "transferLedgerId": pocket, "amount": 200000}]));
        assert!(
            gql_error(
                &schema,
                &format!(
                    r#"mutation {{ createRecurringRule(input: {{ledgerId: {home}, description: "x", amount: 1, dayOfMonth: 1, startMonth: "{this_month}", categoryId: {meal}, transferLedgerId: {pocket}}}) {{ id }} }}"#
                )
            )
            .await
            .contains("이체")
        );
    }

    #[tokio::test]
    async fn rejects_unknown_ledger_or_transaction() {
        let db = crate::db::connect_in_memory().await;
        let home = ledger(&db, "가계부", 0).await;
        let pocket = ledger(&db, "용돈", 1).await;
        let a = tx(&db, home, None).await;
        assert!(move_to_ledger(&db, &[a], pocket + 100).await.is_err());
        assert!(move_to_ledger(&db, &[a, a + 100], pocket).await.is_err());
        assert_eq!(transaction::Entity::find_by_id(a).one(&db).await.unwrap().unwrap().ledger_id, home);
    }
}
