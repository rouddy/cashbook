//! 카드 전월실적과 혜택(할인/적립) 계산.
//!
//! 스프레드시트 카드 시트의 계산식을 일반화했다.
//! - 건별 혜택 = 이용금액 × 혜택률 (건당 최소 결제금액 미만이면 0, 건당 최대 혜택으로 제한)
//! - 항목별 월 한도(`monthly_cap`)와 카드 통합 한도(전월실적 구간, `CardTier`)를 날짜순으로 누적 적용
//! - 전월실적 = 이번 달 이용금액 합계 (실적 제외 거래, 실적 제외 혜택을 받은 거래는 빼고)
//! - 한 거래에 고른 혜택은 모두 더한다 (백화점 1% + 간편결제 3% = 4%). 함께 받을 수 없는 조합은
//!   입력할 때 `stacking_conflict`로 막는다.
//! - 혜택액은 혜택 종류의 단위(할인·포인트는 원, 마일리지는 마일)로 센다.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::entity::{card_benefit, card_tier, enums::BenefitKind};

#[derive(Clone, Debug)]
pub struct BenefitRule {
    pub id: i32,
    pub kind: BenefitKind,
    pub rate: f64,
    pub min_amount: Option<i64>,
    pub per_tx_max: Option<i64>,
    pub monthly_cap: Option<i64>,
    pub shares_card_limit: bool,
    pub excludes_performance: bool,
}

