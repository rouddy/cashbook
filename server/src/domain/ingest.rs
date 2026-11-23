//! 자동 입력: 아이폰 단축어, 안드로이드 문자 전달 앱, 스크립트 등에서 결제 내역을 보내면 거래로 저장한다.
//!
//! 처리 순서
//! 1. 원문(`text`)이 있으면 카드 승인 문자로 보고 금액/가맹점/일시/할부/취소/카드를 뽑는다. 명시 필드가 우선한다.
//! 2. `externalId`(없으면 원문 해시)로 중복을 막는다.
//! 3. 결제수단: 이름·별칭 → 카드번호 끝 4자리 → 원문에 나온 카드 이름 → 카드사(해당 카드사 카드가 하나일 때)
//! 4. 장부/카테고리: 명시값 → 같은 가맹점의 과거 거래에서 가장 많이 쓴 분류 → 기본 장부
//! 5. 같은 결제수단의 ±7일 예정 거래와 금액(또는 내용/키워드)이 맞으면 그 예정 거래를 확정 처리.
//!    반복 규칙에 금액 범위·매칭 키워드를 정했으면 둘 다 맞아야 한다 (`match_rank`).
//! 6. 아니면 새 확정 거래를 만든다.
//!
//! 거래의 `raw_text`에는 받은 원문을 남긴다: `text`는 받은 그대로, `text` 없이 필드만 오면 받은 필드의 JSON.

use std::collections::HashMap;
use std::fmt;

use async_graphql::{InputObject, SimpleObject};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Select, Set, sea_query::Expr,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::sms::{self, ParsedMessage};
use crate::db::sql;
use crate::entity::{
    OfUser, category,
    enums::{CategoryKind, PaymentKind, TxSource, TxStatus},
    ledger, payment_method, recurring_rule, transaction, transaction_benefit,
};

#[derive(InputObject, Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IngestInput {
    /// 금액(원). 없으면 `text`에서 찾는다.
    pub amount: Option<i64>,
    /// 내용/가맹점 (`merchant`로 보내도 된다)
    #[serde(alias = "merchant")]
    pub description: Option<String>,
    /// 거래일 (YYYY-MM-DD). 없으면 원문 날짜 → 오늘
    pub date: Option<NaiveDate>,
    /// 결제수단 이름·별칭 또는 카드번호 끝 4자리 (`card`로 보내도 된다)
    #[serde(alias = "card")]
    pub payment_method: Option<String>,
    /// 카테고리 이름
    pub category: Option<String>,
    /// 장부 이름 (예: 가계부, 용돈)
    pub ledger: Option<String>,
    pub member: Option<String>,
    pub memo: Option<String>,
    pub installment_months: Option<i32>,
    /// 취소/환불이면 true → 음수 금액으로 저장
    pub cancel: Option<bool>,
    /// 카드 승인 문자/알림 원문
    pub text: Option<String>,
    /// 중복 방지 키. 같은 키로 다시 보내면 기존 거래를 돌려준다.
    pub external_id: Option<String>,
    /// true면 예정(PLANNED) 거래로 저장
    pub planned: Option<bool>,
}

