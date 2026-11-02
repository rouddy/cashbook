//! 카드 소비 계획: 이번 달 실적(전월실적 목표 대비), 혜택 사용/잔여, 다음 구간까지 남은 금액.

use std::collections::HashMap;

use async_graphql::SimpleObject;

use super::benefits::{self, BenefitRule, CardTx, Tier};
use super::dates::YearMonth;
use super::summary::outflow;
use crate::entity::{
    card_benefit, card_tier, category,
    enums::{BenefitKind, TxStatus},
    payment_method, transaction,
};

#[derive(SimpleObject, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierInfo {
    pub min_spending: i64,
    pub benefit_limit: i64,
}

impl From<Tier> for TierInfo {
    fn from(t: Tier) -> Self {
        Self { min_spending: t.min_spending, benefit_limit: t.benefit_limit }
    }
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct BenefitUsage {
    pub card_benefit_id: i32,
    pub name: String,
    pub kind: BenefitKind,
    pub rate: f64,
    pub monthly_cap: Option<i64>,
    pub shares_card_limit: bool,
    pub stackable: bool,
    /// 이 혜택이 붙은 이용금액
    pub spending: i64,
    pub count: i32,
    /// 한도 적용 전 혜택 (`kind` 단위)
    pub raw: i64,
    /// 한도 적용 후 혜택 (`kind` 단위)
    pub amount: i64,
    /// 더 받을 수 있는 혜택 (`kind` 단위, 없으면 제한 없음)
    pub remaining: Option<i64>,
    /// 남은 혜택을 다 받으려면 더 써야 하는 금액
    pub spend_to_max: Option<i64>,
}

#[derive(SimpleObject, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxBenefitAmount {
    pub transaction_id: i32,
    pub kind: BenefitKind,
    /// `kind` 단위 (원·P·마일)
    pub amount: i64,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
pub struct CardMonthReport {
    pub payment_method_id: i32,
    pub month: YearMonth,
    /// 이번 달 확정 이용금액
    pub spending: i64,
    /// 이번 달 남은 예정 이용금액
    pub planned_spending: i64,
    pub transaction_count: i32,
    /// 이번 달 실적 (다음 달 혜택 구간 판정 기준)
    pub performance: i64,
    /// 예정 거래까지 포함한 실적 전망
    pub projected_performance: i64,
    /// 지난 달 실적 (이번 달 혜택 한도 기준)
    pub previous_performance: i64,
    pub target: Option<i64>,
    pub remaining_to_target: Option<i64>,
    /// 지난 달 실적으로 정해진 이번 달 통합 혜택 한도 (None = 한도 없음)
    pub current_limit: Option<i64>,
    /// 이번 달 실적으로 달성한 구간 (다음 달 한도)
    pub achieved_tier: Option<TierInfo>,
    pub next_tier: Option<TierInfo>,
    pub remaining_to_next_tier: Option<i64>,
    /// 원으로 셀 수 있는 혜택 합계 (할인 + 포인트)
    pub benefit_total: i64,
    pub discount_total: i64,
    pub point_total: i64,
    /// 마일
    pub mileage_total: i64,
    /// 통합 한도에서 차감된 혜택 (한도 없는 혜택·별도 한도 혜택은 빠진다)
    pub limit_used: i64,
    /// 통합 한도 중 남은 금액
    pub benefit_remaining: Option<i64>,
    pub benefits: Vec<BenefitUsage>,
    /// 거래·혜택 종류별 혜택액
    pub transaction_benefits: Vec<TxBenefitAmount>,
}

pub struct CardReportInput<'a> {
    pub payment_method: &'a payment_method::Model,
    pub benefits: &'a [card_benefit::Model],
    pub tiers: &'a [card_tier::Model],
    pub categories: &'a HashMap<i32, category::Model>,
    /// 이 카드의 지난 달 ~ 이번 달 거래
    pub transactions: &'a [transaction::Model],
    pub tx_benefits: &'a HashMap<i32, Vec<i32>>,
}