impl From<&card_benefit::Model> for BenefitRule {
    fn from(m: &card_benefit::Model) -> Self {
        Self {
            id: m.id,
            kind: m.kind,
            rate: m.rate,
            min_amount: m.min_amount,
            per_tx_max: m.per_tx_max,
            monthly_cap: m.monthly_cap,
            shares_card_limit: m.shares_card_limit,
            excludes_performance: m.excludes_performance,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tier {
    pub min_spending: i64,
    pub benefit_limit: i64,
}

impl From<&card_tier::Model> for Tier {
    fn from(m: &card_tier::Model) -> Self {
        Self { min_spending: m.min_spending, benefit_limit: m.benefit_limit }
    }
}

/// 실적/혜택 계산에 쓰는 카드 거래 한 건.
#[derive(Clone, Debug)]
pub struct CardTx {
    pub id: i32,
    pub date: NaiveDate,
    pub amount: i64,
    pub exclude_from_performance: bool,
    pub benefit_ids: Vec<i32>,
}

/// 전월실적에 반영되는 금액.
pub fn performance_amount(tx: &CardTx, rules: &HashMap<i32, BenefitRule>) -> i64 {
    if tx.exclude_from_performance {
        return 0;
    }
    if tx
        .benefit_ids
        .iter()
        .any(|id| rules.get(id).is_some_and(|r| r.excludes_performance))
    {
        return 0;
    }
    tx.amount
}

pub fn performance(txs: &[CardTx], rules: &HashMap<i32, BenefitRule>) -> i64 {
    txs.iter().map(|t| performance_amount(t, rules)).sum()
}

/// `spending` 실적으로 도달한 가장 높은 구간.
pub fn tier_for(tiers: &[Tier], spending: i64) -> Option<Tier> {
    tiers
        .iter()
        .filter(|t| spending >= t.min_spending)
        .max_by_key(|t| t.min_spending)
        .copied()
}

/// `spending` 다음 구간 (아직 도달하지 못한 가장 낮은 구간).
pub fn next_tier(tiers: &[Tier], spending: i64) -> Option<Tier> {
    tiers
        .iter()
        .filter(|t| spending < t.min_spending)
        .min_by_key(|t| t.min_spending)
        .copied()
}

/// 전월실적에 따른 이번 달 통합 혜택 한도. 구간이 정의되지 않은 카드는 한도 없음(None).
pub fn limit_for(tiers: &[Tier], previous_performance: i64) -> Option<i64> {
    if tiers.is_empty() {
        return None;
    }
    Some(tier_for(tiers, previous_performance).map_or(0, |t| t.benefit_limit))
}

/// 중복 적용(`stackable`)이 아닌 혜택은 거래당 하나만 받는다. 둘 이상 골랐으면 그 이름들.
pub fn stacking_conflict(selected: &[card_benefit::Model]) -> Option<Vec<String>> {
    let names: Vec<String> = selected.iter().filter(|b| !b.stackable).map(|b| b.name.clone()).collect();
    (names.len() > 1).then_some(names)
}

/// 한도 적용 전 건별 혜택.
pub fn raw_benefit(rule: &BenefitRule, amount: i64) -> i64 {
    if let Some(min) = rule.min_amount
        && amount.abs() < min {
            return 0;
        }
    // 1,500 × (1/1500) = 0.999…처럼 부동소수 오차로 1이 깎이지 않게 소수 6자리에서 반올림한 뒤 버린다.
    let mut v = ((amount as f64 * rule.rate * 1e6).round() / 1e6).trunc() as i64;
    if let Some(max) = rule.per_tx_max {
        v = v.clamp(-max.abs(), max.abs());
    }
    v
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleUsage {
    pub rule_id: i32,
    pub kind: BenefitKind,
    /// 이 혜택이 붙은 이용금액 합계
    pub spending: i64,
    pub count: usize,
    /// 한도 적용 전 혜택 합계
    pub raw: i64,
    /// 한도 적용 후 혜택 합계
    pub applied: i64,
}

#[derive(Clone, Debug, Default)]
pub struct BenefitOutcome {
    /// `rules` 순서대로
    pub per_rule: Vec<RuleUsage>,
    /// 거래·혜택 종류별 적용 혜택
    pub per_tx: HashMap<(i32, BenefitKind), i64>,
    /// 통합 한도에 포함되는 혜택 사용액
    pub shared_used: i64,
    pub limit: Option<i64>,
}

impl BenefitOutcome {
    pub fn total_of(&self, kind: BenefitKind) -> i64 {
        self.per_rule.iter().filter(|u| u.kind == kind).map(|u| u.applied).sum()
    }

    pub fn remaining_limit(&self) -> Option<i64> {
        self.limit.map(|l| (l - self.shared_used).max(0))
    }

    /// 이 혜택 항목으로 더 받을 수 있는 혜택 금액 (None = 제한 없음).
    pub fn remaining_for(&self, rule: &BenefitRule) -> Option<i64> {
        let used = self.per_rule.iter().find(|u| u.rule_id == rule.id).map_or(0, |u| u.applied);
        let by_cap = rule.monthly_cap.map(|cap| (cap - used).max(0));
        let by_limit = if rule.shares_card_limit { self.remaining_limit() } else { None };
        match (by_cap, by_limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// 지금까지 받은 혜택에 이어 `txs`의 혜택을 날짜순(같은 날은 id순)으로 누적한다. `rules`는 `compute`에 준 것과 같아야 한다.
    /// 예정 거래는 확정 거래를 다 계산한 뒤 이것으로 더해, 확정 거래의 혜택은 그대로 두고 남은 한도에서 받게 한다.
    pub fn add(&mut self, txs: &[CardTx], rules: &[BenefitRule]) {
        let mut ordered: Vec<&CardTx> = txs.iter().collect();
        ordered.sort_by_key(|t| (t.date, t.id));
        let index: HashMap<i32, usize> = rules.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
        for tx in ordered {
            for bid in &tx.benefit_ids {
                let Some(&i) = index.get(bid) else { continue };
                let rule = &rules[i];
                let raw = raw_benefit(rule, tx.amount);
                let mut applied = raw;
                if applied > 0 {
                    if let Some(cap) = rule.monthly_cap {
                        applied = applied.min((cap - self.per_rule[i].applied).max(0));
                    }
                    if rule.shares_card_limit
                        && let Some(limit) = self.limit
                    {
                        applied = applied.min((limit - self.shared_used).max(0));
                    }
                } else if applied < 0 {
                    // 환불은 이번 달에 실제로 받은 혜택까지만 되돌린다.
                    // (한도 때문에 덜 받은 혜택을 다 깎으면 남은 한도가 원래 한도보다 커진다)
                    applied = applied.max(-self.per_rule[i].applied);
                }
                let u = &mut self.per_rule[i];
                u.spending += tx.amount;
                u.count += 1;
                u.raw += raw;
                u.applied += applied;
                if rule.shares_card_limit {
                    self.shared_used += applied;
                }
                *self.per_tx.entry((tx.id, rule.kind)).or_default() += applied;
            }
        }
    }

    /// 이번 달 더 받을 수 있는 혜택의 종류별 합계 (종류 순서, 한도가 있는 혜택이 있는 종류만 — 비면 한도 있는 혜택이 없다).
    /// 통합 한도를 함께 쓰는 혜택은 한 묶음으로 남은 통합 한도까지만 센다(묶음 혜택이 모두 항목 한도가 있으면 그 남은 합과 비교).
    /// 묶음에 종류가 섞였으면 통합 한도 단위인 할인(원)으로 센다. 한도가 전혀 없는 혜택은 셀 수 없어 뺀다.
    pub fn remaining_by_kind(&self, rules: &[BenefitRule]) -> Vec<(BenefitKind, i64)> {
        let cap_left = |r: &BenefitRule| {
            let used = self.per_rule.iter().find(|u| u.rule_id == r.id).map_or(0, |u| u.applied);
            r.monthly_cap.map(|cap| (cap - used).max(0))
        };
        let mut totals: Vec<(BenefitKind, i64)> = Vec::new();
        let mut add = |kind: BenefitKind, v: i64| match totals.iter_mut().find(|(k, _)| *k == kind) {
            Some(e) => e.1 += v,
            None => totals.push((kind, v)),
        };
        let (shared, separate): (Vec<&BenefitRule>, Vec<&BenefitRule>) =
            rules.iter().partition(|r| r.shares_card_limit && self.limit.is_some());
        if let (Some(left), Some(first)) = (self.remaining_limit(), shared.first()) {
            let caps: Option<i64> = shared.iter().map(|r| cap_left(r)).sum();
            let kind = if shared.iter().all(|r| r.kind == first.kind) { first.kind } else { BenefitKind::Discount };
            add(kind, caps.map_or(left, |c| c.min(left)));
        }
        for r in separate {
            if let Some(v) = cap_left(r) {
                add(r.kind, v);
            }
        }
        totals.sort_by_key(|(k, _)| *k as u8);
        totals
    }
}

/// 날짜순(같은 날은 id순)으로 혜택을 누적 계산한다. `txs`는 같은 달의 확정 거래.
pub fn compute(txs: &[CardTx], rules: &[BenefitRule], limit: Option<i64>) -> BenefitOutcome {
    let mut out = BenefitOutcome {
        per_rule: rules.iter().map(|r| RuleUsage { rule_id: r.id, kind: r.kind, ..Default::default() }).collect(),
        per_tx: HashMap::new(),
        shared_used: 0,
        limit,
    };
    out.add(txs, rules);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    impl BenefitOutcome {
        fn total(&self) -> i64 {
            self.per_rule.iter().map(|u| u.applied).sum()
        }

        fn tx_total(&self, tx_id: i32) -> i64 {
            self.per_tx.iter().filter(|((id, _), _)| *id == tx_id).map(|(_, v)| v).sum()
        }
    }

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, day).unwrap()
    }

    fn rule(id: i32, rate: f64) -> BenefitRule {
        BenefitRule {
            id,
            kind: BenefitKind::Discount,
            rate,
            min_amount: None,
            per_tx_max: None,
            monthly_cap: None,
            shares_card_limit: true,
            excludes_performance: false,
        }
    }

    fn tx(id: i32, day: u32, amount: i64, benefits: &[i32]) -> CardTx {
        CardTx { id, date: d(day), amount, exclude_from_performance: false, benefit_ids: benefits.to_vec() }
    }

    #[test]
    fn mg_plus_s_style_min_amount_and_limit() {
        // 간편결제 10%, 1만원 이상만. 전월실적 103만원 → 한도 6만원
        let mut easy = rule(1, 0.1);
        easy.min_amount = Some(10_000);
        let tiers = [
            Tier { min_spending: 300_000, benefit_limit: 15_000 },
            Tier { min_spending: 600_000, benefit_limit: 30_000 },
            Tier { min_spending: 1_000_000, benefit_limit: 60_000 },
        ];
        let limit = limit_for(&tiers, 1_031_110);
        assert_eq!(limit, Some(60_000));
        assert_eq!(limit_for(&tiers, 299_999), Some(0));
        let txs = [tx(1, 1, 10_500, &[1]), tx(2, 1, 7_900, &[1]), tx(3, 3, 66_110, &[1])];
        let out = compute(&txs, &[easy.clone()], limit);
        assert_eq!(out.tx_total(1), 1_050);
        assert_eq!(out.tx_total(2), 0);
        assert_eq!(out.tx_total(3), 6_611);
        assert_eq!(out.total(), 7_661);
        assert_eq!(out.remaining_limit(), Some(60_000 - 7_661));
        assert_eq!(out.remaining_for(&easy), Some(60_000 - 7_661));
    }

    #[test]
    fn per_tx_max_and_separate_cap() {
        // 알뜰교통: 주말마트 10% 건당 최대 5천원, 전기차 30%는 통합한도 밖 월 2만원
        let mut mart = rule(1, 0.1);
        mart.per_tx_max = Some(5_000);
        let mut ev = rule(2, 0.3);
        ev.per_tx_max = Some(20_000);
        ev.monthly_cap = Some(20_000);
        ev.shares_card_limit = false;
        let txs = [
            tx(1, 2, 50_200, &[1]),
            tx(2, 8, 27_240, &[1]),
            tx(3, 9, 50_000, &[2]),
            tx(4, 10, 50_000, &[2]),
            tx(5, 15, 51_980, &[1]),
        ];
        let out = compute(&txs, &[mart, ev], Some(10_000));
        assert_eq!(out.tx_total(1), 5_000); // 5,020 → 건당 최대 5,000
        assert_eq!(out.tx_total(2), 2_724);
        assert_eq!(out.tx_total(5), 2_276); // 통합한도 1만원 중 남은 금액
        assert_eq!(out.tx_total(3), 15_000);
        assert_eq!(out.tx_total(4), 5_000); // 전기차 월 2만원 한도
        assert_eq!(out.shared_used, 10_000);
        assert_eq!(out.remaining_limit(), Some(0));
    }

    #[test]
    fn refunds_reduce_benefit_and_performance_exclusions() {
        let mut shop = rule(1, 0.2);
        shop.monthly_cap = Some(15_000);
        shop.excludes_performance = true;
        let rules: HashMap<i32, BenefitRule> = [(1, shop.clone())].into_iter().collect();
        let mut excluded = tx(3, 5, 100_000, &[]);
        excluded.exclude_from_performance = true;
        let txs = [tx(1, 1, 47_510, &[1]), tx(2, 7, -24_300, &[1]), excluded, tx(4, 9, 30_000, &[])];
        assert_eq!(performance(&txs, &rules), 30_000);
        let out = compute(&txs, &[shop], None);
        assert_eq!(out.tx_total(1), 9_502);
        assert_eq!(out.tx_total(2), -4_860);
        assert_eq!(out.total(), 4_642);
    }

    #[test]
    fn refund_only_reverses_granted_benefit() {
        // 20% · 항목 한도 15,000 · 통합 한도 20,000: 100,000원 결제 후 같은 달 전액 환불
        let mut shop = rule(1, 0.2);
        shop.monthly_cap = Some(15_000);
        let txs = [tx(1, 1, 100_000, &[1]), tx(2, 4, -100_000, &[1])];
        let out = compute(&txs, &[shop.clone()], Some(20_000));
        assert_eq!(out.tx_total(1), 15_000);
        assert_eq!(out.tx_total(2), -15_000);
        assert_eq!(out.total(), 0);
        assert_eq!(out.remaining_for(&shop), Some(15_000));
        assert_eq!(out.remaining_limit(), Some(20_000));
        // 혜택이 없던 달의 환불은 혜택을 음수로 만들지 않는다
        let out = compute(&[tx(3, 2, -50_000, &[1])], &[shop], Some(20_000));
        assert_eq!(out.total(), 0);
    }

    #[test]
    fn stacked_and_unlimited_benefits() {
        // 카드의정석: 백화점 1% + 간편결제 3%(중복 적용) = 4%, etc 0.8%는 통합한도를 쓰지 않는다
        let mut dept = rule(1, 0.01);
        dept.min_amount = Some(10_000);
        let mut easy = rule(2, 0.03);
        easy.min_amount = Some(10_000);
        let mut etc = rule(3, 0.008);
        etc.min_amount = Some(10_000);
        etc.shares_card_limit = false;
        let rules = [dept, easy, etc.clone()];
        let txs = [
            tx(1, 1, 200_000, &[1, 2]),
            tx(2, 2, 100_000, &[3, 2]),
            tx(3, 3, 300_000, &[3]),
            tx(4, 4, 50_000, &[1, 2]),
        ];
        let out = compute(&txs, &rules, Some(10_000));
        assert_eq!(out.tx_total(1), 8_000); // 2,000 + 6,000
        assert_eq!(out.tx_total(2), 2_000 + 800); // 간편결제는 남은 한도 2,000원까지
        assert_eq!(out.tx_total(3), 2_400); // 한도를 다 써도 etc는 그대로
        assert_eq!(out.tx_total(4), 0);
        assert_eq!(out.shared_used, 10_000);
        assert_eq!(out.remaining_limit(), Some(0));
        assert_eq!(out.remaining_for(&etc), None);
        // 전월실적 미달(한도 0)이어도 한도 없는 혜택은 받는다
        let out = compute(&txs, &rules, Some(0));
        assert_eq!(out.total(), 800 + 2_400);
    }

    #[test]
    fn stacking_conflict_allows_one_exclusive() {
        let b = |id: i32, name: &str, stackable: bool| card_benefit::Model {
            id,
            card_product_id: 1,
            name: name.into(),
            kind: BenefitKind::Discount,
            rate: 0.01,
            min_amount: None,
            per_tx_max: None,
            monthly_cap: None,
            shares_card_limit: true,
            excludes_performance: false,
            stackable,
            sort_order: 0,
        };
        assert_eq!(stacking_conflict(&[b(1, "백화점", false), b(2, "간편결제", true)]), None);
        assert_eq!(stacking_conflict(&[b(2, "간편결제", true), b(3, "해외", true)]), None);
        assert_eq!(
            stacking_conflict(&[b(1, "백화점", false), b(4, "할인점", false), b(2, "간편결제", true)]),
            Some(vec!["백화점".to_string(), "할인점".to_string()])
        );
    }

    #[test]
    fn mileage_counts_in_miles() {
        // 1,500원당 1마일, 월 최대 1,000마일 (통합 한도와 무관)
        let mut mile = rule(1, 1.0 / 1500.0);
        mile.kind = BenefitKind::Mileage;
        mile.shares_card_limit = false;
        mile.monthly_cap = Some(1_000);
        let disc = rule(2, 0.29);
        let txs = [tx(1, 1, 1_500, &[1]), tx(2, 2, 10_000, &[1]), tx(3, 3, 100, &[2]), tx(4, 4, 3_000_000, &[1])];
        let out = compute(&txs, &[mile, disc], None);
        assert_eq!(out.tx_total(1), 1);
        assert_eq!(out.tx_total(2), 6);
        assert_eq!(out.tx_total(3), 29); // 0.29 × 100 = 28.999…가 아니라 29
        assert_eq!(out.tx_total(4), 1_000 - 7);
        assert_eq!(out.total_of(BenefitKind::Mileage), 1_000);
        assert_eq!(out.total_of(BenefitKind::Discount), 29);
        assert_eq!(out.per_tx[&(2, BenefitKind::Mileage)], 6);
    }

    #[test]
    fn planned_uses_what_confirmed_left() {
        // 10% · 통합 한도 5,000원. 예정 거래가 확정 거래보다 날짜가 앞서도 확정 혜택은 그대로 두고 남은 한도에서 받는다
        let easy = rule(1, 0.1);
        let rules = [easy.clone()];
        let confirmed = [tx(1, 10, 30_000, &[1])];
        let planned = [tx(2, 5, 30_000, &[1]), tx(3, 20, 10_000, &[1])];
        let current = compute(&confirmed, &rules, Some(5_000));
        let mut projected = current.clone();
        projected.add(&planned, &rules);
        assert_eq!(current.tx_total(1), 3_000);
        assert_eq!(current.remaining_for(&easy), Some(2_000));
        assert_eq!(projected.tx_total(1), 3_000);
        assert_eq!(projected.tx_total(2), 2_000);
        assert_eq!(projected.tx_total(3), 0);
        assert_eq!(projected.per_rule[0].spending, 70_000);
        assert_eq!(projected.per_rule[0].count, 3);
        assert_eq!(projected.shared_used, 5_000);
        assert_eq!(projected.remaining_for(&easy), Some(0));
        assert_eq!(projected.remaining_by_kind(&rules), vec![(BenefitKind::Discount, 0)]);
    }

    #[test]
    fn remaining_by_kind_counts_shared_limit_once() {
        // 백화점 1% · 간편결제 3%는 통합 한도 1만원을 함께, 전기차 30%는 별도 월 2만원, etc 0.8%는 한도 없음, 마일리지 월 1,000마일
        let dept = rule(1, 0.01);
        let easy = rule(2, 0.03);
        let mut ev = rule(3, 0.3);
        ev.shares_card_limit = false;
        ev.monthly_cap = Some(20_000);
        let mut etc = rule(4, 0.008);
        etc.shares_card_limit = false;
        let mut mile = rule(5, 1.0 / 1500.0);
        mile.kind = BenefitKind::Mileage;
        mile.shares_card_limit = false;
        mile.monthly_cap = Some(1_000);
        let rules = [dept.clone(), easy.clone(), ev, etc.clone(), mile];
        let txs = [tx(1, 1, 100_000, &[1, 2]), tx(2, 2, 50_000, &[3]), tx(3, 3, 15_000, &[5]), tx(4, 4, 10_000, &[4])];
        let out = compute(&txs, &rules, Some(10_000));
        // 통합 한도 남은 6,000원을 한 번만 + 전기차 5,000원, 마일리지 990마일, etc는 빠진다
        assert_eq!(out.remaining_by_kind(&rules), vec![(BenefitKind::Discount, 11_000), (BenefitKind::Mileage, 990)]);

        // 통합 한도를 함께 쓰는 혜택이 모두 항목 한도가 있으면 그 남은 합까지만
        let mut dept_cap = dept.clone();
        dept_cap.monthly_cap = Some(1_500);
        let mut easy_cap = easy.clone();
        easy_cap.monthly_cap = Some(2_000);
        let rules = [dept_cap, easy_cap];
        let out = compute(&[tx(1, 1, 100_000, &[1, 2])], &rules, Some(10_000));
        assert_eq!(out.remaining_limit(), Some(7_000));
        assert_eq!(out.remaining_by_kind(&rules), vec![(BenefitKind::Discount, 500)]);

        // 통합 한도가 없는 카드는 항목 한도만, 한도 있는 혜택이 없으면 빈 목록
        let mut capped = easy.clone();
        capped.monthly_cap = Some(5_000);
        let out = compute(&[tx(1, 1, 100_000, &[2])], &[capped.clone(), etc.clone()], None);
        assert_eq!(out.remaining_by_kind(&[capped, etc.clone()]), vec![(BenefitKind::Discount, 2_000)]);
        assert_eq!(compute(&[], &[etc.clone()], None).remaining_by_kind(&[etc]), vec![]);

        // 포인트만 통합 한도를 쓰면 포인트로, 종류가 섞이면 할인(원)으로 센다
        let mut point = rule(6, 0.01);
        point.kind = BenefitKind::Point;
        let out = compute(&[], &[point.clone()], Some(3_000));
        assert_eq!(out.remaining_by_kind(&[point.clone()]), vec![(BenefitKind::Point, 3_000)]);
        let out = compute(&[], &[point.clone(), dept.clone()], Some(3_000));
        assert_eq!(out.remaining_by_kind(&[point, dept]), vec![(BenefitKind::Discount, 3_000)]);
    }

    #[test]
    fn tier_progress() {
        let tiers = [
            Tier { min_spending: 300_000, benefit_limit: 10_000 },
            Tier { min_spending: 600_000, benefit_limit: 20_000 },
        ];
        assert_eq!(tier_for(&tiers, 450_000).map(|t| t.benefit_limit), Some(10_000));
        assert_eq!(next_tier(&tiers, 450_000).map(|t| t.min_spending), Some(600_000));
        assert_eq!(next_tier(&tiers, 700_000), None);
        assert_eq!(limit_for(&[], 0), None);
    }
}