#[derive(SimpleObject, Clone, Debug)]
#[graphql(name = "IngestResult")]
pub struct IngestOutcome {
    pub transaction: transaction::Model,
    /// 새 거래를 만들었는지
    pub created: bool,
    /// 이미 같은 키로 들어온 거래인지
    pub duplicate: bool,
    /// 예정 거래를 확정 처리했는지
    pub matched_planned: bool,
    pub parsed: Option<ParsedMessage>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum IngestError {
    Invalid(String),
    Db(DbErr),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngestError::Invalid(msg) => write!(f, "{msg}"),
            IngestError::Db(e) => write!(f, "DB 오류: {e}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<DbErr> for IngestError {
    fn from(e: DbErr) -> Self {
        IngestError::Db(e)
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// 결제수단 찾기. 애매하면 None.
pub fn resolve_payment_method<'a>(
    pms: &'a [payment_method::Model],
    hint: Option<&str>,
    last4: Option<&str>,
    issuer: Option<&str>,
    text: Option<&str>,
) -> Option<&'a payment_method::Model> {
    let active = || pms.iter().filter(|p| !p.archived);
    if let Some(h) = hint.map(norm).filter(|h| !h.is_empty()) {
        let by_name = |p: &&payment_method::Model| norm(&p.name) == h || p.alias_list().iter().any(|a| norm(a) == h);
        if let Some(p) = active().find(by_name).or_else(|| pms.iter().find(by_name)) {
            return Some(p);
        }
        if h.len() == 4 && h.chars().all(|c| c.is_ascii_digit())
            && let Some(p) = active().find(|p| p.last4.as_deref() == Some(h.as_str())) {
                return Some(p);
            }
        let partial: Vec<_> = active()
            .filter(|p| {
                let n = norm(&p.name);
                n.chars().count() >= 2 && (h.contains(&n) || n.contains(&h))
            })
            .collect();
        if partial.len() == 1 {
            return Some(partial[0]);
        }
    }
    if let Some(l4) = last4
        && let Some(p) = active().find(|p| p.last4.as_deref() == Some(l4)) {
            return Some(p);
        }
    if let Some(t) = text.map(norm) {
        let mentioned: Vec<_> = active()
            .filter(|p| p.kind.is_card())
            .filter(|p| {
                std::iter::once(p.name.clone())
                    .chain(p.alias_list())
                    .map(|n| norm(&n))
                    .any(|n| n.chars().count() >= 2 && t.contains(&n))
            })
            .collect();
        if mentioned.len() == 1 {
            return Some(mentioned[0]);
        }
    }
    if let Some(iss) = issuer.map(norm) {
        let by_issuer: Vec<_> = active()
            .filter(|p| p.kind.is_card())
            .filter(|p| {
                p.issuer.as_deref().map(norm).is_some_and(|i| !i.is_empty() && (i == iss || i.contains(&iss) || iss.contains(&i)))
            })
            .collect();
        if by_issuer.len() == 1 {
            return Some(by_issuer[0]);
        }
        // 카드사만 맞으면 본인 카드와 그 가족카드 사이에서는 본인 카드 (끝자리 없는 문자)
        let heads: Vec<_> = by_issuer.iter().filter(|p| p.parent_id.is_none()).collect();
        if let [head] = heads[..]
            && by_issuer.iter().all(|p| p.parent_id.is_none_or(|id| id == head.id))
        {
            return Some(head);
        }
    }
    None
}

/// 과거 거래 기반 분류 추천.
#[derive(SimpleObject, Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub description: String,
    pub ledger_id: i32,
    pub category_id: Option<i32>,
    pub payment_method_id: Option<i32>,
    pub member: Option<String>,
    /// 같은 결제수단으로 썼을 때 붙였던 혜택 항목
    pub benefit_ids: Vec<i32>,
    pub last_amount: i64,
    pub last_date: NaiveDate,
    pub count: i32,
}

/// 추천에 쓰는 과거 거래: 확정 거래, `since`가 있으면 그날 이후만.
/// 분할해 옮긴 거래는 원래 거래의 분류를 흐리지 않게, 장부 이체는 결제가 아니라서 뺀다.
fn history(uid: i32, since: Option<NaiveDate>) -> Select<transaction::Entity> {
    transaction::Entity::find()
        .of_user(uid)
        .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
        .filter(transaction::Column::SplitParentId.is_null())
        .filter(transaction::Column::TransferLedgerId.is_null())
        .apply_if(since, |q, d| q.filter(transaction::Column::Date.gte(d)))
}

async fn benefit_ids_of<C: ConnectionTrait>(db: &C, transaction_id: i32) -> Result<Vec<i32>, DbErr> {
    Ok(transaction_benefit::Entity::find()
        .filter(transaction_benefit::Column::TransactionId.eq(transaction_id))
        .order_by_asc(transaction_benefit::Column::CardBenefitId)
        .all(db)
        .await?
        .into_iter()
        .map(|b| b.card_benefit_id)
        .collect())
}

/// 같은(또는 서로 포함하는) 내용의 최근 확정 거래로 장부/카테고리를 추천한다.
pub async fn suggest<C: ConnectionTrait>(
    db: &C,
    uid: i32,
    description: &str,
    payment_method_id: Option<i32>,
) -> Result<Option<Suggestion>, DbErr> {
    suggest_since(db, uid, description, payment_method_id, None).await
}

/// [`suggest`]와 같고, `since`가 있으면 그날 이후 거래만 본다.
pub async fn suggest_since<C: ConnectionTrait>(
    db: &C,
    uid: i32,
    description: &str,
    payment_method_id: Option<i32>,
    since: Option<NaiveDate>,
) -> Result<Option<Suggestion>, DbErr> {
    let d = description.trim();
    if d.chars().count() < 2 {
        return Ok(None);
    }
    let contained_in_d = format!(
        "({}(description) >= 2 AND instr(?, description) > 0)",
        sql::char_length(db.get_database_backend())
    );
    let rows = history(uid, since)
        .filter(
            Condition::any()
                .add(transaction::Column::Description.eq(d))
                .add(Expr::cust_with_values("instr(description, ?) > 0", [d]))
                .add(Expr::cust_with_values(contained_in_d, [d])),
        )
        .order_by_desc(transaction::Column::Date)
        .order_by_desc(transaction::Column::Id)
        .limit(60)
        .all(db)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let exact: Vec<&transaction::Model> = rows.iter().filter(|r| r.description == d).collect();
    let pool: Vec<&transaction::Model> = if exact.is_empty() { rows.iter().collect() } else { exact };
    // (장부, 카테고리)별 횟수, 동률이면 최근 것
    let mut counts: HashMap<(i32, Option<i32>), (usize, usize)> = HashMap::new();
    for (i, r) in pool.iter().enumerate() {
        let e = counts.entry((r.ledger_id, r.category_id)).or_insert((0, i));
        e.0 += 1;
    }
    let (&(ledger_id, category_id), &(count, _)) = counts
        .iter()
        .max_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)))
        .expect("pool is not empty");
    let latest = pool
        .iter()
        .find(|r| r.ledger_id == ledger_id && r.category_id == category_id)
        .copied()
        .unwrap_or(pool[0]);
    let pm_for_benefits = payment_method_id.or(latest.payment_method_id);
    let benefit_source = pool.iter().find(|r| r.payment_method_id == pm_for_benefits && pm_for_benefits.is_some());
    let benefit_ids = match benefit_source {
        Some(src) => benefit_ids_of(db, src.id).await?,
        None => vec![],
    };
    Ok(Some(Suggestion {
        description: latest.description.clone(),
        ledger_id,
        category_id,
        payment_method_id: latest.payment_method_id,
        member: latest.member.clone(),
        benefit_ids,
        last_amount: latest.amount,
        last_date: latest.date,
        count: count as i32,
    }))
}

