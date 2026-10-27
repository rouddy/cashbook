//! 여유자금 = 은행 잔고 − 아직 잔고에 반영되지 않은 카드대금/계좌 출금.
//!
//! 잔고는 입력한 날(`balance_date`) 기준이므로,
//! - 신용카드: 결제일이 잔고 기준일 이후인 청구서 금액(이용기간이 진행 중인 청구서 포함)을 뺀다.
//!   결제 계좌가 연결되어 있으면 그 계좌의 기준일, 아니면 포함된 계좌 중 가장 최근 기준일을 쓴다.
//! - 체크카드/계좌이체/현금(연결 계좌가 있는 경우): 계좌 기준일 이후의 확정 거래를 뺀다.
//! - 이번 달까지의 예정 거래는 "예정 반영 후" 금액에만 반영한다.

use std::collections::{BTreeSet, HashMap};

use async_graphql::SimpleObject;
use chrono::{Duration, NaiveDate};

use super::billing::{BillingCycle, Charge, ChargePart, charge_parts};
use super::dates::YearMonth;
use super::summary::outflow;
use crate::entity::{
    account, category,
    enums::{PaymentKind, TxStatus},
    payment_method, transaction,
};

/// 청구서 한 장.
#[derive(SimpleObject, Clone, Debug, PartialEq, Eq)]
pub struct StatementInfo {
    /// 결제월
    pub month: YearMonth,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
    pub confirmed_amount: i64,
    pub planned_amount: i64,
    /// 이용기간이 끝났는지
    pub closed: bool,
    /// 결제일이 지났는지
    pub paid: bool,
}

#[derive(SimpleObject, Clone, Debug)]
#[graphql(complex)]
pub struct CardDue {
    pub payment_method_id: i32,
    pub billing_configured: bool,
    /// 결제 계좌가 여유자금 계산에 포함되는지. 제외된 계좌에서 나가는 카드대금은 합계에서 빼지 않는다.
    pub counted: bool,
    /// 이 날짜 이후 결제분을 미반영 카드대금으로 본다
    pub cutoff_date: NaiveDate,
    /// 확정 거래 기준 미반영 카드대금
    pub due: i64,
    /// 이번 달까지의 예정 거래로 추가될 카드대금
    pub due_planned: i64,
    pub next_payment: Option<StatementInfo>,
    /// 결제일이 기준일 이후인 청구서들
    pub statements: Vec<StatementInfo>,
    /// 정산 일정이 없는 카드의 최근 60일 사용액 (참고용, 합계에는 포함하지 않음)
    pub recent_spending: i64,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct AvailableFunds {
    pub today: NaiveDate,
    pub accounts_total: i64,
    /// 포함된 계좌 중 가장 최근 잔고 기준일
    pub balance_date: Option<NaiveDate>,
    pub card_due: i64,
    pub card_due_planned: i64,
    /// 잔고 기준일 이후의 계좌 출금(체크카드·계좌이체 등)
    pub unreflected_outflow: i64,
    /// 이번 달까지 남은 예정 지출 (신용카드 제외)
    pub planned_outflow: i64,
    /// 이번 달까지 남은 예정 수입
    pub planned_inflow: i64,
    /// 현재 여유자금
    pub available: i64,
    /// 이번 달 예정 거래까지 반영한 여유자금
    pub available_after_planned: i64,
    pub cards: Vec<CardDue>,
    /// 정산 일정을 입력하지 않아 카드대금을 계산하지 못한 카드 (최근 사용 이력 있음)
    pub unconfigured_card_ids: Vec<i32>,
}

pub struct FundsInput<'a> {
    pub today: NaiveDate,
    pub accounts: &'a [account::Model],
    pub payment_methods: &'a [payment_method::Model],
    pub categories: &'a HashMap<i32, category::Model>,
    /// 신용카드 할부를 고려해 오늘로부터 1년 이상 이전부터의 거래
    pub transactions: &'a [transaction::Model],
}

fn card_charges(
    pm: &payment_method::Model,
    txs: &[&transaction::Model],
    categories: &HashMap<i32, category::Model>,
    planned_until: Option<NaiveDate>,
) -> Vec<Charge> {
    txs.iter()
        .filter(|t| t.payment_method_id == Some(pm.id))
        .filter(|t| match t.status {
            TxStatus::Confirmed => true,
            TxStatus::Planned => planned_until.is_none_or(|until| t.date <= until),
            TxStatus::Skipped => false,
        })
        .map(|t| Charge {
            tx_id: t.id,
            date: t.date,
            amount: outflow(t, categories),
            installment_months: t.installment_months,
            planned: t.status == TxStatus::Planned,
        })
        .collect()
}

