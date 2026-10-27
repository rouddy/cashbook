//! 신용카드 정산 일정: 이용기간, 결제일, 할부 분할, 결제(예정) 금액.
//!
//! 카드사는 결제일마다 이용기간을 정해 둔다 (예: 25일 결제 → 전월 12일 ~ 당월 11일).
//! 여기서는 "결제월 M의 청구서"를 기준으로
//! - 마감일 = (M + closing_month_offset)월의 closing_day일 (말일 초과 시 말일)
//! - 이용기간 = (M-1 청구서 마감일 다음날) ~ (M 청구서 마감일)
//! - 결제일 = M월 payment_day일 (주말이면 다음 월요일)
//!
//! 로 계산한다.

use chrono::{Duration, NaiveDate};

use super::dates::{YearMonth, next_business_day};
use crate::entity::{enums::PaymentKind, payment_method};

/// 할부는 최대 60개월이라 청구 예정 금액을 계산할 때 이만큼 과거 거래까지 본다.
pub const INSTALLMENT_LOOKBACK_DAYS: i64 = 61 * 31;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BillingCycle {
    pub payment_day: u32,
    pub closing_day: u32,
    pub closing_month_offset: i32,
}

impl BillingCycle {
    /// 신용카드이고 정산 일정이 모두 입력된 경우에만 Some.
    pub fn from_model(pm: &payment_method::Model) -> Option<Self> {
        if pm.kind != PaymentKind::CreditCard {
            return None;
        }
        let cycle = Self {
            payment_day: u32::try_from(pm.payment_day?).ok()?,
            closing_day: u32::try_from(pm.closing_day?).ok()?,
            closing_month_offset: pm.closing_month_offset?,
        };
        cycle.validate().ok().map(|_| cycle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(1..=31).contains(&self.payment_day) {
            return Err("결제일은 1~31 사이여야 합니다".into());
        }
        if !(1..=31).contains(&self.closing_day) {
            return Err("마감일은 1~31 사이여야 합니다".into());
        }
        if !(-2..=0).contains(&self.closing_month_offset) {
            return Err("마감월은 전전월(-2), 전월(-1), 당월(0) 중 하나여야 합니다".into());
        }
        if self.closing_month_offset == 0 && self.closing_day >= self.payment_day {
            return Err("당월 마감이면 마감일이 결제일보다 앞서야 합니다".into());
        }
        Ok(())
    }

    /// 카드사 일반 기준(결제일 약 2주 전 마감)으로 추정한 정산 일정.
    /// 예: 1일 결제 → 전월 17일 마감, 14일 결제 → 전월 말일 마감, 25일 결제 → 당월 11일 마감.
    /// 카드사마다 하루 이틀씩 다를 수 있어 명세서로 확인해야 한다.
    pub fn suggested(payment_day: u32) -> Self {
        let payment_day = payment_day.clamp(1, 31);
        if payment_day > 14 {
            Self { payment_day, closing_day: payment_day - 14, closing_month_offset: 0 }
        } else if payment_day == 14 {
            Self { payment_day, closing_day: 31, closing_month_offset: -1 }
        } else {
            Self { payment_day, closing_day: 16 + payment_day, closing_month_offset: -1 }
        }
    }

    /// 결제월 `statement` 청구서의 이용기간 마감일.
    pub fn closing_date(&self, statement: YearMonth) -> NaiveDate {
        statement.add_months(self.closing_month_offset).day(self.closing_day)
    }

    /// 결제월 `statement` 청구서의 이용기간 (시작일, 마감일).
    pub fn period(&self, statement: YearMonth) -> (NaiveDate, NaiveDate) {
        (self.closing_date(statement.prev()) + Duration::days(1), self.closing_date(statement))
    }

    /// 실제 출금일 (주말이면 다음 영업일).
    pub fn payment_date(&self, statement: YearMonth) -> NaiveDate {
        next_business_day(statement.day(self.payment_day))
    }

    /// `date`에 쓴 금액이 청구되는 결제월.
    pub fn statement_for(&self, date: NaiveDate) -> YearMonth {
        let mut m = YearMonth::of(date).add_months(-self.closing_month_offset);
        loop {
            if date > self.closing_date(m) {
                m = m.next();
            } else if date <= self.closing_date(m.prev()) {
                m = m.prev();
            } else {
                return m;
            }
        }
    }

    /// 오늘 기준 다음 결제 청구서 (결제일이 오늘 이후인 첫 청구서).
    pub fn next_statement(&self, today: NaiveDate) -> YearMonth {
        // 전전월 마감 카드는 오늘 이전 이용분의 결제일이 2~3달 뒤라서 넉넉히 앞에서부터 찾는다.
        let mut m = self.statement_for(today).add_months(-3);
        while self.payment_date(m) < today {
            m = m.next();
        }
        m
    }
}

/// 할부 금액 분할. 나머지는 첫 회차에 붙인다.
pub fn split_installments(amount: i64, months: i32) -> Vec<i64> {
    let n = months.max(1) as i64;
    let base = amount / n;
    let first = amount - base * (n - 1);
    std::iter::once(first).chain(std::iter::repeat_n(base, (n - 1) as usize)).collect()
}

/// 카드 이용 한 건.
#[derive(Clone, Debug)]
pub struct Charge {
    pub tx_id: i32,
    pub date: NaiveDate,
    /// 카드에 청구되는 금액 (환불은 음수)
    pub amount: i64,
    pub installment_months: i32,
    pub planned: bool,
}

/// 청구서에 들어가는 한 회차.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChargePart {
    pub tx_id: i32,
    pub statement: YearMonth,
    pub amount: i64,
    /// 1부터 시작하는 회차
    pub installment_no: i32,
    pub installment_months: i32,
    pub planned: bool,
}

