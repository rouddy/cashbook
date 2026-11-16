//! 스프레드시트에서 변환한 정규화 JSON(`tools/export_sheets.py` 출력)을 DB에 넣는다.
//! `cashbook import <file.json> [--replace]`

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, Set, TransactionSession, TransactionTrait,
};
use serde::Deserialize;

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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlanItem {
    pub ledger: String,
    pub kind: PlanItemKind,
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

pub async fn run<C: ConnectionTrait + TransactionTrait>(db: &C, data: ImportData, replace: bool) -> Result<ImportStats, DbErr> {
    let existing = transaction::Entity::find().count(db).await? + ledger::Entity::find().count(db).await?;
    if existing > 0 && !replace {
        return Err(DbErr::Custom(
            "DB에 이미 데이터가 있습니다. 기존 데이터를 지우고 가져오려면 --replace 를 붙이세요".into(),
        ));
    }
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
            excluded_from_spending: Set(c.excluded_from_spending),
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

    for p in &data.plan_items {
        plan_item::ActiveModel {
            ledger_id: Set(ledger_id(&p.ledger)?),
            kind: Set(p.kind),
            name: Set(p.name.clone()),
            amount: Set(p.amount),
            member: Set(p.member.clone()),
            sort_order: Set(p.sort_order),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        stats.plan_items += 1;
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
