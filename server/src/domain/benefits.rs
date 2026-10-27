//! 카드 전월실적과 혜택(할인/적립) 계산.
//!
//! 스프레드시트 카드 시트의 계산식을 일반화했다.
//! - 건별 혜택 = 이용금액 × 혜택률 (건당 최소 결제금액 미만이면 0, 건당 최대 혜택으로 제한)
//! - 항목별 월 한도(`monthly_cap`)와 카드 통합 한도(전월실적 구간, `CardTier`)를 날짜순으로 누적 적용
//! - 전월실적 = 이번 달 이용금액 합계 (실적 제외 거래, 실적 제외 혜택을 받은 거래는 빼고)

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::entity::{card_benefit, card_tier};

#[derive(Clone, Debug)]
pub struct BenefitRule {
    pub id: i32,
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

/// 한도 적용 전 건별 혜택.
pub fn raw_benefit(rule: &BenefitRule, amount: i64) -> i64 {
    if let Some(min) = rule.min_amount
        && amount.abs() < min {
            return 0;
        }
    let mut v = (amount as f64 * rule.rate).trunc() as i64;
    if let Some(max) = rule.per_tx_max {
        v = v.clamp(-max.abs(), max.abs());
    }
    v
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleUsage {
    pub rule_id: i32,
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
    /// 거래별 적용 혜택
    pub per_tx: HashMap<i32, i64>,
    pub total: i64,
    /// 통합 한도에 포함되는 혜택 사용액
    pub shared_used: i64,
    pub limit: Option<i64>,
}

impl BenefitOutcome {
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
}

/// 날짜순(같은 날은 id순)으로 혜택을 누적 계산한다. `txs`는 같은 달의 확정 거래.
pub fn compute(txs: &[CardTx], rules: &[BenefitRule], limit: Option<i64>) -> BenefitOutcome {
    let mut ordered: Vec<&CardTx> = txs.iter().collect();
    ordered.sort_by_key(|t| (t.date, t.id));
    let index: HashMap<i32, usize> = rules.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut usage: Vec<RuleUsage> = rules
        .iter()
        .map(|r| RuleUsage { rule_id: r.id, ..Default::default() })
        .collect();
    let mut per_tx: HashMap<i32, i64> = HashMap::new();
    let mut shared_used = 0i64;
    for tx in ordered {
        for bid in &tx.benefit_ids {
            let Some(&i) = index.get(bid) else { continue };
            let rule = &rules[i];
            let raw = raw_benefit(rule, tx.amount);
            let mut applied = raw;
            if applied > 0 {
                if let Some(cap) = rule.monthly_cap {
                    applied = applied.min((cap - usage[i].applied).max(0));
                }
                if rule.shares_card_limit
                    && let Some(limit) = limit
                {
                    applied = applied.min((limit - shared_used).max(0));
                }
            } else if applied < 0 {
                // 환불은 이번 달에 실제로 받은 혜택까지만 되돌린다.
                // (한도 때문에 덜 받은 혜택을 다 깎으면 남은 한도가 원래 한도보다 커진다)
                applied = applied.max(-usage[i].applied);
            }
            let u = &mut usage[i];
            u.spending += tx.amount;
            u.count += 1;
            u.raw += raw;
            u.applied += applied;
            if rule.shares_card_limit {
                shared_used += applied;
            }
            *per_tx.entry(tx.id).or_default() += applied;
        }
    }
    let total = usage.iter().map(|u| u.applied).sum();
    BenefitOutcome { per_rule: usage, per_tx, total, shared_used, limit }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, day).unwrap()
    }

    fn rule(id: i32, rate: f64) -> BenefitRule {
        BenefitRule {
            id,
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
        assert_eq!(out.per_tx[&1], 1_050);
        assert_eq!(out.per_tx[&2], 0);
        assert_eq!(out.per_tx[&3], 6_611);
        assert_eq!(out.total, 7_661);
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
        assert_eq!(out.per_tx[&1], 5_000); // 5,020 → 건당 최대 5,000
        assert_eq!(out.per_tx[&2], 2_724);
        assert_eq!(out.per_tx[&5], 2_276); // 통합한도 1만원 중 남은 금액
        assert_eq!(out.per_tx[&3], 15_000);
        assert_eq!(out.per_tx[&4], 5_000); // 전기차 월 2만원 한도
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
        assert_eq!(out.per_tx[&1], 9_502);
        assert_eq!(out.per_tx[&2], -4_860);
        assert_eq!(out.total, 4_642);
    }

    #[test]
    fn refund_only_reverses_granted_benefit() {
        // 20% · 항목 한도 15,000 · 통합 한도 20,000: 100,000원 결제 후 같은 달 전액 환불
        let mut shop = rule(1, 0.2);
        shop.monthly_cap = Some(15_000);
        let txs = [tx(1, 1, 100_000, &[1]), tx(2, 4, -100_000, &[1])];
        let out = compute(&txs, &[shop.clone()], Some(20_000));
        assert_eq!(out.per_tx[&1], 15_000);
        assert_eq!(out.per_tx[&2], -15_000);
        assert_eq!(out.total, 0);
        assert_eq!(out.remaining_for(&shop), Some(15_000));
        assert_eq!(out.remaining_limit(), Some(20_000));
        // 혜택이 없던 달의 환불은 혜택을 음수로 만들지 않는다
        let out = compute(&[tx(3, 2, -50_000, &[1])], &[shop], Some(20_000));
        assert_eq!(out.total, 0);
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