/// 내용에 `query`가 들어간 과거 거래를 (내용, 장부, 카테고리)마다 하나씩, 자주 쓴 순으로 돌려준다.
/// 같은 내용이라도 분류가 다르면 따로 나와 어느 쪽이든 고를 수 있다.
/// 결제수단·구성원·혜택·최근 금액은 그 묶음의 가장 최근 거래 것이고, 횟수는 `since` 이후만 센다.
pub async fn suggestions_by_category<C: ConnectionTrait>(
    db: &C,
    uid: i32,
    query: &str,
    ledger_id: Option<i32>,
    since: Option<NaiveDate>,
    limit: u64,
) -> Result<Vec<Suggestion>, DbErr> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let groups: Vec<(String, i32, Option<i32>, i64)> = history(uid, since)
        .select_only()
        .column(transaction::Column::Description)
        .column(transaction::Column::LedgerId)
        .column(transaction::Column::CategoryId)
        .column_as(Expr::cust("count(*)"), "n")
        .filter(Expr::cust_with_values("instr(description, ?) > 0", [q]))
        .apply_if(ledger_id, |s, id| s.filter(transaction::Column::LedgerId.eq(id)))
        .group_by(transaction::Column::Description)
        .group_by(transaction::Column::LedgerId)
        .group_by(transaction::Column::CategoryId)
        // 횟수가 같으면 최근에 쓴 것, 그다음 이름·장부·카테고리 순 (DB와 관계없이 같은 결과)
        .order_by_desc(Expr::cust("count(*)"))
        .order_by_desc(Expr::cust("max(date)"))
        .order_by_asc(transaction::Column::Description)
        .order_by_asc(transaction::Column::LedgerId)
        .order_by_asc(transaction::Column::CategoryId)
        .limit(limit)
        .into_tuple()
        .all(db)
        .await?;
    let mut out = Vec::with_capacity(groups.len());
    for (description, ledger_id, category_id, count) in groups {
        let same_category = match category_id {
            Some(id) => transaction::Column::CategoryId.eq(id),
            None => transaction::Column::CategoryId.is_null(),
        };
        let Some(latest) = history(uid, since)
            .filter(transaction::Column::Description.eq(description.as_str()))
            .filter(transaction::Column::LedgerId.eq(ledger_id))
            .filter(same_category)
            .order_by_desc(transaction::Column::Date)
            .order_by_desc(transaction::Column::Id)
            .one(db)
            .await?
        else {
            continue;
        };
        out.push(Suggestion {
            benefit_ids: benefit_ids_of(db, latest.id).await?,
            description,
            ledger_id,
            category_id,
            payment_method_id: latest.payment_method_id,
            member: latest.member,
            last_amount: latest.amount,
            last_date: latest.date,
            count: count as i32,
        });
    }
    Ok(out)
}

fn strip_day_prefix(s: &str) -> &str {
    // "(25)아파트관리비" 같은 예정 표기
    if let Some(rest) = s.strip_prefix('(')
        && let Some(i) = rest.find(')')
            && rest[..i].chars().all(|c| c.is_ascii_digit()) {
                return rest[i + 1..].trim();
            }
    s
}

fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (norm(strip_day_prefix(a)), norm(strip_day_prefix(b)));
    a.chars().count() >= 2 && b.chars().count() >= 2 && (a.contains(&b) || b.contains(&a))
}

/// 반복 규칙이 예정 항목에 거는 자동 입력 매칭 조건. 정한 조건은 모두 맞아야 한다(필터).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleMatch {
    /// 정규화한 키워드 (비어 있으면 조건 없음)
    keywords: Vec<String>,
    min: Option<i64>,
    max: Option<i64>,
}

impl RuleMatch {
    fn new(keyword: Option<&str>, min: Option<i64>, max: Option<i64>) -> Self {
        let keywords = keyword.unwrap_or("").split(',').map(norm).filter(|k| !k.is_empty()).collect();
        Self { keywords, min, max }
    }

    fn of(rule: &recurring_rule::Model) -> Self {
        Self::new(rule.match_keyword.as_deref(), rule.match_amount_min, rule.match_amount_max)
    }

    /// 금액 범위를 정했으면 그 안인지 (양끝 포함). 범위가 없으면 `None`.
    fn amount_in_range(&self, amount: i64) -> Option<bool> {
        (self.min.is_some() || self.max.is_some())
            .then(|| self.min.is_none_or(|m| amount >= m) && self.max.is_none_or(|m| amount <= m))
    }

    /// 키워드를 정했으면 정규화한 가맹점에 하나라도 들어 있는지. 키워드가 없으면 `None`.
    fn keyword_hit(&self, description_norm: &str) -> Option<bool> {
        (!self.keywords.is_empty()).then(|| self.keywords.iter().any(|k| description_norm.contains(k.as_str())))
    }
}

/// 예정 항목 하나가 자동 입력과 맞으면 우선순위(작을수록 먼저), 아니면 `None`.
/// 반복 규칙의 금액 범위·키워드는 정했으면 반드시 맞아야 한다. 그다음 금액(범위가 있으면 범위 안, 없으면 예정 금액과 같음)·
/// 키워드·내용 비슷함 중 하나는 맞아야 하고, 금액과 내용(키워드)이 함께 맞을수록 먼저 고른다.
fn match_rank(planned: (i64, &str), rule: Option<&RuleMatch>, amount: i64, description: &str) -> Option<u8> {
    let (planned_amount, planned_description) = planned;
    let in_range = rule.and_then(|r| r.amount_in_range(amount));
    let keyword = rule.and_then(|r| r.keyword_hit(&norm(description)));
    if in_range == Some(false) || keyword == Some(false) {
        return None;
    }
    let amount_match = in_range.unwrap_or(planned_amount == amount);
    let text_match = keyword == Some(true) || similar(planned_description, description);
    match (amount_match, text_match) {
        (true, true) => Some(0),
        (false, true) => Some(1),
        (true, false) => Some(2),
        (false, false) => None,
    }
}

