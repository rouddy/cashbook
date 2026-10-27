//! 자동 입력: 아이폰 단축어, 안드로이드 문자 전달 앱, 스크립트 등에서 결제 내역을 보내면 거래로 저장한다.
//!
//! 처리 순서
//! 1. 원문(`text`)이 있으면 카드 승인 문자로 보고 금액/가맹점/일시/할부/취소/카드를 뽑는다. 명시 필드가 우선한다.
//! 2. `externalId`(없으면 원문 해시)로 중복을 막는다.
//! 3. 결제수단: 이름·별칭 → 카드번호 끝 4자리 → 원문에 나온 카드 이름 → 카드사(해당 카드사 카드가 하나일 때)
//! 4. 장부/카테고리: 명시값 → 같은 가맹점의 과거 거래에서 가장 많이 쓴 분류 → 기본 장부
//! 5. 같은 결제수단의 ±7일 예정 거래와 금액(또는 내용/키워드)이 맞으면 그 예정 거래를 확정 처리
//! 6. 아니면 새 확정 거래를 만든다.

use std::collections::HashMap;
use std::fmt;

use async_graphql::{InputObject, SimpleObject};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, sea_query::Expr,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::sms::{self, ParsedMessage};
use crate::entity::{
    category,
    enums::{TxSource, TxStatus},
    ledger, payment_method, recurring_rule, transaction, transaction_benefit,
};

#[derive(InputObject, Deserialize, Clone, Debug, Default)]
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

