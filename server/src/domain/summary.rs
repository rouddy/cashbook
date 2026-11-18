//! 월별 요약: 이번 달 지출/예정/평균 비교, 카테고리·결제수단·구성원별 합계, 지출계획 대비 월잔액.
//!
//! 스프레드시트의 월 시트 요약과 같은 기준을 쓴다.
//! - 소비(spending) = 지출 카테고리 합계. 단 `excluded_from_spending` 카테고리(경조사비·여행)는 별도 집계
//! - 월잔액 = 계획 수입 + 수입 카테고리 합계 − 소비 − 배분(용돈·저축·적금 등)
//! - 구성원 용돈(송화 용돈)은 월잔액에서 빼지 않고 구성원 표시 지출과 비교만 한다

use std::collections::{BTreeMap, HashMap};

use async_graphql::SimpleObject;
use chrono::{Datelike, NaiveDate};

use super::dates::YearMonth;
use crate::entity::{
    category,
    enums::{CategoryKind, PlanItemKind, TxStatus},
    payment_method, plan_item, transaction,
};

pub const UNCATEGORIZED: &str = "미분류";

/// 거래가 월 합계에서 어디에 속하는지.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Flow {
    Spending { fixed: bool },
    /// 소비 합계에서 제외되는 지출 (적립금에서 쓰는 경조사비·여행 등)
    ExcludedSpending,
    Income,
}

pub fn classify(tx: &transaction::Model, categories: &HashMap<i32, category::Model>) -> Flow {
    match tx.category_id.and_then(|id| categories.get(&id)) {
        Some(c) if c.kind == CategoryKind::Income => Flow::Income,
        Some(c) if c.excluded_from_spending => Flow::ExcludedSpending,
        Some(c) => Flow::Spending { fixed: c.is_fixed },
        None => Flow::Spending { fixed: false },
    }
}