pub fn charge_parts(cycle: &BillingCycle, charges: &[Charge]) -> Vec<ChargePart> {
    let mut out = Vec::new();
    for c in charges {
        let first = cycle.statement_for(c.date);
        let months = c.installment_months.max(1);
        for (i, amount) in split_installments(c.amount, months).into_iter().enumerate() {
            out.push(ChargePart {
                tx_id: c.tx_id,
                statement: first.add_months(i as i32),
                amount,
                installment_no: i as i32 + 1,
                installment_months: months,
                planned: c.planned,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn ym(y: i32, m: u32) -> YearMonth {
        YearMonth::new(y, m).unwrap()
    }

    #[test]
    fn same_month_closing() {
        // 25일 결제, 당월 11일 마감: 전월 12일 ~ 당월 11일
        let c = BillingCycle { payment_day: 25, closing_day: 11, closing_month_offset: 0 };
        assert!(c.validate().is_ok());
        assert_eq!(c.period(ym(2026, 10)), (d(2026, 9, 12), d(2026, 10, 11)));
        assert_eq!(c.statement_for(d(2026, 10, 11)), ym(2026, 10));
        assert_eq!(c.statement_for(d(2026, 10, 12)), ym(2026, 11));
        assert_eq!(c.statement_for(d(2026, 9, 12)), ym(2026, 10));
        assert_eq!(c.statement_for(d(2026, 9, 11)), ym(2026, 9));
        // 2026-10-25는 일요일 → 26일 출금
        assert_eq!(c.payment_date(ym(2026, 10)), d(2026, 10, 26));
    }

    #[test]
    fn previous_month_end_closing() {
        // 14일 결제, 전월 말일 마감: 전월 1일 ~ 말일
        let c = BillingCycle { payment_day: 14, closing_day: 31, closing_month_offset: -1 };
        assert_eq!(c.period(ym(2026, 3)), (d(2026, 2, 1), d(2026, 2, 28)));
        assert_eq!(c.statement_for(d(2026, 2, 28)), ym(2026, 3));
        assert_eq!(c.statement_for(d(2026, 3, 1)), ym(2026, 4));
        assert_eq!(c.statement_for(d(2025, 12, 31)), ym(2026, 1));
    }

    #[test]
    fn two_months_offset() {
        let c = BillingCycle { payment_day: 1, closing_day: 18, closing_month_offset: -1 };
        assert_eq!(c.period(ym(2026, 11)), (d(2026, 9, 19), d(2026, 10, 18)));
        assert_eq!(c.statement_for(d(2026, 10, 18)), ym(2026, 11));
        assert_eq!(c.statement_for(d(2026, 10, 19)), ym(2026, 12));
        let c2 = BillingCycle { payment_day: 1, closing_day: 20, closing_month_offset: -2 };
        assert_eq!(c2.statement_for(d(2026, 9, 20)), ym(2026, 11));
        assert_eq!(c2.statement_for(d(2026, 9, 21)), ym(2026, 12));
    }

    #[test]
    fn suggestions_are_valid() {
        for day in 1..=31 {
            let c = BillingCycle::suggested(day);
            assert!(c.validate().is_ok(), "{day}: {c:?}");
        }
        assert_eq!(BillingCycle::suggested(25), BillingCycle { payment_day: 25, closing_day: 11, closing_month_offset: 0 });
        assert_eq!(BillingCycle::suggested(14), BillingCycle { payment_day: 14, closing_day: 31, closing_month_offset: -1 });
        assert_eq!(BillingCycle::suggested(1), BillingCycle { payment_day: 1, closing_day: 17, closing_month_offset: -1 });
    }

    #[test]
    fn next_statement_after_today() {
        let c = BillingCycle { payment_day: 25, closing_day: 11, closing_month_offset: 0 };
        // 10/23: 10월 청구서(9/12~10/11)는 10/26 출금 예정
        assert_eq!(c.next_statement(d(2026, 10, 23)), ym(2026, 10));
        assert_eq!(c.next_statement(d(2026, 10, 27)), ym(2026, 11));
        // 전전월 마감: 11월 청구서(8/21~9/20)는 11/2(월) 출금 → 10/23 기준 다음 결제
        let c2 = BillingCycle { payment_day: 1, closing_day: 20, closing_month_offset: -2 };
        assert_eq!(c2.next_statement(d(2026, 10, 23)), ym(2026, 11));
    }

    #[test]
    fn installments() {
        assert_eq!(split_installments(100_000, 3), vec![33_334, 33_333, 33_333]);
        assert_eq!(split_installments(-90_000, 3), vec![-30_000, -30_000, -30_000]);
        assert_eq!(split_installments(5_000, 1), vec![5_000]);
        let c = BillingCycle { payment_day: 14, closing_day: 31, closing_month_offset: -1 };
        let parts = charge_parts(
            &c,
            &[
                Charge { tx_id: 1, date: d(2026, 9, 30), amount: 300_000, installment_months: 3, planned: false },
                Charge { tx_id: 2, date: d(2026, 10, 2), amount: 10_000, installment_months: 1, planned: true },
            ],
        );
        let sum = |m: YearMonth, planned: bool| -> i64 {
            parts.iter().filter(|p| p.statement == m && p.planned == planned).map(|p| p.amount).sum()
        };
        assert_eq!(sum(ym(2026, 10), false), 100_000);
        assert_eq!(sum(ym(2026, 11), false), 100_000);
        assert_eq!(sum(ym(2026, 11), true), 10_000);
        assert_eq!(sum(ym(2026, 12), false), 100_000);
        assert_eq!(parts.iter().filter(|p| p.tx_id == 1).map(|p| p.installment_no).collect::<Vec<_>>(), vec![1, 2, 3]);
    }
}