fn statement_info(cycle: &BillingCycle, parts: &[ChargePart], m: YearMonth, today: NaiveDate) -> StatementInfo {
    let (start, end) = cycle.period(m);
    let payment_date = cycle.payment_date(m);
    let (mut confirmed, mut planned) = (0, 0);
    for p in parts.iter().filter(|p| p.statement == m) {
        if p.planned {
            planned += p.amount;
        } else {
            confirmed += p.amount;
        }
    }
    StatementInfo {
        month: m,
        period_start: start,
        period_end: end,
        payment_date,
        confirmed_amount: confirmed,
        planned_amount: planned,
        closed: end < today,
        paid: payment_date < today,
    }
}

/// 카드 한 장의 청구서 목록. 거래가 있는 결제월과 다음 결제월을 포함한다.
pub fn card_statements(
    pm: &payment_method::Model,
    cycle: &BillingCycle,
    txs: &[&transaction::Model],
    categories: &HashMap<i32, category::Model>,
    today: NaiveDate,
    planned_until: Option<NaiveDate>,
) -> Vec<StatementInfo> {
    let parts = charge_parts(cycle, &card_charges(pm, txs, categories, planned_until));
    let mut months: BTreeSet<YearMonth> = parts.iter().map(|p| p.statement).collect();
    months.insert(cycle.next_statement(today));
    months.into_iter().map(|m| statement_info(cycle, &parts, m, today)).collect()
}

/// 지정한 결제월 범위의 청구서 (거래가 없는 달도 포함).
pub fn statement_range(
    pm: &payment_method::Model,
    cycle: &BillingCycle,
    txs: &[&transaction::Model],
    categories: &HashMap<i32, category::Model>,
    today: NaiveDate,
    from: YearMonth,
    to: YearMonth,
) -> Vec<StatementInfo> {
    let parts = charge_parts(cycle, &card_charges(pm, txs, categories, None));
    from.range_inclusive(to).map(|m| statement_info(cycle, &parts, m, today)).collect()
}

/// 결제월 `month` 청구서에 들어가는 회차들.
pub fn statement_parts(
    pm: &payment_method::Model,
    cycle: &BillingCycle,
    txs: &[&transaction::Model],
    categories: &HashMap<i32, category::Model>,
    month: YearMonth,
) -> Vec<ChargePart> {
    charge_parts(cycle, &card_charges(pm, txs, categories, None))
        .into_iter()
        .filter(|p| p.statement == month)
        .collect()
}