async fn find_planned_match<C: ConnectionTrait>(
    db: &C,
    uid: i32,
    payment_method_id: i32,
    amount: i64,
    date: NaiveDate,
    description: &str,
) -> Result<Option<transaction::Model>, DbErr> {
    let candidates = transaction::Entity::find()
        .of_user(uid)
        .filter(transaction::Column::Status.eq(TxStatus::Planned))
        .filter(transaction::Column::PaymentMethodId.eq(payment_method_id))
        .filter(transaction::Column::Date.between(date - Duration::days(7), date + Duration::days(7)))
        .all(db)
        .await?;
    if candidates.is_empty() {
        return Ok(None);
    }
    let rule_ids: Vec<i32> = candidates.iter().filter_map(|c| c.recurring_rule_id).collect();
    let rules: HashMap<i32, RuleMatch> = if rule_ids.is_empty() {
        HashMap::new()
    } else {
        recurring_rule::Entity::find()
            .of_user(uid)
            .filter(recurring_rule::Column::Id.is_in(rule_ids))
            .all(db)
            .await?
            .iter()
            .map(|r| (r.id, RuleMatch::of(r)))
            .collect()
    };
    let best = candidates
        .into_iter()
        .filter_map(|c| {
            let rule = c.recurring_rule_id.and_then(|id| rules.get(&id));
            let rank = match_rank((c.amount, &c.description), rule, amount, description)?;
            Some(((rank, (c.date - date).num_days().abs(), (c.amount - amount).abs(), c.id), c))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, c)| c);
    Ok(best)
}

/// 거래에 남길 자동 입력 원문. 승인 문자(`text`)는 받은 그대로, 없으면 받은 필드를 JSON으로.
fn raw_input(input: &IngestInput) -> Option<String> {
    if let Some(t) = input.text.as_deref().filter(|t| !t.trim().is_empty()) {
        return Some(t.to_string());
    }
    let mut value = serde_json::to_value(input).ok()?;
    if let Some(fields) = value.as_object_mut() {
        fields.retain(|_, v| !v.is_null());
    }
    Some(value.to_string())
}

fn hash_text(text: &str) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    format!("text:{}", &hex::encode(digest)[..32])
}