/// 돈이 나간 방향 기준 금액 (지출 +, 수입 −). 결제수단 사용액·카드 청구액에 쓴다.
pub fn outflow(tx: &transaction::Model, categories: &HashMap<i32, category::Model>) -> i64 {
    match classify(tx, categories) {
        Flow::Income => -tx.amount,
        _ => tx.amount,
    }
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct CategorySummary {
    pub category_id: Option<i32>,
    pub ledger_id: Option<i32>,
    pub name: String,
    pub kind: CategoryKind,
    pub is_fixed: bool,
    pub excluded_from_spending: bool,
    pub confirmed: i64,
    pub planned: i64,
    pub count: i32,
    /// 비교 기간 월평균 (확정 기준)
    pub average: f64,
    pub budget: Option<i64>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct PaymentMethodSummary {
    pub payment_method_id: Option<i32>,
    pub name: String,
    pub confirmed: i64,
    pub planned: i64,
    pub count: i32,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct MemberSummary {
    pub member: String,
    /// 구성원 표시 지출 (확정)
    pub spending: i64,
    pub planned: i64,
    /// 지출계획의 구성원 용돈
    pub allowance: Option<i64>,
    pub remaining: Option<i64>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct DailyPoint {
    pub day: i32,
    pub date: NaiveDate,
    pub spending: i64,
    pub cumulative: i64,
    /// 예정 포함 누적
    pub projected_cumulative: i64,
    /// 비교 기간의 같은 날짜까지 평균 누적
    pub average_cumulative: f64,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct MonthPoint {
    pub month: YearMonth,
    pub spending: i64,
    pub planned_spending: i64,
    pub excluded_spending: i64,
    pub income: i64,
    pub fixed_spending: i64,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct PlanSummary {
    /// 계획 수입 (월급 등)
    pub income: i64,
    /// 배분 합계 (용돈·저축·적금 등)
    pub allocations: i64,
    pub member_allowances: i64,
    pub items: Vec<plan_item::Model>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct MonthlySummary {
    pub month: YearMonth,
    pub ledger_id: Option<i32>,
    pub average_months: i32,
    /// 평균 계산에 실제로 쓴 달 수 (거래가 있는 달만)
    pub average_months_used: i32,
    /// 이번 달 확정 소비
    pub spending: i64,
    /// 이번 달 남은 예정 소비
    pub planned_spending: i64,
    pub fixed_spending: i64,
    pub variable_spending: i64,
    pub excluded_spending: i64,
    pub planned_excluded_spending: i64,
    pub income: i64,
    pub planned_income: i64,
    pub average_spending: f64,
    pub average_fixed_spending: f64,
    pub average_variable_spending: f64,
    pub budget_total: Option<i64>,
    pub plan: Option<PlanSummary>,
    /// 월잔액 = 계획 수입 + 수입 − 소비 − 배분
    pub balance: Option<i64>,
    /// 예정 거래까지 반영한 월잔액
    pub projected_balance: Option<i64>,
    pub categories: Vec<CategorySummary>,
    pub payment_methods: Vec<PaymentMethodSummary>,
    pub members: Vec<MemberSummary>,
    pub daily: Vec<DailyPoint>,
    pub history: Vec<MonthPoint>,
}

pub struct SummaryContext<'a> {
    pub categories: &'a HashMap<i32, category::Model>,
    pub payment_methods: &'a HashMap<i32, payment_method::Model>,
    pub ledger_id: Option<i32>,
    /// `ledger_id`가 있을 때 그 장부의 계획 항목
    pub plan_items: &'a [plan_item::Model],
}

#[derive(Default, Clone, Copy)]
struct Totals {
    spending: i64,
    fixed: i64,
    excluded: i64,
    income: i64,
}

impl Totals {
    fn add(&mut self, flow: Flow, amount: i64) {
        match flow {
            Flow::Spending { fixed } => {
                self.spending += amount;
                if fixed {
                    self.fixed += amount;
                }
            }
            Flow::ExcludedSpending => self.excluded += amount,
            Flow::Income => self.income += amount,
        }
    }
}

/// 이번 달 요약을 계산한다.
/// `txs`: 장부 범위로 걸러진, `month - max(average_months, history_months)`부터 `month` 말일까지의 거래.
pub fn monthly_summary(
    ctx: &SummaryContext,
    month: YearMonth,
    average_months: u32,
    history_months: u32,
    txs: &[transaction::Model],
) -> MonthlySummary {
    let mut confirmed: BTreeMap<YearMonth, Totals> = BTreeMap::new();
    let mut planned: BTreeMap<YearMonth, Totals> = BTreeMap::new();
    let mut active_months: BTreeMap<YearMonth, bool> = BTreeMap::new();
    // (month, category) → 확정 합계
    let mut cat_month: HashMap<(YearMonth, Option<i32>), i64> = HashMap::new();
    let mut daily_by_month: HashMap<YearMonth, Vec<i64>> = HashMap::new();
    let mut cur_categories: HashMap<Option<i32>, (i64, i64, i32)> = HashMap::new();
    let mut cur_methods: HashMap<Option<i32>, (i64, i64, i32)> = HashMap::new();
    let mut cur_members: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut cur_planned_daily = vec![0i64; month.days() as usize];

    for tx in txs {
        if tx.status == TxStatus::Skipped {
            continue;
        }
        let ym = YearMonth::of(tx.date);
        let flow = classify(tx, ctx.categories);
        let is_confirmed = tx.status == TxStatus::Confirmed;
        if is_confirmed {
            confirmed.entry(ym).or_default().add(flow, tx.amount);
            active_months.insert(ym, true);
            *cat_month.entry((ym, tx.category_id)).or_default() += tx.amount;
            if matches!(flow, Flow::Spending { .. }) {
                let days = daily_by_month.entry(ym).or_insert_with(|| vec![0; ym.days() as usize]);
                days[tx.date.day0() as usize] += tx.amount;
            }
        } else {
            planned.entry(ym).or_default().add(flow, tx.amount);
        }
        if ym != month {
            continue;
        }
        let c = cur_categories.entry(tx.category_id).or_default();
        let m = cur_methods.entry(tx.payment_method_id).or_default();
        let out = outflow(tx, ctx.categories);
        if is_confirmed {
            c.0 += tx.amount;
            m.0 += out;
        } else {
            c.1 += tx.amount;
            m.1 += out;
            if matches!(flow, Flow::Spending { .. }) {
                cur_planned_daily[tx.date.day0() as usize] += tx.amount;
            }
        }
        c.2 += 1;
        m.2 += 1;
        if let Some(member) = tx.member.as_ref().filter(|m| !m.is_empty())
            && flow != Flow::Income {
                let e = cur_members.entry(member.clone()).or_default();
                if is_confirmed {
                    e.0 += tx.amount;
                } else {
                    e.1 += tx.amount;
                }
            }
    }

    // 평균: 이번 달 이전 average_months개월 중 확정 거래가 있는 달만
    let avg_range: Vec<YearMonth> = (1..=average_months as i32).map(|i| month.add_months(-i)).collect();
    let used: Vec<YearMonth> = avg_range.iter().copied().filter(|m| active_months.contains_key(m)).collect();
    let n = used.len() as f64;
    let avg = |f: &dyn Fn(&Totals) -> i64| -> f64 {
        if used.is_empty() {
            0.0
        } else {
            used.iter().map(|m| confirmed.get(m).map_or(0, f)).sum::<i64>() as f64 / n
        }
    };
    let average_spending = avg(&|t| t.spending);
    let average_fixed = avg(&|t| t.fixed);

    let cur = confirmed.get(&month).copied().unwrap_or_default();
    let cur_planned = planned.get(&month).copied().unwrap_or_default();

    // 카테고리 목록: 장부 범위의 카테고리 + 이번 달/평균에 금액이 있는 카테고리
    let mut cats: Vec<&category::Model> = ctx
        .categories
        .values()
        .filter(|c| ctx.ledger_id.is_none_or(|l| c.ledger_id == l))
        .collect();
    cats.sort_by_key(|c| (c.ledger_id, c.sort_order, c.id));
    let mut categories = Vec::new();
    let mut push_category = |cid: Option<i32>, c: Option<&category::Model>| {
        let (conf, plan, count) = cur_categories.get(&cid).copied().unwrap_or_default();
        let average = if used.is_empty() {
            0.0
        } else {
            used.iter().map(|m| cat_month.get(&(*m, cid)).copied().unwrap_or(0)).sum::<i64>() as f64 / n
        };
        // 보관된 카테고리와 미분류는 금액이 있을 때만 보여준다.
        let has_activity = conf != 0 || plan != 0 || count != 0 || average != 0.0;
        if !has_activity && c.is_none_or(|c| c.archived) {
            return;
        }
        categories.push(CategorySummary {
            category_id: cid,
            ledger_id: c.map(|c| c.ledger_id),
            name: c.map_or_else(|| UNCATEGORIZED.to_string(), |c| c.name.clone()),
            kind: c.map_or(CategoryKind::Expense, |c| c.kind),
            is_fixed: c.is_some_and(|c| c.is_fixed),
            excluded_from_spending: c.is_some_and(|c| c.excluded_from_spending),
            confirmed: conf,
            planned: plan,
            count,
            average,
            budget: c.and_then(|c| c.monthly_budget),
        });
    };
    for c in &cats {
        push_category(Some(c.id), Some(c));
    }
    push_category(None, None);
    // 다른 장부 카테고리로 잘못 분류된 거래도 빠뜨리지 않는다
    let listed: Vec<Option<i32>> = categories.iter().map(|c| c.category_id).collect();
    for (cid, (conf, plan, count)) in &cur_categories {
        if listed.contains(cid) {
            continue;
        }
        let c = cid.and_then(|id| ctx.categories.get(&id));
        categories.push(CategorySummary {
            category_id: *cid,
            ledger_id: c.map(|c| c.ledger_id),
            name: c.map_or_else(|| UNCATEGORIZED.to_string(), |c| c.name.clone()),
            kind: c.map_or(CategoryKind::Expense, |c| c.kind),
            is_fixed: c.is_some_and(|c| c.is_fixed),
            excluded_from_spending: c.is_some_and(|c| c.excluded_from_spending),
            confirmed: *conf,
            planned: *plan,
            count: *count,
            average: 0.0,
            budget: c.and_then(|c| c.monthly_budget),
        });
    }

    let budget_total = {
        let budgets: Vec<i64> = categories
            .iter()
            .filter(|c| c.kind == CategoryKind::Expense && !c.excluded_from_spending)
            .filter_map(|c| c.budget)
            .collect();
        (!budgets.is_empty()).then(|| budgets.iter().sum())
    };

    let mut payment_methods: Vec<PaymentMethodSummary> = cur_methods
        .iter()
        .map(|(pid, (conf, plan, count))| PaymentMethodSummary {
            payment_method_id: *pid,
            name: pid
                .and_then(|id| ctx.payment_methods.get(&id))
                .map_or_else(|| "미지정".to_string(), |p| p.name.clone()),
            confirmed: *conf,
            planned: *plan,
            count: *count,
        })
        .collect();
    payment_methods.sort_by(|a, b| (b.confirmed + b.planned).cmp(&(a.confirmed + a.planned)).then(a.name.cmp(&b.name)));

    // 지출계획
    let plan = ctx.ledger_id.map(|_| {
        let sum = |k: PlanItemKind| ctx.plan_items.iter().filter(|p| p.kind == k).map(|p| p.amount).sum::<i64>();
        let mut items = ctx.plan_items.to_vec();
        items.sort_by_key(|p| (p.sort_order, p.id));
        PlanSummary {
            income: sum(PlanItemKind::Income),
            allocations: sum(PlanItemKind::Allocation),
            member_allowances: sum(PlanItemKind::MemberAllowance),
            items,
        }
    });
    let balance = plan.as_ref().map(|p| p.income + cur.income - cur.spending - p.allocations);
    let projected_balance = balance.map(|b| b + cur_planned.income - cur_planned.spending);

    // 구성원
    let mut members: Vec<MemberSummary> = Vec::new();
    for (member, (spent, plan_amt)) in &cur_members {
        members.push(MemberSummary { member: member.clone(), spending: *spent, planned: *plan_amt, allowance: None, remaining: None });
    }
    for item in ctx.plan_items.iter().filter(|p| p.kind == PlanItemKind::MemberAllowance) {
        let Some(member) = item.member.clone().filter(|m| !m.is_empty()) else { continue };
        match members.iter_mut().find(|m| m.member == member) {
            Some(m) => m.allowance = Some(m.allowance.unwrap_or(0) + item.amount),
            None => members.push(MemberSummary { member, spending: 0, planned: 0, allowance: Some(item.amount), remaining: None }),
        }
    }
    for m in &mut members {
        m.remaining = m.allowance.map(|a| a - m.spending);
    }

    // 일별 누적
    let cur_daily = daily_by_month.get(&month).cloned().unwrap_or_else(|| vec![0; month.days() as usize]);
    let mut daily = Vec::with_capacity(cur_daily.len());
    let (mut cum, mut proj) = (0i64, 0i64);
    for (i, spent) in cur_daily.iter().enumerate() {
        cum += spent;
        proj += spent + cur_planned_daily[i];
        let day = i as u32 + 1;
        let average_cumulative = if used.is_empty() {
            0.0
        } else {
            used.iter()
                .map(|m| {
                    let days = daily_by_month.get(m);
                    let upto = day.min(m.days()) as usize;
                    days.map_or(0, |d| d[..upto].iter().sum::<i64>())
                })
                .sum::<i64>() as f64
                / n
        };
        daily.push(DailyPoint {
            day: day as i32,
            date: month.day(day),
            spending: *spent,
            cumulative: cum,
            projected_cumulative: proj,
            average_cumulative,
        });
    }

    let history = (0..history_months.max(1) as i32)
        .rev()
        .map(|i| month.add_months(-i))
        .map(|m| {
            let c = confirmed.get(&m).copied().unwrap_or_default();
            let p = planned.get(&m).copied().unwrap_or_default();
            MonthPoint {
                month: m,
                spending: c.spending,
                planned_spending: p.spending,
                excluded_spending: c.excluded,
                income: c.income,
                fixed_spending: c.fixed,
            }
        })
        .collect();

    MonthlySummary {
        month,
        ledger_id: ctx.ledger_id,
        average_months: average_months as i32,
        average_months_used: used.len() as i32,
        spending: cur.spending,
        planned_spending: cur_planned.spending,
        fixed_spending: cur.fixed,
        variable_spending: cur.spending - cur.fixed,
        excluded_spending: cur.excluded,
        planned_excluded_spending: cur_planned.excluded,
        income: cur.income,
        planned_income: cur_planned.income,
        average_spending,
        average_fixed_spending: average_fixed,
        average_variable_spending: average_spending - average_fixed,
        budget_total,
        plan,
        balance,
        projected_balance,
        categories,
        payment_methods,
        members,
        daily,
        history,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::enums::TxSource;
    use chrono::Utc;

    fn cat(id: i32, name: &str, kind: CategoryKind, fixed: bool, excluded: bool, budget: Option<i64>) -> category::Model {
        category::Model {
            id,
            ledger_id: 1,
            name: name.into(),
            kind,
            monthly_budget: budget,
            is_fixed: fixed,
            excluded_from_spending: excluded,
            color: None,
            sort_order: id,
            archived: false,
        }
    }

    fn tx(id: i32, date: (i32, u32, u32), amount: i64, cat: Option<i32>, status: TxStatus, member: Option<&str>) -> transaction::Model {
        transaction::Model {
            id,
            ledger_id: 1,
            date: NaiveDate::from_ymd_opt(date.0, date.1, date.2).unwrap(),
            amount,
            category_id: cat,
            payment_method_id: Some(1),
            description: format!("t{id}"),
            memo: None,
            member: member.map(str::to_string),
            status,
            installment_months: 1,
            exclude_from_performance: false,
            recurring_rule_id: None,
            recurring_month: None,
            source: TxSource::Manual,
            external_id: None,
            raw_text: None,
            split_parent_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn summary_matches_sheet_rules() {
        let categories: HashMap<i32, category::Model> = [
            cat(1, "외식", CategoryKind::Expense, false, false, Some(500_000)),
            cat(2, "비용", CategoryKind::Expense, true, false, Some(2_500_000)),
            cat(3, "여행", CategoryKind::Expense, false, true, None),
            cat(4, "기타수익", CategoryKind::Income, false, false, None),
        ]
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
        let pms = HashMap::new();
        let plan_items = vec![
            plan_item::Model { id: 1, ledger_id: 1, kind: PlanItemKind::Income, name: "월급".into(), amount: 4_500_000, member: None, sort_order: 0 },
            plan_item::Model { id: 2, ledger_id: 1, kind: PlanItemKind::Allocation, name: "재홍 용돈".into(), amount: 200_000, member: None, sort_order: 1 },
            plan_item::Model { id: 3, ledger_id: 1, kind: PlanItemKind::MemberAllowance, name: "송화 용돈".into(), amount: 500_000, member: Some("송화".into()), sort_order: 2 },
        ];
        let ctx = SummaryContext { categories: &categories, payment_methods: &pms, ledger_id: Some(1), plan_items: &plan_items };
        let txs = vec![
            // 지난 두 달
            tx(1, (2026, 8, 3), 100_000, Some(1), TxStatus::Confirmed, None),
            tx(2, (2026, 8, 10), 1_000_000, Some(2), TxStatus::Confirmed, None),
            tx(3, (2026, 7, 3), 300_000, Some(1), TxStatus::Confirmed, None),
            // 이번 달
            tx(4, (2026, 10, 1), 50_000, Some(1), TxStatus::Confirmed, Some("송화")),
            tx(5, (2026, 10, 2), 20_000, Some(1), TxStatus::Confirmed, None),
            tx(6, (2026, 10, 10), 1_000_000, Some(2), TxStatus::Planned, None),
            tx(7, (2026, 10, 5), 700_000, Some(3), TxStatus::Confirmed, None),
            tx(8, (2026, 10, 6), 30_000, Some(4), TxStatus::Confirmed, None),
            tx(9, (2026, 10, 7), 99_999, Some(1), TxStatus::Skipped, None),
        ];
        let s = monthly_summary(&ctx, YearMonth::new(2026, 10).unwrap(), 6, 3, &txs);
        assert_eq!(s.spending, 70_000);
        assert_eq!(s.planned_spending, 1_000_000);
        assert_eq!(s.excluded_spending, 700_000);
        assert_eq!(s.income, 30_000);
        assert_eq!(s.average_months_used, 2);
        assert_eq!(s.average_spending, 700_000.0); // (1,100,000 + 300,000) / 2
        assert_eq!(s.average_fixed_spending, 500_000.0);
        assert_eq!(s.budget_total, Some(3_000_000));
        assert_eq!(s.balance, Some(4_500_000 + 30_000 - 70_000 - 200_000));
        assert_eq!(s.projected_balance, Some(4_500_000 + 30_000 - 70_000 - 200_000 - 1_000_000));
        let food = s.categories.iter().find(|c| c.name == "외식").unwrap();
        assert_eq!((food.confirmed, food.planned, food.average), (70_000, 0, 200_000.0));
        let songhwa = s.members.iter().find(|m| m.member == "송화").unwrap();
        assert_eq!((songhwa.spending, songhwa.allowance, songhwa.remaining), (50_000, Some(500_000), Some(450_000)));
        assert_eq!(s.daily.len(), 31);
        assert_eq!(s.daily[1].cumulative, 70_000);
        assert_eq!(s.daily[9].projected_cumulative, 1_070_000);
        // 8월 3일까지 평균 누적: (100,000 + 300,000) / 2
        assert_eq!(s.daily[2].average_cumulative, 200_000.0);
        assert_eq!(s.history.iter().map(|h| h.spending).collect::<Vec<_>>(), vec![1_100_000, 0, 70_000]);
    }
}