/// 같은(또는 서로 포함하는) 내용의 최근 확정 거래로 장부/카테고리를 추천한다.
pub async fn suggest<C: ConnectionTrait>(
    db: &C,
    description: &str,
    payment_method_id: Option<i32>,
) -> Result<Option<Suggestion>, DbErr> {
    let d = description.trim();
    if d.chars().count() < 2 {
        return Ok(None);
    }
    let rows = transaction::Entity::find()
        .filter(transaction::Column::Status.eq(TxStatus::Confirmed))
        .filter(
            Condition::any()
                .add(transaction::Column::Description.eq(d))
                .add(Expr::cust_with_values("instr(description, ?) > 0", [d]))
                .add(Expr::cust_with_values("(length(description) >= 2 AND instr(?, description) > 0)", [d])),
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
        Some(src) => transaction_benefit::Entity::find()
            .filter(transaction_benefit::Column::TransactionId.eq(src.id))
            .all(db)
            .await?
            .into_iter()
            .map(|b| b.card_benefit_id)
            .collect(),
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

async fn find_planned_match<C: ConnectionTrait>(
    db: &C,
    payment_method_id: i32,
    amount: i64,
    date: NaiveDate,
    description: &str,
) -> Result<Option<transaction::Model>, DbErr> {
    let candidates = transaction::Entity::find()
        .filter(transaction::Column::Status.eq(TxStatus::Planned))
        .filter(transaction::Column::PaymentMethodId.eq(payment_method_id))
        .filter(transaction::Column::Date.between(date - Duration::days(7), date + Duration::days(7)))
        .all(db)
        .await?;
    if candidates.is_empty() {
        return Ok(None);
    }
    let rule_ids: Vec<i32> = candidates.iter().filter_map(|c| c.recurring_rule_id).collect();
    let keywords: HashMap<i32, String> = if rule_ids.is_empty() {
        HashMap::new()
    } else {
        recurring_rule::Entity::find()
            .filter(recurring_rule::Column::Id.is_in(rule_ids))
            .all(db)
            .await?
            .into_iter()
            .filter_map(|r| Some((r.id, r.match_keyword.filter(|k| !k.trim().is_empty())?)))
            .collect()
    };
    let desc = norm(description);
    let best = candidates
        .into_iter()
        .filter_map(|c| {
            let amount_match = c.amount == amount;
            let keyword_match = c
                .recurring_rule_id
                .and_then(|id| keywords.get(&id))
                .is_some_and(|k| k.split(',').map(norm).any(|k| !k.is_empty() && desc.contains(&k)));
            let text_match = similar(&c.description, description);
            if !(amount_match || keyword_match || text_match) {
                return None;
            }
            let rank = if amount_match && (keyword_match || text_match) {
                0
            } else if keyword_match || text_match {
                1
            } else {
                2
            };
            Some(((rank, (c.date - date).num_days().abs(), c.id), c))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, c)| c);
    Ok(best)
}

fn hash_text(text: &str) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    format!("text:{}", &hex::encode(digest)[..32])
}

pub async fn ingest<C: ConnectionTrait>(db: &C, input: IngestInput, today: NaiveDate) -> Result<IngestOutcome, IngestError> {
    let mut warnings = Vec::new();
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

    let pms = payment_method::Entity::find().all(db).await?;
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

    let ledgers = ledger::Entity::find().all(db).await?;
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

    let suggestion = suggest(db, &description, pm_id).await?;
    let categories = category::Entity::find().all(db).await?;
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
            && let Some(found) = find_planned_match(db, pid, amount, date, &description).await? {
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
                active.raw_text = Set(text.clone());
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
        raw_text: Set(text),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    if let (Some(s), Some(pid)) = (&suggestion, pm_id) {
        let valid: Vec<i32> = crate::entity::card_benefit::Entity::find()
            .filter(crate::entity::card_benefit::Column::PaymentMethodId.eq(pid))
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
    use crate::entity::enums::{CategoryKind, PaymentKind};

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::connect_in_memory().await;
        let now = Utc::now();
        let ledger = ledger::ActiveModel {
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
                ledger_id: Set(ledger.id),
                name: Set(name.into()),
                kind: Set(kind),
                monthly_budget: Set(None),
                is_fixed: Set(false),
                excluded_from_spending: Set(false),
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
        let sms = "[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원";
        let out = ingest(&db, IngestInput { text: Some(sms.into()), ..Default::default() }, today()).await.unwrap();
        assert!(out.created);
        assert_eq!(out.transaction.amount, 12_500);
        assert_eq!(out.transaction.payment_method_id, Some(1));
        assert_eq!(out.transaction.category_id, Some(1));
        assert_eq!(out.transaction.description, "스타벅스");
        let again = ingest(&db, IngestInput { text: Some(sms.into()), ..Default::default() }, today()).await.unwrap();
        assert!(again.duplicate);
        assert_eq!(again.transaction.id, out.transaction.id);
    }

    #[tokio::test]
    async fn json_ingest_confirms_planned() {
        let db = setup().await;
        let planned = insert_tx(&db, "아파트관리비", 350_000, 2, 1, TxStatus::Planned, NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()).await;
        let out = ingest(
            &db,
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
    }

    #[tokio::test]
    async fn ambiguous_issuer_leaves_payment_method_empty() {
        let db = setup().await;
        let out = ingest(
            &db,
            IngestInput { text: Some("KB국민카드 승인 홍*동님 9,000원 일시불 10/23 09:00 김밥천국".into()), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(out.transaction.payment_method_id, None);
        assert!(!out.warnings.is_empty());
        let by_last4 = ingest(
            &db,
            IngestInput { text: Some("KB국민카드9876승인 홍*동님 9,000원 일시불 10/23 09:10 김밥천국".into()), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(by_last4.transaction.payment_method_id, Some(3));
    }

    #[tokio::test]
    async fn category_must_belong_to_given_ledger() {
        let db = setup().await;
        let now = Utc::now();
        let allowance = ledger::ActiveModel {
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
            IngestInput { amount: Some(12_000), description: Some("쿠팡".into()), cancel: Some(true), ..Default::default() },
            today(),
        )
        .await
        .unwrap();
        assert_eq!(out.transaction.amount, -12_000);
        assert!(ingest(&db, IngestInput::default(), today()).await.is_err());
    }
}