fn card_txs(input: &CardReportInput, month: YearMonth, status: TxStatus) -> Vec<CardTx> {
    input
        .transactions
        .iter()
        .filter(|t| t.payment_method_id == Some(input.payment_method.id))
        .filter(|t| t.status == status && month.contains(t.date))
        .map(|t| CardTx {
            id: t.id,
            date: t.date,
            amount: outflow(t, input.categories),
            exclude_from_performance: t.exclude_from_performance,
            benefit_ids: input.tx_benefits.get(&t.id).cloned().unwrap_or_default(),
        })
        .collect()
}

pub fn month_report(input: &CardReportInput, month: YearMonth) -> CardMonthReport {
    let mut rules_sorted: Vec<&card_benefit::Model> = input.benefits.iter().collect();
    rules_sorted.sort_by_key(|b| (b.sort_order, b.id));
    let rules: Vec<BenefitRule> = rules_sorted.iter().map(|b| BenefitRule::from(*b)).collect();
    let rule_map: HashMap<i32, BenefitRule> = rules.iter().map(|r| (r.id, r.clone())).collect();
    let tiers: Vec<Tier> = input.tiers.iter().map(Tier::from).collect();

    let current = card_txs(input, month, TxStatus::Confirmed);
    let planned = card_txs(input, month, TxStatus::Planned);
    let previous = card_txs(input, month.prev(), TxStatus::Confirmed);

    let performance = benefits::performance(&current, &rule_map);
    let projected_performance = performance + benefits::performance(&planned, &rule_map);
    let previous_performance = benefits::performance(&previous, &rule_map);
    let limit = benefits::limit_for(&tiers, previous_performance);
    let outcome = benefits::compute(&current, &rules, limit);

    let usages = rules_sorted
        .iter()
        .zip(&rules)
        .map(|(model, rule)| {
            let u = outcome.per_rule.iter().find(|u| u.rule_id == rule.id).cloned().unwrap_or_default();
            let remaining = outcome.remaining_for(rule);
            let spend_to_max = remaining.and_then(|r| (rule.rate > 0.0).then(|| (r as f64 / rule.rate).ceil() as i64));
            BenefitUsage {
                card_benefit_id: model.id,
                name: model.name.clone(),
                kind: model.kind,
                rate: model.rate,
                monthly_cap: model.monthly_cap,
                shares_card_limit: model.shares_card_limit,
                stackable: model.stackable,
                spending: u.spending,
                count: u.count as i32,
                raw: u.raw,
                amount: u.applied,
                remaining,
                spend_to_max,
            }
        })
        .collect();

    let mut transaction_benefits: Vec<TxBenefitAmount> = outcome
        .per_tx
        .iter()
        .map(|((id, kind), amount)| TxBenefitAmount { transaction_id: *id, kind: *kind, amount: *amount })
        .collect();
    transaction_benefits.sort_by_key(|t| (t.transaction_id, t.kind as u8));

    let discount_total = outcome.total_of(BenefitKind::Discount);
    let point_total = outcome.total_of(BenefitKind::Point);
    let target = input.payment_method.monthly_target.filter(|t| *t > 0);
    let next_tier = benefits::next_tier(&tiers, performance);
    CardMonthReport {
        payment_method_id: input.payment_method.id,
        month,
        spending: current.iter().map(|t| t.amount).sum(),
        planned_spending: planned.iter().map(|t| t.amount).sum(),
        transaction_count: current.len() as i32,
        performance,
        projected_performance,
        previous_performance,
        target,
        remaining_to_target: target.map(|t| (t - performance).max(0)),
        current_limit: limit,
        achieved_tier: benefits::tier_for(&tiers, performance).map(TierInfo::from),
        next_tier: next_tier.map(TierInfo::from),
        remaining_to_next_tier: next_tier.map(|t| t.min_spending - performance),
        benefit_total: discount_total + point_total,
        discount_total,
        point_total,
        mileage_total: outcome.total_of(BenefitKind::Mileage),
        limit_used: outcome.shared_used,
        benefit_remaining: outcome.remaining_limit(),
        benefits: usages,
        transaction_benefits,
    }
}