pub fn compute(input: &FundsInput) -> AvailableFunds {
    let today = input.today;
    let month_end = YearMonth::of(today).last_day();
    let included: HashMap<i32, &account::Model> = input
        .accounts
        .iter()
        .filter(|a| a.include_in_available && !a.archived)
        .map(|a| (a.id, a))
        .collect();
    let accounts_total = included.values().map(|a| a.balance).sum();
    let balance_date = included.values().filter_map(|a| a.balance_date).max();
    let default_cutoff = balance_date.unwrap_or(today);
    let pms: HashMap<i32, &payment_method::Model> = input.payment_methods.iter().map(|p| (p.id, p)).collect();

    let mut by_pm: HashMap<i32, Vec<&transaction::Model>> = HashMap::new();
    for t in input.transactions {
        if let Some(pid) = t.payment_method_id {
            by_pm.entry(pid).or_default().push(t);
        }
    }

    let mut cards = Vec::new();
    let mut unconfigured = Vec::new();
    let (mut card_due, mut card_due_planned) = (0i64, 0i64);
    // 출금 계좌를 지정하지 않은 결제수단은 여유자금 계좌에서 나간다고 본다.
    let counts = |pm: &payment_method::Model| pm.account_id.is_none_or(|id| included.contains_key(&id));
    let mut credit_cards: Vec<&payment_method::Model> =
        input.payment_methods.iter().filter(|p| p.kind == PaymentKind::CreditCard).collect();
    credit_cards.sort_by_key(|p| (p.archived, p.sort_order, p.id));
    for pm in credit_cards {
        let counted = counts(pm);
        let txs = by_pm.get(&pm.id).map(Vec::as_slice).unwrap_or(&[]);
        let cutoff = pm
            .account_id
            .and_then(|id| included.get(&id))
            .and_then(|a| a.balance_date)
            .unwrap_or(default_cutoff);
        match BillingCycle::from_model(pm) {
            Some(cycle) => {
                let all = card_statements(pm, &cycle, txs, input.categories, today, Some(month_end));
                let unpaid: Vec<StatementInfo> = all.into_iter().filter(|s| s.payment_date > cutoff).collect();
                let due: i64 = unpaid.iter().map(|s| s.confirmed_amount).sum();
                let due_planned: i64 = unpaid.iter().map(|s| s.planned_amount).sum();
                if pm.archived && due == 0 && due_planned == 0 {
                    continue;
                }
                let next = cycle.next_statement(today);
                if counted {
                    card_due += due;
                    card_due_planned += due_planned;
                }
                cards.push(CardDue {
                    payment_method_id: pm.id,
                    billing_configured: true,
                    counted,
                    cutoff_date: cutoff,
                    due,
                    due_planned,
                    next_payment: unpaid.iter().find(|s| s.month == next).cloned(),
                    statements: unpaid,
                    recent_spending: 0,
                });
            }
            None => {
                let since = today - Duration::days(60);
                let recent: i64 = txs
                    .iter()
                    .filter(|t| t.status == TxStatus::Confirmed && t.date >= since && t.date <= today)
                    .map(|t| outflow(t, input.categories))
                    .sum();
                let has_recent = txs.iter().any(|t| t.status != TxStatus::Skipped && t.date >= since);
                if pm.archived && !has_recent {
                    continue;
                }
                if has_recent && counted {
                    unconfigured.push(pm.id);
                }
                cards.push(CardDue {
                    payment_method_id: pm.id,
                    billing_configured: false,
                    counted,
                    cutoff_date: cutoff,
                    due: 0,
                    due_planned: 0,
                    next_payment: None,
                    statements: vec![],
                    recent_spending: recent,
                });
            }
        }
    }

    let (mut unreflected, mut planned_out, mut planned_in) = (0i64, 0i64, 0i64);
    for t in input.transactions {
        let pm = t.payment_method_id.and_then(|id| pms.get(&id));
        let is_credit = pm.is_some_and(|p| p.kind == PaymentKind::CreditCard);
        match t.status {
            TxStatus::Confirmed if !is_credit => {
                let Some(acct) = pm.and_then(|p| p.account_id).and_then(|id| included.get(&id)) else { continue };
                if acct.balance_date.is_some_and(|bd| t.date > bd) {
                    unreflected += outflow(t, input.categories);
                }
            }
            TxStatus::Planned if !is_credit && t.date <= month_end => {
                if pm.is_some_and(|p| !counts(p)) {
                    continue;
                }
                let out = outflow(t, input.categories);
                if out >= 0 {
                    planned_out += out;
                } else {
                    planned_in += -out;
                }
            }
            _ => {}
        }
    }

    let available = accounts_total - card_due - unreflected;
    AvailableFunds {
        today,
        accounts_total,
        balance_date,
        card_due,
        card_due_planned,
        unreflected_outflow: unreflected,
        planned_outflow: planned_out,
        planned_inflow: planned_in,
        available,
        available_after_planned: available - card_due_planned - planned_out + planned_in,
        cards,
        unconfigured_card_ids: unconfigured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::enums::{AccountKind, CategoryKind, TxSource};
    use chrono::Utc;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    fn acct(id: i32, balance: i64, date: Option<NaiveDate>) -> account::Model {
        account::Model {
            id,
            name: format!("a{id}"),
            bank: None,
            kind: AccountKind::Checking,
            account_number: None,
            balance,
            balance_date: date,
            include_in_available: true,
            interest_rate: None,
            maturity_date: None,
            memo: None,
            sort_order: 0,
            archived: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn pm(id: i32, kind: PaymentKind, account_id: Option<i32>, cycle: Option<(i32, i32, i32)>) -> payment_method::Model {
        payment_method::Model {
            id,
            name: format!("p{id}"),
            kind,
            issuer: None,
            product_name: None,
            last4: None,
            aliases: None,
            account_id,
            payment_day: cycle.map(|c| c.0),
            closing_day: cycle.map(|c| c.1),
            closing_month_offset: cycle.map(|c| c.2),
            monthly_target: None,
            annual_fee: None,
            expires_on: None,
            color: None,
            memo: None,
            sort_order: id,
            archived: false,
            created_at: Utc::now(),
        }
    }

    fn tx(id: i32, pm: i32, date: NaiveDate, amount: i64, status: TxStatus) -> transaction::Model {
        transaction::Model {
            id,
            ledger_id: 1,
            date,
            amount,
            category_id: Some(1),
            payment_method_id: Some(pm),
            description: String::new(),
            memo: None,
            member: None,
            status,
            installment_months: 1,
            exclude_from_performance: false,
            recurring_rule_id: None,
            recurring_month: None,
            source: TxSource::Manual,
            external_id: None,
            raw_text: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn available_funds() {
        let categories: HashMap<i32, category::Model> = [(
            1,
            category::Model {
                id: 1,
                ledger_id: 1,
                name: "외식".into(),
                kind: CategoryKind::Expense,
                monthly_budget: None,
                is_fixed: false,
                excluded_from_spending: false,
                color: None,
                sort_order: 0,
                archived: false,
            },
        )]
        .into_iter()
        .collect();
        // 잔고 기준일 10/20, 오늘 10/23
        let accounts = vec![acct(1, 5_000_000, Some(d(10, 20)))];
        let pms = vec![
            // 25일 결제, 당월 11일 마감 → 10월 청구서(9/12~10/11)는 10/26 출금
            pm(10, PaymentKind::CreditCard, Some(1), Some((25, 11, 0))),
            pm(11, PaymentKind::CheckCard, Some(1), None),
            pm(12, PaymentKind::CreditCard, None, None),
        ];
        let txs = vec![
            tx(1, 10, d(9, 15), 100_000, TxStatus::Confirmed), // 10/26 결제 → 미반영
            tx(2, 10, d(10, 15), 50_000, TxStatus::Confirmed), // 11/25 결제 → 미반영
            tx(3, 10, d(8, 20), 70_000, TxStatus::Confirmed),  // 9/25 결제 → 이미 반영
            tx(4, 10, d(10, 28), 30_000, TxStatus::Planned),   // 이번 달 예정 → 예정 반영
            tx(5, 10, d(11, 5), 99_000, TxStatus::Planned),    // 다음 달 예정 → 제외
            tx(6, 11, d(10, 21), 10_000, TxStatus::Confirmed), // 기준일 이후 체크카드
            tx(7, 11, d(10, 19), 20_000, TxStatus::Confirmed), // 기준일 이전 → 반영됨
            tx(8, 12, d(10, 1), 40_000, TxStatus::Confirmed),  // 정산일정 없는 카드
        ];
        let f = compute(&FundsInput {
            today: d(10, 23),
            accounts: &accounts,
            payment_methods: &pms,
            categories: &categories,
            transactions: &txs,
        });
        assert_eq!(f.accounts_total, 5_000_000);
        assert_eq!(f.card_due, 150_000);
        assert_eq!(f.card_due_planned, 30_000);
        assert_eq!(f.unreflected_outflow, 10_000);
        assert_eq!(f.available, 5_000_000 - 150_000 - 10_000);
        assert_eq!(f.available_after_planned, f.available - 30_000);
        assert_eq!(f.unconfigured_card_ids, vec![12]);
        let card = f.cards.iter().find(|c| c.payment_method_id == 10).unwrap();
        assert!(card.counted);
        let next = card.next_payment.as_ref().unwrap();
        assert_eq!((next.payment_date, next.confirmed_amount), (d(10, 26), 100_000));
        assert_eq!(card.statements.len(), 2);
    }

    #[test]
    fn cards_paid_from_excluded_accounts_are_not_subtracted() {
        let categories = HashMap::new();
        let mut savings = acct(2, 9_000_000, Some(d(10, 20)));
        savings.include_in_available = false;
        let accounts = vec![acct(1, 5_000_000, Some(d(10, 20))), savings];
        let pms = vec![pm(10, PaymentKind::CreditCard, Some(2), Some((25, 11, 0))), pm(11, PaymentKind::CheckCard, Some(2), None)];
        let txs = vec![
            tx(1, 10, d(9, 15), 100_000, TxStatus::Confirmed),
            tx(2, 11, d(10, 21), 10_000, TxStatus::Confirmed),
            tx(3, 11, d(10, 28), 20_000, TxStatus::Planned),
        ];
        let f = compute(&FundsInput { today: d(10, 23), accounts: &accounts, payment_methods: &pms, categories: &categories, transactions: &txs });
        assert_eq!(f.accounts_total, 5_000_000);
        assert_eq!((f.card_due, f.unreflected_outflow, f.planned_outflow), (0, 0, 0));
        assert_eq!(f.available, 5_000_000);
        let card = f.cards.iter().find(|c| c.payment_method_id == 10).unwrap();
        assert!(!card.counted);
        assert_eq!(card.due, 100_000);
    }
}
