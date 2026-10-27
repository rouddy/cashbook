//! 월 단위 계산 도우미. 가계부는 대부분 "YYYY-MM" 월 단위로 묶어서 본다.

use std::fmt;

use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};

/// 연-월. GraphQL에서는 `"2026-10"` 형식의 `YearMonth` 스칼라.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    pub year: i32,
    pub month: u32,
}

impl YearMonth {
    pub fn new(year: i32, month: u32) -> Option<Self> {
        (1..=12).contains(&month).then_some(Self { year, month })
    }

    pub fn of(date: NaiveDate) -> Self {
        Self { year: date.year(), month: date.month() }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (y, m) = s.trim().split_once('-')?;
        if y.len() != 4 || m.is_empty() || m.len() > 2 {
            return None;
        }
        Self::new(y.parse().ok()?, m.parse().ok()?)
    }

    /// 1월 = 0 기준 일련 번호. 월 차이 계산용.
    fn index(self) -> i64 {
        self.year as i64 * 12 + (self.month as i64 - 1)
    }

    fn from_index(i: i64) -> Self {
        Self { year: i.div_euclid(12) as i32, month: (i.rem_euclid(12) + 1) as u32 }
    }

    pub fn add_months(self, n: i32) -> Self {
        Self::from_index(self.index() + n as i64)
    }

    pub fn next(self) -> Self {
        self.add_months(1)
    }

    pub fn prev(self) -> Self {
        self.add_months(-1)
    }

    /// `other - self` (개월)
    pub fn months_until(self, other: YearMonth) -> i32 {
        (other.index() - self.index()) as i32
    }

    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, 1).expect("valid month")
    }

    pub fn last_day(self) -> NaiveDate {
        self.next().first_day() - Duration::days(1)
    }

    pub fn days(self) -> u32 {
        self.last_day().day()
    }

    /// 해당 월의 `day`일. 월의 마지막 날을 넘으면 말일로 맞춘다 (31 → 2월 28일).
    pub fn day(self, day: u32) -> NaiveDate {
        let d = day.clamp(1, self.days());
        NaiveDate::from_ymd_opt(self.year, self.month, d).expect("clamped day")
    }

    pub fn contains(self, date: NaiveDate) -> bool {
        YearMonth::of(date) == self
    }

    /// `self..=end` 범위의 월 목록
    pub fn range_inclusive(self, end: YearMonth) -> impl Iterator<Item = YearMonth> {
        let (start, end) = (self.index(), end.index());
        (start..=end).map(YearMonth::from_index)
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

#[Scalar(name = "YearMonth")]
impl ScalarType for YearMonth {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => {
                YearMonth::parse(&s).ok_or_else(|| InputValueError::custom("YearMonth 형식은 YYYY-MM 입니다"))
            }
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

/// 설정된 시간대 기준 오늘 날짜.
pub fn today_in(tz: chrono_tz::Tz) -> NaiveDate {
    Utc::now().with_timezone(&tz).date_naive()
}

/// 주말이면 다음 영업일(월요일)로 미룬다. 공휴일은 고려하지 않는다.
pub fn next_business_day(date: NaiveDate) -> NaiveDate {
    match date.weekday() {
        Weekday::Sat => date + Duration::days(2),
        Weekday::Sun => date + Duration::days(1),
        _ => date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn month_arithmetic() {
        let ym = YearMonth::new(2026, 1).unwrap();
        assert_eq!(ym.add_months(-1), YearMonth::new(2025, 12).unwrap());
        assert_eq!(ym.add_months(13), YearMonth::new(2027, 2).unwrap());
        assert_eq!(ym.add_months(-13), YearMonth::new(2024, 12).unwrap());
        assert_eq!(ym.months_until(YearMonth::new(2026, 10).unwrap()), 9);
        assert_eq!(YearMonth::new(2026, 2).unwrap().day(31), d(2026, 2, 28));
        assert_eq!(YearMonth::new(2024, 2).unwrap().last_day(), d(2024, 2, 29));
        assert_eq!(YearMonth::parse("2026-10"), YearMonth::new(2026, 10));
        assert_eq!(YearMonth::parse("2026-1"), YearMonth::new(2026, 1));
        assert_eq!(YearMonth::parse("2026-13"), None);
        assert_eq!(YearMonth::parse("26-10"), None);
        assert_eq!(YearMonth::new(2026, 3).unwrap().to_string(), "2026-03");
        let months: Vec<_> = YearMonth::new(2025, 11)
            .unwrap()
            .range_inclusive(YearMonth::new(2026, 2).unwrap())
            .map(|m| m.to_string())
            .collect();
        assert_eq!(months, ["2025-11", "2025-12", "2026-01", "2026-02"]);
    }

    #[test]
    fn business_day() {
        assert_eq!(next_business_day(d(2026, 10, 24)), d(2026, 10, 26)); // 토
        assert_eq!(next_business_day(d(2026, 10, 25)), d(2026, 10, 26)); // 일
        assert_eq!(next_business_day(d(2026, 10, 23)), d(2026, 10, 23)); // 금
    }
}
