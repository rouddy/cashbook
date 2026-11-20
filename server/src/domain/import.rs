//! 스프레드시트에서 변환한 정규화 JSON(`tools/export_sheets.py` 출력)을 DB에 넣는다.
//! `cashbook import <file.json> [--replace]`
//!
//! 시트의 '합계 제외' 카테고리와 월 수입·배분 계획은 DB 마이그레이션과 같은 규칙(`legacy_plan`)으로
//! 장부·정기수익 반복 수입·장부 이체 반복으로 바꿔 넣는다.

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, Set, TransactionSession, TransactionTrait,
};
use serde::Deserialize;

use super::dates::YearMonth;
use super::legacy_plan::{self, Conversion, LedgerInfo, LegacyItem, LegacyKind};
use crate::entity::{
    account, account_balance, card_benefit, card_tier, category,
    enums::{AccountKind, BenefitKind, CategoryKind, PaymentKind, PlanItemKind, TxSource, TxStatus},
    ledger, payment_method, plan_item, recurring_rule, transaction, transaction_benefit,
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportData {
    pub ledgers: Vec<ImportLedger>,
    pub categories: Vec<ImportCategory>,
    pub payment_methods: Vec<ImportPaymentMethod>,
    #[serde(default)]
    pub accounts: Vec<ImportAccount>,
    #[serde(default)]
    pub plan_items: Vec<ImportPlanItem>,
    #[serde(default)]
    pub recurring_rules: Vec<ImportRecurringRule>,
    pub transactions: Vec<ImportTransaction>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportLedger {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportCategory {
    pub ledger: String,
    pub name: String,
    pub kind: CategoryKind,
    pub monthly_budget: Option<i64>,
    #[serde(default)]
    pub is_fixed: bool,
    /// 시트에서 소비 합계에서 뺀 항목(경조사비·여행). 같은 이름의 장부로 옮긴다
    #[serde(default)]
    pub excluded_from_spending: bool,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportBenefit {
    pub name: String,
    #[serde(default)]
    pub kind: BenefitKind,
    pub rate: f64,
    pub min_amount: Option<i64>,
    pub per_tx_max: Option<i64>,
    pub monthly_cap: Option<i64>,
    #[serde(default = "default_true")]
    pub shares_card_limit: bool,
    #[serde(default)]
    pub excludes_performance: bool,
    #[serde(default)]
    pub stackable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportTier {
    pub min_spending: i64,
    pub benefit_limit: i64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportPaymentMethod {
    pub name: String,
    pub kind: PaymentKind,
    pub issuer: Option<String>,
    pub product_name: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub last4: Option<String>,
    pub expires_on: Option<String>,
    pub annual_fee: Option<i64>,
    pub monthly_target: Option<i64>,
    pub memo: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub benefits: Vec<ImportBenefit>,
    #[serde(default)]
    pub tiers: Vec<ImportTier>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportAccount {
    pub name: String,
    pub bank: Option<String>,
    pub kind: AccountKind,
    pub balance: i64,
    pub balance_date: Option<NaiveDate>,
    #[serde(default = "default_true")]
    pub include_in_available: bool,
    pub interest_rate: Option<f64>,
    pub memo: Option<String>,
}

/// 시트의 월 계획 항목 종류
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImportPlanKind {
    /// 월 수입 (월급) → 정기수익 반복 수입
    Income,
    /// 배분 (용돈·적립) → 맞는 장부가 있으면 장부 이체 반복
    Allocation,
    MemberAllowance,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlanItem {
    pub ledger: String,
    pub kind: ImportPlanKind,
    pub name: String,
    pub amount: i64,
    pub member: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecurringRule {
    pub key: String,
    pub ledger: String,
    pub description: String,
    pub amount: i64,
    pub category: Option<String>,
    pub payment_method: Option<String>,
    pub day_of_month: i32,
    pub start_month: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportTransaction {
    pub ledger: String,
    pub date: NaiveDate,
    pub amount: i64,
    pub category: Option<String>,
    pub payment_method: Option<String>,
    #[serde(default)]
    pub description: String,
    pub member: Option<String>,
    pub memo: Option<String>,
    pub status: TxStatus,
    pub recurring_rule: Option<String>,
    pub recurring_month: Option<String>,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default)]
    pub exclude_from_performance: bool,
}

#[derive(Debug, Default)]
pub struct ImportStats {
    pub ledgers: usize,
    pub categories: usize,
    pub payment_methods: usize,
    pub accounts: usize,
    pub plan_items: usize,
    pub recurring_rules: usize,
    pub transactions: usize,
    pub transaction_benefits: usize,
}

/// '합계 제외' 카테고리를 같은 이름의 장부로 옮긴다 (없으면 장부를 만든다). 그 항목의 거래·반복 규칙도 함께.
fn split_excluded_categories(data: &mut ImportData) {
    let excluded: Vec<(String, String)> =
        data.categories.iter().filter(|c| c.excluded_from_spending).map(|c| (c.ledger.clone(), c.name.clone())).collect();
    for (from, name) in excluded {
        let to = match data.ledgers.iter().find(|l| l.name == name) {
            Some(l) => l.key.clone(),
            None => {
                let key = format!("{from}/{name}");
                data.ledgers.push(ImportLedger { key: key.clone(), name: name.clone(), description: None, is_default: false });
                key
            }
        };
        if to == from {
            continue;
        }
        if data.categories.iter().any(|c| c.ledger == to && c.name == name) {
            data.categories.retain(|c| !(c.ledger == from && c.name == name));
        } else if let Some(c) = data.categories.iter_mut().find(|c| c.ledger == from && c.name == name) {
            c.ledger = to.clone();
        }
        let moves = |ledger: &String, category: &Option<String>| *ledger == from && category.as_deref() == Some(name.as_str());
        for t in data.transactions.iter_mut().filter(|t| moves(&t.ledger, &t.category)) {
            t.ledger = to.clone();
        }
        for r in data.recurring_rules.iter_mut().filter(|r| moves(&r.ledger, &r.category)) {
            r.ledger = to.clone();
        }
    }
}

fn missing(kind: &str, name: &str) -> DbErr {
    DbErr::Custom(format!("가져오기 데이터 오류: {kind} '{name}'를 찾을 수 없습니다"))
}

async fn clear_all<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    // FK 순서대로 지운다. API 토큰은 남긴다.
    transaction_benefit::Entity::delete_many().exec(db).await?;
    transaction::Entity::delete_many().exec(db).await?;
    recurring_rule::Entity::delete_many().exec(db).await?;
    plan_item::Entity::delete_many().exec(db).await?;
    card_tier::Entity::delete_many().exec(db).await?;
    card_benefit::Entity::delete_many().exec(db).await?;
    payment_method::Entity::delete_many().exec(db).await?;
    account_balance::Entity::delete_many().exec(db).await?;
    account::Entity::delete_many().exec(db).await?;
    category::Entity::delete_many().exec(db).await?;
    ledger::Entity::delete_many().exec(db).await?;
    Ok(())
}

/// `today`: 시트의 월 수입·배분을 옮긴 반복 규칙의 시작 월
pub async fn run<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    mut data: ImportData,
    replace: bool,
    today: NaiveDate,
) -> Result<ImportStats, DbErr> {
    let existing = transaction::Entity::find().count(db).await? + ledger::Entity::find().count(db).await?;
    if existing > 0 && !replace {
        return Err(DbErr::Custom(
            "DB에 이미 데이터가 있습니다. 기존 데이터를 지우고 가져오려면 --replace 를 붙이세요".into(),
        ));
    }
    split_excluded_categories(&mut data);
    let txn = db.begin().await?;
    if replace {
        clear_all(&txn).await?;
    }
    let now = Utc::now();
    let mut stats = ImportStats::default();

    let mut ledger_ids: HashMap<String, i32> = HashMap::new();
    for (i, l) in data.ledgers.iter().enumerate() {
        let saved = ledger::ActiveModel {
            name: Set(l.name.clone()),
            description: Set(l.description.clone()),
            is_default: Set(l.is_default),
            sort_order: Set(i as i32),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        ledger_ids.insert(l.key.clone(), saved.id);
        stats.ledgers += 1;
    }
    let ledger_id = |key: &str| ledger_ids.get(key).copied().ok_or_else(|| missing("장부", key));

    let mut category_ids: HashMap<(i32, String), i32> = HashMap::new();
    for c in &data.categories {
        let lid = ledger_id(&c.ledger)?;
        let saved = category::ActiveModel {
            ledger_id: Set(lid),
            name: Set(c.name.clone()),
            kind: Set(c.kind),
            monthly_budget: Set(c.monthly_budget),
            is_fixed: Set(c.is_fixed),
            color: Set(None),
            sort_order: Set(c.sort_order),
            archived: Set(false),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        category_ids.insert((lid, c.name.clone()), saved.id);
        stats.categories += 1;
    }

    // 시트의 월 수입·배분을 어떻게 옮길지 정하고, 정기수익 항목이 필요한 장부에는 만들어 둔다
    let ledger_infos: Vec<LedgerInfo> = data
        .ledgers
        .iter()
        .map(|l| Ok(LedgerInfo { id: ledger_id(&l.key)?, name: l.name.clone(), description: l.description.clone() }))
        .collect::<Result<_, DbErr>>()?;
    let legacy_items: Vec<LegacyItem> = data
        .plan_items
        .iter()
        .enumerate()
        .map(|(i, p)| {
            Ok(LegacyItem {
                id: i as i32,
                ledger_id: ledger_id(&p.ledger)?,
                kind: match p.kind {
                    ImportPlanKind::Income => LegacyKind::Income,
                    ImportPlanKind::Allocation => LegacyKind::Allocation,
                    ImportPlanKind::MemberAllowance => LegacyKind::Other,
                },
                name: p.name.clone(),
                amount: p.amount,
            })
        })
        .collect::<Result<_, DbErr>>()?;
    let conversions = legacy_plan::convert(&ledger_infos, &legacy_items);
    for (i, conversion) in &conversions {
        let lid = legacy_items[*i as usize].ledger_id;
        let key = (lid, legacy_plan::REGULAR_INCOME_CATEGORY.to_string());
        if *conversion != Conversion::RegularIncome || category_ids.contains_key(&key) {
            continue;
        }
        let order = data.categories.iter().filter(|c| ledger_ids.get(&c.ledger) == Some(&lid)).count();
        let saved = category::ActiveModel {
            ledger_id: Set(lid),
            name: Set(key.1.clone()),
            kind: Set(CategoryKind::Income),
            monthly_budget: Set(None),
            is_fixed: Set(false),
            color: Set(None),
            sort_order: Set(order as i32),
            archived: Set(false),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        category_ids.insert(key, saved.id);
        stats.categories += 1;
    }
    let category_id = |lid: i32, name: &Option<String>| -> Result<Option<i32>, DbErr> {
        match name {
            None => Ok(None),
            Some(n) => category_ids.get(&(lid, n.clone())).copied().map(Some).ok_or_else(|| missing("카테고리", n)),
        }
    };

    let mut pm_ids: HashMap<String, i32> = HashMap::new();
    let mut benefit_ids: HashMap<(i32, String), i32> = HashMap::new();
    for p in &data.payment_methods {
        let aliases = p.aliases.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect::<Vec<_>>().join(",");
        let saved = payment_method::ActiveModel {
            name: Set(p.name.clone()),
            kind: Set(p.kind),
            issuer: Set(p.issuer.clone()),
            product_name: Set(p.product_name.clone()),
            last4: Set(p.last4.clone()),
            aliases: Set((!aliases.is_empty()).then_some(aliases)),
            account_id: Set(None),
            payment_day: Set(None),
            closing_day: Set(None),
            closing_month_offset: Set(None),
            monthly_target: Set(p.monthly_target),
            annual_fee: Set(p.annual_fee),
            expires_on: Set(p.expires_on.clone()),
            color: Set(None),
            memo: Set(p.memo.clone()),
            sort_order: Set(p.sort_order),
            archived: Set(p.archived),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        for (i, b) in p.benefits.iter().enumerate() {
            let benefit = card_benefit::ActiveModel {
                payment_method_id: Set(saved.id),
                name: Set(b.name.clone()),
                kind: Set(b.kind),
                rate: Set(b.rate),
                min_amount: Set(b.min_amount),
                per_tx_max: Set(b.per_tx_max),
                monthly_cap: Set(b.monthly_cap),
                shares_card_limit: Set(b.shares_card_limit),
                excludes_performance: Set(b.excludes_performance),
                stackable: Set(b.stackable),
                sort_order: Set(i as i32),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
            benefit_ids.insert((saved.id, b.name.clone()), benefit.id);
        }
        for t in &p.tiers {
            card_tier::ActiveModel {
                payment_method_id: Set(saved.id),
                min_spending: Set(t.min_spending),
                benefit_limit: Set(t.benefit_limit),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        pm_ids.insert(p.name.clone(), saved.id);
        stats.payment_methods += 1;
    }
    let pm_id = |name: &Option<String>| -> Result<Option<i32>, DbErr> {
        match name {
            None => Ok(None),
            Some(n) => pm_ids.get(n).copied().map(Some).ok_or_else(|| missing("결제수단", n)),
        }
    };

    for (i, a) in data.accounts.iter().enumerate() {
        let saved = account::ActiveModel {
            name: Set(a.name.clone()),
            bank: Set(a.bank.clone()),
            kind: Set(a.kind),
            account_number: Set(None),
            balance: Set(a.balance),
            balance_date: Set(a.balance_date),
            include_in_available: Set(a.include_in_available),
            interest_rate: Set(a.interest_rate),
            maturity_date: Set(None),
            memo: Set(a.memo.clone()),
            sort_order: Set(i as i32),
            archived: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        if let Some(date) = a.balance_date {
            account_balance::ActiveModel {
                account_id: Set(saved.id),
                balance: Set(a.balance),
                balance_date: Set(date),
                memo: Set(Some("가져오기".into())),
                created_at: Set(now),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
        stats.accounts += 1;
    }

    let start_month = YearMonth::of(today).to_string();
    for (i, conversion) in conversions {
        let p = &data.plan_items[i as usize];
        let lid = legacy_items[i as usize].ledger_id;
        let (category, transfer, day) = match conversion {
            Conversion::ReplacedByTransfer => continue,
            Conversion::Keep => {
                let kind = match p.kind {
                    ImportPlanKind::MemberAllowance => PlanItemKind::MemberAllowance,
                    _ => PlanItemKind::Allocation,
                };
                plan_item::ActiveModel {
                    ledger_id: Set(lid),
                    kind: Set(kind),
                    name: Set(p.name.clone()),
                    amount: Set(p.amount),
                    member: Set(p.member.clone()),
                    sort_order: Set(p.sort_order),
                    ..Default::default()
                }
                .insert(&txn)
                .await?;
                stats.plan_items += 1;
                continue;
            }
            Conversion::Transfer { to } => (None, Some(to), legacy_plan::TRANSFER_DAY),
            Conversion::RegularIncome => {
                let cid = category_id(lid, &Some(legacy_plan::REGULAR_INCOME_CATEGORY.to_string()))?;
                (cid, None, legacy_plan::INCOME_DAY)
            }
        };
        recurring_rule::ActiveModel {
            ledger_id: Set(lid),
            description: Set(p.name.clone()),
            amount: Set(p.amount),
            category_id: Set(category),
            payment_method_id: Set(None),
            day_of_month: Set(day),
            interval_months: Set(1),
            start_month: Set(start_month.clone()),
            end_month: Set(None),
            member: Set(None),
            memo: Set(None),
            match_keyword: Set(None),
            exclude_from_performance: Set(false),
            transfer_ledger_id: Set(transfer),
            active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        stats.recurring_rules += 1;
    }

    let mut rule_ids: HashMap<String, i32> = HashMap::new();
    for r in &data.recurring_rules {
        let lid = ledger_id(&r.ledger)?;
        let saved = recurring_rule::ActiveModel {
            ledger_id: Set(lid),
            description: Set(r.description.clone()),
            amount: Set(r.amount),
            category_id: Set(category_id(lid, &r.category)?),
            payment_method_id: Set(pm_id(&r.payment_method)?),
            day_of_month: Set(r.day_of_month),
            interval_months: Set(1),
            start_month: Set(r.start_month.clone()),
            end_month: Set(None),
            member: Set(None),
            memo: Set(None),
            match_keyword: Set(None),
            exclude_from_performance: Set(false),
            active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        rule_ids.insert(r.key.clone(), saved.id);
        stats.recurring_rules += 1;
    }

    for t in &data.transactions {
        let lid = ledger_id(&t.ledger)?;
        let pid = pm_id(&t.payment_method)?;
        let saved = transaction::ActiveModel {
            ledger_id: Set(lid),
            date: Set(t.date),
            amount: Set(t.amount),
            category_id: Set(category_id(lid, &t.category)?),
            payment_method_id: Set(pid),
            description: Set(t.description.clone()),
            memo: Set(t.memo.clone()),
            member: Set(t.member.clone()),
            status: Set(t.status),
            installment_months: Set(1),
            exclude_from_performance: Set(t.exclude_from_performance),
            recurring_rule_id: Set(t.recurring_rule.as_ref().and_then(|k| rule_ids.get(k).copied())),
            recurring_month: Set(t.recurring_month.clone()),
            source: Set(TxSource::Import),
            external_id: Set(None),
            raw_text: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        stats.transactions += 1;
        if let Some(pid) = pid {
            for b in &t.benefits {
                if let Some(&bid) = benefit_ids.get(&(pid, b.clone())) {
                    transaction_benefit::ActiveModel { transaction_id: Set(saved.id), card_benefit_id: Set(bid) }
                        .insert(&txn)
                        .await?;
                    stats.transaction_benefits += 1;
                }
            }
        }
    }

    txn.commit().await?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ColumnTrait, QueryFilter, QueryOrder};

    #[tokio::test]
    async fn sheet_plan_becomes_ledgers_and_recurring_rules() {
        let db = crate::db::connect_in_memory().await;
        let data: ImportData = serde_json::from_value(serde_json::json!({
            "ledgers": [
                {"key": "household", "name": "가계부", "description": "월급사용내역 (생활비)", "isDefault": true},
                {"key": "allowance", "name": "용돈", "description": "재홍 용돈"}
            ],
            "categories": [
                {"ledger": "household", "name": "외식", "kind": "EXPENSE", "sortOrder": 0},
                {"ledger": "household", "name": "경조사비", "kind": "EXPENSE", "monthlyBudget": 250000, "excludedFromSpending": true, "sortOrder": 1},
                {"ledger": "allowance", "name": "점심", "kind": "EXPENSE", "sortOrder": 0}
            ],
            "paymentMethods": [],
            "planItems": [
                {"ledger": "household", "kind": "INCOME", "name": "월급", "amount": 4500000, "sortOrder": 0},
                {"ledger": "household", "kind": "ALLOCATION", "name": "재홍 용돈", "amount": 200000, "sortOrder": 1},
                {"ledger": "household", "kind": "MEMBER_ALLOWANCE", "name": "송화 용돈", "amount": 500000, "member": "송화", "sortOrder": 2},
                {"ledger": "household", "kind": "ALLOCATION", "name": "경조사비", "amount": 250000, "sortOrder": 3},
                {"ledger": "household", "kind": "ALLOCATION", "name": "저축", "amount": 100000, "sortOrder": 4},
                {"ledger": "allowance", "kind": "INCOME", "name": "용돈", "amount": 200000, "sortOrder": 0}
            ],
            "transactions": [
                {"ledger": "household", "date": "2026-02-18", "amount": 100000, "category": "경조사비", "description": "결혼식", "status": "CONFIRMED"}
            ]
        }))
        .unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        run(&db, data, false, today).await.unwrap();

        let ledgers = ledger::Entity::find().order_by_asc(ledger::Column::SortOrder).all(&db).await.unwrap();
        assert_eq!(ledgers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["가계부", "용돈", "경조사비"]);
        let gyeongjo = ledgers[2].id;
        let wedding = transaction::Entity::find().one(&db).await.unwrap().unwrap();
        let category = category::Entity::find_by_id(wedding.category_id.unwrap()).one(&db).await.unwrap().unwrap();
        assert_eq!((wedding.ledger_id, category.ledger_id, category.name.as_str()), (gyeongjo, gyeongjo, "경조사비"));

        let income = category::Entity::find().filter(category::Column::Name.eq("정기수익")).one(&db).await.unwrap().unwrap();
        assert_eq!((income.ledger_id, income.kind), (ledgers[0].id, CategoryKind::Income));
        let rules: Vec<(String, Option<i32>, Option<i32>, i32, String)> = recurring_rule::Entity::find()
            .order_by_asc(recurring_rule::Column::Id)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.description, r.category_id, r.transfer_ledger_id, r.day_of_month, r.start_month))
            .collect();
        assert_eq!(
            rules,
            vec![
                ("월급".into(), Some(income.id), None, 10, "2026-11".into()),
                ("재홍 용돈".into(), None, Some(ledgers[1].id), 1, "2026-11".into()),
                ("경조사비".into(), None, Some(gyeongjo), 1, "2026-11".into()),
            ]
        );
        let items: Vec<String> = plan_item::Entity::find().all(&db).await.unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(items, vec!["송화 용돈", "저축"]);
    }
}