/// 자동 입력 규칙을 만들 때 고를 수 있는 값 (`GET /api/ingest/options`).
/// 규칙 화면의 선택 목록이라 이름처럼 규칙에 필요한 것만 담고 거래·금액은 넣지 않는다.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IngestOptions {
    pub ledgers: Vec<LedgerOption>,
    pub categories: Vec<CategoryOption>,
    pub payment_methods: Vec<PaymentMethodOption>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerOption {
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryOption {
    pub name: String,
    /// 장부 이름
    pub ledger: String,
    pub kind: CategoryKind,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentMethodOption {
    pub name: String,
    pub kind: PaymentKind,
    pub issuer: Option<String>,
    pub last4: Option<String>,
    pub aliases: Vec<String>,
}

/// 보관하지 않은 장부·카테고리·결제수단. 화면과 같은 순서(sort_order, id)로, 기본 장부를 먼저 둔다.
pub async fn options<C: ConnectionTrait>(db: &C, uid: i32) -> Result<IngestOptions, DbErr> {
    let mut ledgers = ledger::Entity::find().of_user(uid).all(db).await?;
    ledgers.sort_by_key(|l| (!l.is_default, l.sort_order, l.id));
    let ledger_order: HashMap<i32, usize> = ledgers.iter().enumerate().map(|(i, l)| (l.id, i)).collect();

    let mut categories: Vec<category::Model> =
        category::Entity::find().of_user(uid).all(db).await?.into_iter().filter(|c| !c.archived && ledger_order.contains_key(&c.ledger_id)).collect();
    categories.sort_by_key(|c| (ledger_order[&c.ledger_id], c.kind == CategoryKind::Income, c.sort_order, c.id));

    let mut pms: Vec<payment_method::Model> =
        payment_method::Entity::find().of_user(uid).all(db).await?.into_iter().filter(|p| !p.archived).collect();
    pms.sort_by_key(|p| (p.sort_order, p.id));

    let ledger_name: HashMap<i32, &str> = ledgers.iter().map(|l| (l.id, l.name.as_str())).collect();
    Ok(IngestOptions {
        categories: categories
            .iter()
            .map(|c| CategoryOption { name: c.name.clone(), ledger: ledger_name[&c.ledger_id].to_string(), kind: c.kind })
            .collect(),
        payment_methods: pms
            .iter()
            .map(|p| PaymentMethodOption {
                name: p.name.clone(),
                kind: p.kind,
                issuer: p.issuer.clone().filter(|s| !s.trim().is_empty()),
                last4: p.last4.clone().filter(|s| !s.trim().is_empty()),
                aliases: p.alias_list(),
            })
            .collect(),
        ledgers: ledgers.iter().map(|l| LedgerOption { name: l.name.clone(), is_default: l.is_default }).collect(),
    })
}

pub async fn ingest<C: ConnectionTrait>(
    db: &C,
    uid: i32,
    input: IngestInput,
    today: NaiveDate,
) -> Result<IngestOutcome, IngestError> {
    let mut warnings = Vec::new();
    let raw_text = raw_input(&input);
    let text = non_empty(input.text.as_deref()).map(str::to_string);
    let parsed = text.as_deref().map(|t| sms::parse(t, today));
    let p = parsed.clone().unwrap_or_default();

    let amount = input
        .amount
        .or(p.amount)
        .ok_or_else(|| IngestError::Invalid("금액(amount)이 없고 원문에서도 찾지 못했습니다".into()))?;
    if amount == 0 {
        return Err(IngestError::Invalid("금액이 0원입니다".into()));
    }
    let cancel = input.cancel.unwrap_or(false) || p.cancel;
    let amount = if cancel { -amount.abs() } else { amount };
    let description = non_empty(input.description.as_deref())
        .map(str::to_string)
        .or(p.merchant.clone())
        .unwrap_or_else(|| {
            warnings.push("내용(가맹점)을 찾지 못해 '자동입력'으로 저장했습니다".into());
            "자동입력".into()
        });
    let date = input.date.or(p.date).unwrap_or(today);
    let installment_months = input.installment_months.or(p.installment_months).unwrap_or(1).clamp(1, 60);
    let external_id = non_empty(input.external_id.as_deref())
        .map(|s| s.chars().take(200).collect::<String>())
        .or_else(|| text.as_deref().map(hash_text));

    if let Some(eid) = &external_id
        && let Some(existing) = transaction::Entity::find()
            .of_user(uid)
            .filter(transaction::Column::ExternalId.eq(eid.as_str()))
            .one(db)
            .await?
        {
            return Ok(IngestOutcome {
                transaction: existing,
                created: false,
                duplicate: true,
                matched_planned: false,
                parsed,
                warnings,
            });
        }

    let pms = payment_method::Entity::find().of_user(uid).all(db).await?;
    let pm = resolve_payment_method(
        &pms,
        non_empty(input.payment_method.as_deref()),
        p.last4.as_deref(),
        p.issuer.as_deref(),
        text.as_deref(),
    );
    if pm.is_none() {
        let hint = non_empty(input.payment_method.as_deref()).or(p.issuer.as_deref()).unwrap_or("");
        warnings.push(if hint.is_empty() {
            "결제수단을 알 수 없어 비워 두었습니다".to_string()
        } else {
            format!("결제수단 '{hint}'에 해당하는 카드를 찾지 못했습니다 (카드 별칭/끝 4자리를 등록하세요)")
        });
    }
    let pm_id = pm.map(|p| p.id);

    let ledgers = ledger::Entity::find().of_user(uid).all(db).await?;
    let default_ledger = ledgers
        .iter()
        .find(|l| l.is_default)
        .or_else(|| ledgers.iter().min_by_key(|l| (l.sort_order, l.id)))
        .ok_or_else(|| IngestError::Invalid("장부가 없습니다".into()))?;
    let explicit_ledger = match non_empty(input.ledger.as_deref()) {
        Some(name) => {
            let found = ledgers.iter().find(|l| norm(&l.name) == norm(name));
            if found.is_none() {
                warnings.push(format!("장부 '{name}'를 찾지 못해 기본 장부를 썼습니다"));
            }
            found
        }
        None => None,
    };

    let suggestion = suggest(db, uid, &description, pm_id).await?;
    let categories = category::Entity::find().of_user(uid).all(db).await?;
    let explicit_category = match non_empty(input.category.as_deref()) {
        Some(name) => {
            // 장부를 지정했으면 그 장부의 카테고리만, 아니면 기본 장부의 카테고리를 먼저 찾는다.
            let mut matches: Vec<&category::Model> = categories
                .iter()
                .filter(|c| norm(&c.name) == norm(name))
                .filter(|c| explicit_ledger.is_none_or(|l| c.ledger_id == l.id))
                .collect();
            matches.sort_by_key(|c| (c.ledger_id != default_ledger.id, c.archived, c.id));
            if matches.is_empty() {
                warnings.push(match explicit_ledger {
                    Some(l) => format!("'{}' 장부에 '{name}' 카테고리가 없습니다", l.name),
                    None => format!("카테고리 '{name}'를 찾지 못했습니다"),
                });
            }
            matches.first().copied()
        }
        None => None,
    };
    let (ledger_id, category_id) = match (explicit_ledger, explicit_category, &suggestion) {
        (_, Some(c), _) => (c.ledger_id, Some(c.id)),
        (Some(l), None, Some(s)) if s.ledger_id == l.id => (l.id, s.category_id),
        (Some(l), None, _) => (l.id, None),
        (None, None, Some(s)) => (s.ledger_id, s.category_id),
        (None, None, None) => (default_ledger.id, None),
    };
    let member = non_empty(input.member.as_deref()).map(str::to_string);
    let planned = input.planned.unwrap_or(false);
    let now = Utc::now();

    if !planned && !cancel
        && let Some(pid) = pm_id
            && let Some(found) = find_planned_match(db, uid, pid, amount, date, &description).await? {
                let mut memo = found.memo.clone().unwrap_or_default();
                if !similar(&found.description, &description) {
                    let note = format!("자동입력: {description}");
                    memo = if memo.is_empty() { note } else { format!("{memo} / {note}") };
                }
                if found.amount != amount {
                    warnings.push(format!("예정 금액 {}원을 실제 금액 {}원으로 바꿨습니다", found.amount, amount));
                }
                let mut active: transaction::ActiveModel = found.into();
                active.status = Set(TxStatus::Confirmed);
                active.date = Set(date);
                active.amount = Set(amount);
                active.installment_months = Set(installment_months);
                active.source = Set(TxSource::Api);
                active.external_id = Set(external_id.clone());
                active.raw_text = Set(raw_text);
                active.memo = Set(non_empty(Some(&memo)).map(str::to_string));
                if member.is_some() {
                    active.member = Set(member.clone());
                }
                active.updated_at = Set(now);
                let updated = active.update(db).await?;
                return Ok(IngestOutcome {
                    transaction: updated,
                    created: false,
                    duplicate: false,
                    matched_planned: true,
                    parsed,
                    warnings,
                });
            }

    let saved = transaction::ActiveModel {
        user_id: Set(uid),
        ledger_id: Set(ledger_id),
        date: Set(date),
        amount: Set(amount),
        category_id: Set(category_id),
        payment_method_id: Set(pm_id),
        description: Set(description.chars().take(200).collect()),
        memo: Set(non_empty(input.memo.as_deref()).map(str::to_string)),
        member: Set(member.or_else(|| suggestion.as_ref().and_then(|s| s.member.clone()))),
        status: Set(if planned { TxStatus::Planned } else { TxStatus::Confirmed }),
        installment_months: Set(installment_months),
        exclude_from_performance: Set(false),
        recurring_rule_id: Set(None),
        recurring_month: Set(None),
        source: Set(TxSource::Api),
        external_id: Set(external_id),
        raw_text: Set(raw_text),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    // 가족카드는 본인 카드의 혜택을 쓴다
    if let (Some(s), Some(card)) = (&suggestion, pm.map(|p| p.benefit_card_id())) {
        let valid: Vec<i32> = crate::entity::card_benefit::Entity::find()
            .filter(crate::entity::card_benefit::Column::PaymentMethodId.eq(card))
            .filter(crate::entity::card_benefit::Column::Id.is_in(s.benefit_ids.clone()))
            .all(db)
            .await?
            .into_iter()
            .map(|b| b.id)
            .collect();
        for bid in valid {
            transaction_benefit::ActiveModel { transaction_id: Set(saved.id), card_benefit_id: Set(bid) }
                .insert(db)
                .await?;
        }
    }

    Ok(IngestOutcome { transaction: saved, created: true, duplicate: false, matched_planned: false, parsed, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::connect_in_memory().await;
        let now = Utc::now();
        let ledger = ledger::ActiveModel {
            user_id: Set(1),
            name: Set("가계부".into()),
            description: Set(None),
            is_default: Set(true),
            sort_order: Set(0),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for (name, kind) in [("외식", CategoryKind::Expense), ("비용", CategoryKind::Expense)] {
            category::ActiveModel {
                user_id: Set(1),
                ledger_id: Set(ledger.id),
                name: Set(name.into()),
                kind: Set(kind),
                monthly_budget: Set(None),
                is_fixed: Set(false),
                color: Set(None),
                sort_order: Set(0),
                archived: Set(false),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        for (name, issuer, last4, aliases) in [
            ("알뜰교통신한", "신한", Some("1234"), Some("알뜰교통 신한카드")),
            ("탄탄대로biz", "KB국민", None, None),
            ("스카이패스", "KB국민", Some("9876"), None),
        ] {
            payment_method::ActiveModel {
                user_id: Set(1),
                name: Set(name.into()),
                kind: Set(PaymentKind::CreditCard),
                issuer: Set(Some(issuer.into())),
                product_name: Set(None),
                last4: Set(last4.map(str::to_string)),
                aliases: Set(aliases.map(str::to_string)),
                account_id: Set(None),
                payment_day: Set(None),
                closing_day: Set(None),
                closing_month_offset: Set(None),
                monthly_target: Set(None),
                annual_fee: Set(None),
                expires_on: Set(None),
                color: Set(None),
                memo: Set(None),
                sort_order: Set(0),
                archived: Set(false),
                created_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        db
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()
    }

    async fn insert_tx(db: &sea_orm::DatabaseConnection, desc: &str, amount: i64, cat: i32, pm: i32, status: TxStatus, date: NaiveDate) -> transaction::Model {
        let now = Utc::now();
        transaction::ActiveModel {
            user_id: Set(1),
            ledger_id: Set(1),
            date: Set(date),
            amount: Set(amount),
            category_id: Set(Some(cat)),
            payment_method_id: Set(Some(pm)),
            description: Set(desc.into()),
            memo: Set(None),
            member: Set(None),
            status: Set(status),
            installment_months: Set(1),
            exclude_from_performance: Set(false),
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
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn sms_ingest_categorizes_and_dedupes() {
        let db = setup().await;
        insert_tx(&db, "스타벅스", 5_000, 1, 1, TxStatus::Confirmed, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()).await;
        let sms = "[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원\n";
        let out = ingest(&db, 1, IngestInput { text: Some(sms.into()), ..Default::default() }, today()).await.unwrap();
        assert!(out.created);
        assert_eq!(out.transaction.raw_text.as_deref(), Some(sms), "원문은 줄바꿈까지 그대로");
        assert_eq!(out.transaction.amount, 12_500);
        assert_eq!(out.transaction.payment_method_id, Some(1));
        assert_eq!(out.transaction.category_id, Some(1));
        assert_eq!(out.transaction.description, "스타벅스");
        let again = ingest(&db, 1, IngestInput { text: Some(sms.into()), ..Default::default() }, today()).await.unwrap();
        assert!(again.duplicate);
        assert_eq!(again.transaction.id, out.transaction.id);
    }

    #[tokio::test]
    async fn json_ingest_confirms_planned() {
        let db = setup().await;
        let planned = insert_tx(&db, "아파트관리비", 350_000, 2, 1, TxStatus::Planned, NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()).await;
        let out = ingest(
            &db,
            1,
            IngestInput {
                amount: Some(302_590),
                description: Some("위례아파트관리비".into()),
                payment_method: Some("알뜰교통 신한카드".into()),
                date: NaiveDate::from_ymd_opt(2026, 10, 24),
                ..Default::default()
            },
            today(),
        )
        .await
        .unwrap();
        assert!(out.matched_planned, "{:?}", out.warnings);
        assert_eq!(out.transaction.id, planned.id);
        assert_eq!(out.transaction.status, TxStatus::Confirmed);
        assert_eq!(out.transaction.amount, 302_590);
        // text 없이 필드만 보내면 받은 필드를 JSON으로 남긴다 (보내지 않은 필드는 빼고)
        let raw: serde_json::Value = serde_json::from_str(out.transaction.raw_text.as_deref().unwrap()).unwrap();
        assert_eq!(
            raw,
            serde_json::json!({
                "amount": 302_590,
                "description": "위례아파트관리비",
                "paymentMethod": "알뜰교통 신한카드",
                "date": "2026-10-24",
            })
        );
    }

    #[test]
    fn rule_amount_range_and_keyword_are_filters() {
        let planned = (350_000, "(25)아파트관리비");
        let rule = RuleMatch::new;
        // 조건 없는 예정(규칙 없음 또는 규칙에 조건 없음): 금액이 같거나 내용이 비슷하면
        for r in [None, Some(&rule(Some(" , "), None, None))] {
            assert_eq!(match_rank(planned, r, 350_000, "위례아파트관리비"), Some(0));
            assert_eq!(match_rank(planned, r, 302_590, "위례아파트관리비"), Some(1));
            assert_eq!(match_rank(planned, r, 350_000, "스타벅스"), Some(2));
            assert_eq!(match_rank(planned, r, 302_590, "스타벅스"), None);
        }
        // 시작 = 끝: 그 금액일 때만
        let exact = rule(None, Some(350_000), Some(350_000));
        assert_eq!(match_rank(planned, Some(&exact), 350_000, "스타벅스"), Some(2));
        assert_eq!(match_rank(planned, Some(&exact), 350_001, "위례아파트관리비"), None);
        // 범위: 양끝 포함, 범위 안이면 금액이 맞는 것으로 본다
        let range = rule(None, Some(300_000), Some(400_000));
        assert_eq!(match_rank(planned, Some(&range), 300_000, "위례아파트관리비"), Some(0));
        assert_eq!(match_rank(planned, Some(&range), 400_000, "스타벅스"), Some(2));
        assert_eq!(match_rank(planned, Some(&range), 299_999, "위례아파트관리비"), None);
        // 한쪽만 정한 범위
        let at_least = rule(None, Some(300_000), None);
        assert_eq!(match_rank(planned, Some(&at_least), 9_000_000, "관리비"), Some(0));
        assert_eq!(match_rank(planned, Some(&at_least), 200_000, "관리비"), None);
        // 키워드: 가맹점에 없으면 금액이 같아도 안 맞고, 있으면 금액이 달라도 맞는다 (쉼표로 여러 개, 공백·대소문자 무시)
        let kw = rule(Some("관리 비, LH"), None, None);
        assert_eq!(match_rank(planned, Some(&kw), 350_000, "스타벅스"), None);
        assert_eq!(match_rank(planned, Some(&kw), 123_456, "lh공사"), Some(1));
        // 키워드 + 범위: 둘 다 맞아야
        let both = rule(Some("관리비"), Some(300_000), Some(400_000));
        assert_eq!(match_rank(planned, Some(&both), 320_000, "위례아파트관리비"), Some(0));
        assert_eq!(match_rank(planned, Some(&both), 320_000, "스타벅스"), None);
        assert_eq!(match_rank(planned, Some(&both), 420_000, "위례아파트관리비"), None);
    }

    #[tokio::test]
    async fn exact_amount_rule_confirms_only_that_amount() {
        let db = setup().await;
        let now = Utc::now();
        let rule = recurring_rule::ActiveModel {
            user_id: Set(1),
            ledger_id: Set(1),
            description: Set("아파트관리비".into()),
            amount: Set(350_000),
            category_id: Set(Some(2)),
            payment_method_id: Set(Some(1)),
            day_of_month: Set(25),
            interval_months: Set(1),
            start_month: Set("2026-10".into()),
            end_month: Set(None),
            member: Set(None),
            memo: Set(None),
            match_keyword: Set(None),
            match_amount_min: Set(Some(350_000)),
            match_amount_max: Set(Some(350_000)),
            exclude_from_performance: Set(false),
            transfer_ledger_id: Set(None),
            active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let planned = insert_tx(&db, "아파트관리비", 350_000, 2, 1, TxStatus::Planned, NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()).await;
        let mut active: transaction::ActiveModel = planned.clone().into();
        active.recurring_rule_id = Set(Some(rule.id));
        active.recurring_month = Set(Some("2026-10".into()));
        active.update(&db).await.unwrap();
        let send = |amount: i64| IngestInput {
            amount: Some(amount),
            description: Some("위례아파트관리비".into()),
            payment_method: Some("알뜰교통신한".into()),
            date: NaiveDate::from_ymd_opt(2026, 10, 24),
            ..Default::default()
        };
        // 내용이 비슷해도 금액이 다르면 새 거래로 저장하고 예정은 그대로 둔다
        let other = ingest(&db, 1, send(302_590), today()).await.unwrap();
        assert!(other.created && !other.matched_planned);
        assert_ne!(other.transaction.id, planned.id);
        let exact = ingest(&db, 1, send(350_000), today()).await.unwrap();
        assert!(exact.matched_planned, "{:?}", exact.warnings);
        assert_eq!(exact.transaction.id, planned.id);
        assert_eq!(exact.transaction.status, TxStatus::Confirmed);
    }

    #[tokio::test]
    async fn ambiguous_issuer_leaves_payment_method_empty() {
        let db = setup().await;
        let out = ingest(
            &db,
            1,
            IngestInput { text: Some("KB국민카드 승인 홍*동님 9,000원 일시불 10/23 09:00 김밥천국".into()), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(out.transaction.payment_method_id, None);
        assert!(!out.warnings.is_empty());
        let by_last4 = ingest(
            &db,
            1,
            IngestInput { text: Some("KB국민카드9876승인 홍*동님 9,000원 일시불 10/23 09:10 김밥천국".into()), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(by_last4.transaction.payment_method_id, Some(3));
    }

    #[tokio::test]
    async fn issuer_only_prefers_parent_over_family_card() {
        let db = setup().await;
        // 신한은 카드 하나뿐이었는데 가족카드를 더해도 끝자리 없는 문자는 본인 카드로
        let family = payment_method::ActiveModel {
            user_id: Set(1),
            name: Set("알뜰교통신한 가족".into()),
            kind: Set(PaymentKind::CreditCard),
            issuer: Set(Some("신한".into())),
            last4: Set(Some("5555".into())),
            sort_order: Set(0),
            archived: Set(false),
            created_at: Set(Utc::now()),
            parent_id: Set(Some(1)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let send = |text: &str| IngestInput { text: Some(text.into()), ..Default::default() };
        let out = ingest(&db, 1, send("신한카드 승인 홍*동님 9,000원 일시불 10/23 09:00 김밥천국"), today()).await.unwrap();
        assert_eq!(out.transaction.payment_method_id, Some(1), "{:?}", out.warnings);
        let by_last4 = ingest(&db, 1, send("신한카드(5555)승인 홍*동님 9,000원 일시불 10/23 09:10 김밥천국"), today()).await.unwrap();
        assert_eq!(by_last4.transaction.payment_method_id, Some(family.id), "{:?}", by_last4.warnings);
    }

    #[tokio::test]
    async fn category_must_belong_to_given_ledger() {
        let db = setup().await;
        let now = Utc::now();
        let allowance = ledger::ActiveModel {
            user_id: Set(1),
            name: Set("용돈".into()),
            description: Set(None),
            is_default: Set(false),
            sort_order: Set(1),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let out = ingest(
            &db,
            1,
            IngestInput {
                amount: Some(9_000),
                description: Some("분식".into()),
                ledger: Some("용돈".into()),
                category: Some("외식".into()),
                ..Default::default()
            },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(out.transaction.ledger_id, allowance.id);
        assert_eq!(out.transaction.category_id, None);
        assert!(out.warnings.iter().any(|w| w.contains("용돈")), "{:?}", out.warnings);
    }

    #[tokio::test]
    async fn cancel_is_negative() {
        let db = setup().await;
        let out = ingest(
            &db,
            1,
            IngestInput { amount: Some(12_000), description: Some("쿠팡".into()), cancel: Some(true), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(out.transaction.amount, -12_000);
        assert!(ingest(&db, 1, IngestInput::default(), today()).await.is_err());
    }

    #[tokio::test]
    async fn options_list_active_names_default_ledger_first() {
        let db = setup().await;
        let allowance = ledger::ActiveModel {
            user_id: Set(1),
            name: Set("용돈".into()),
            description: Set(None),
            is_default: Set(false),
            sort_order: Set(-1),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for (name, kind, archived) in
            [("월급", CategoryKind::Income, false), ("간식", CategoryKind::Expense, false), ("옛날", CategoryKind::Expense, true)]
        {
            category::ActiveModel {
                user_id: Set(1),
                ledger_id: Set(if name == "월급" { 1 } else { allowance.id }),
                name: Set(name.into()),
                kind: Set(kind),
                monthly_budget: Set(None),
                is_fixed: Set(false),
                color: Set(None),
                sort_order: Set(-5),
                archived: Set(archived),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let mut sky: payment_method::ActiveModel =
            payment_method::Entity::find().filter(payment_method::Column::Name.eq("스카이패스")).one(&db).await.unwrap().unwrap().into();
        sky.archived = Set(true);
        sky.update(&db).await.unwrap();

        let o = options(&db, 1).await.unwrap();
        // 기본 장부가 sort_order보다 먼저
        assert_eq!(o.ledgers.iter().map(|l| (l.name.as_str(), l.is_default)).collect::<Vec<_>>(), [("가계부", true), ("용돈", false)]);
        // 장부 순서 → 지출 먼저 → sort_order, 보관한 것은 빠짐
        let cats: Vec<_> = o.categories.iter().map(|c| (c.ledger.as_str(), c.name.as_str())).collect();
        assert_eq!(cats, [("가계부", "외식"), ("가계부", "비용"), ("가계부", "월급"), ("용돈", "간식")]);
        let pms: Vec<_> = o.payment_methods.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(pms, ["알뜰교통신한", "탄탄대로biz"]);
        assert_eq!(o.payment_methods[0].aliases, ["알뜰교통 신한카드"]);
        assert_eq!(o.payment_methods[0].last4.as_deref(), Some("1234"));
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["paymentMethods"][0]["kind"], "CREDIT_CARD");
        assert_eq!(json["categories"][2]["kind"], "INCOME");
    }
}
