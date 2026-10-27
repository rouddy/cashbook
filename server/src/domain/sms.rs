//! 카드 승인 문자/푸시 알림 원문에서 금액, 가맹점, 일시, 할부, 취소 여부, 카드사, 카드번호 끝자리를 뽑는다.
//!
//! 카드사마다 형식이 조금씩 다르지만 대부분
//! `[카드사]카드(1234)승인 홍*동 12,500원 일시불 10/23 12:34 가맹점 누적 1,234,567원`
//! 처럼 "금액 → 할부 → 월/일 시:분 → 가맹점 → 누적" 순서이거나 같은 순서로 줄바꿈되어 온다.
//! 확실하지 않은 값은 None으로 두고, 호출하는 쪽(자동 입력 API)이 명시 필드로 보완한다.

use std::sync::LazyLock;

use async_graphql::SimpleObject;
use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;
use serde::Serialize;

#[derive(SimpleObject, Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMessage {
    pub amount: Option<i64>,
    pub merchant: Option<String>,
    pub date: Option<NaiveDate>,
    /// "HH:MM"
    pub time: Option<String>,
    pub installment_months: Option<i32>,
    pub cancel: bool,
    /// 카드사 (예: 신한, KB국민)
    pub issuer: Option<String>,
    pub last4: Option<String>,
}

static AMOUNT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3}(?:,\d{3})+|\d+)\s*원").unwrap());
static DATETIME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{1,2})[/.\-](\d{1,2})\s*\(?[월화수목금토일]?\)?\s*(\d{1,2}):(\d{2})(?::\d{2})?").unwrap()
});
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\d,])(\d{1,2})/(\d{1,2})(?:[^\d]|$)").unwrap());
static INSTALLMENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,2})\s*개월").unwrap());
// "신한카드", "KB국민체크", "삼성1234승인", "하나(1*2*)승인", "롯데 승인"처럼 카드사 뒤에 카드/체크/번호/승인이 오는 경우
static ISSUER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(KB국민|국민|KB|신한|현대|삼성|롯데|하나|우리|BC|비씨|NH농협|NH|농협|씨티|IBK|기업|카카오뱅크|토스|케이뱅크)(?:\s?카드|\s?체크|\d{4}|\(|\s?승인)",
    )
    .unwrap()
});
// 취소 문자: "승인취소", "취소승인", "결제취소", "[취소]", "취소 12,500원" 등. 가맹점 이름의 "취소"는 제외
static CANCEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"승인\s*취소|취소\s*승인|(?:결제|사용|매출|거래)\s*취소|\[취소\]|\(취소\)|^\s*취소|취소\s*[\d,]+\s*원|[\d,]+\s*원\s*\(?취소")
        .unwrap()
});
static LAST4_PAREN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d{4})\)").unwrap());
static LAST4_AFTER_CARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:카드|체크)\s?(\d{4})(?:\s|승인|$)").unwrap());
static LAST4_BEFORE_APPROVAL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\d,])(\d{4})\s?승인").unwrap());
static MASKED_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[가-힣A-Za-z]\*[가-힣A-Za-z*]*(님)?$").unwrap());

/// 누적·잔액 등 결제 금액이 아닌 금액 앞에 오는 말
const NON_PAYMENT_PREFIXES: [&str; 5] = ["누적", "잔액", "잔여", "한도", "포인트"];
/// 가맹점 뒤에 붙는 꼬리 (여기서 자른다)
const MERCHANT_STOPS: [&str; 6] = ["누적", "잔액", "잔여", "사용가능", "포인트", "[Web발신]"];

fn canonical_issuer(raw: &str) -> &'static str {
    match raw {
        "KB국민" | "국민" | "KB" => "KB국민",
        "신한" => "신한",
        "현대" => "현대",
        "삼성" => "삼성",
        "롯데" => "롯데",
        "하나" => "하나",
        "우리" => "우리",
        "BC" | "비씨" => "BC",
        "NH농협" | "NH" | "농협" => "NH농협",
        "씨티" => "씨티",
        "IBK" | "기업" => "IBK기업",
        "카카오뱅크" => "카카오뱅크",
        "토스" => "토스",
        "케이뱅크" => "케이뱅크",
        _ => "",
    }
}

fn parse_amount(text: &str) -> Option<i64> {
    for m in AMOUNT_RE.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let before: String = text[..whole.start()].chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
        if NON_PAYMENT_PREFIXES.iter().any(|p| before.contains(p)) {
            continue;
        }
        let digits: String = m[1].chars().filter(char::is_ascii_digit).collect();
        if let Ok(v) = digits.parse::<i64>()
            && v > 0 {
                return Some(v);
            }
    }
    None
}

fn infer_date(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    let date = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    // 1월에 받은 12/31 승인 문자처럼 미래로 보이면 작년으로 본다.
    if date > today + Duration::days(7) {
        NaiveDate::from_ymd_opt(today.year() - 1, month, day)
    } else {
        Some(date)
    }
}

fn clean_merchant(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    for stop in MERCHANT_STOPS {
        if let Some(i) = s.find(stop) {
            s = &s[..i];
        }
    }
    let s = s
        .trim()
        .trim_start_matches(|c: char| c == '-' || c == ':' || c == ',' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '-' || c == ',' || c.is_whitespace());
    let s = s.strip_suffix("에서").unwrap_or(s).trim();
    if s.is_empty() || AMOUNT_RE.is_match(s) && parse_amount(s).is_some() && s.ends_with('원') {
        return None;
    }
    Some(s.chars().take(100).collect())
}

/// 가맹점으로 보기 어려운 줄 (금액, 할부, 이름, 카드사 머리글 등)
fn is_noise_line(line: &str) -> bool {
    let l = line.trim();
    l.is_empty()
        || l.starts_with('[') && l.ends_with(']')
        || MERCHANT_STOPS.iter().any(|s| l.starts_with(s))
        || l.contains("승인")
        || ISSUER_RE.is_match(l)
        || MASKED_NAME_RE.is_match(l)
        || l == "일시불"
        || INSTALLMENT_RE.is_match(l) && l.len() <= 12
        || AMOUNT_RE.is_match(l) && l.ends_with('원')
        || DATETIME_RE.is_match(l) && DATETIME_RE.replace(l, "").trim().is_empty()
}

pub fn parse(text: &str, today: NaiveDate) -> ParsedMessage {
    let text = text.replace("[Web발신]", "\n").replace('\r', "");
    let mut out = ParsedMessage {
        cancel: text.lines().any(|l| CANCEL_RE.is_match(l)),
        amount: parse_amount(&text),
        ..Default::default()
    };

    if let Some(m) = ISSUER_RE.captures(&text) {
        let issuer = canonical_issuer(&m[1]);
        if !issuer.is_empty() {
            out.issuer = Some(issuer.to_string());
        }
    }
    out.last4 = LAST4_PAREN_RE
        .captures(&text)
        .or_else(|| LAST4_AFTER_CARD_RE.captures(&text))
        .or_else(|| LAST4_BEFORE_APPROVAL_RE.captures(&text))
        .map(|c| c[1].to_string());

    if text.contains("일시불") {
        out.installment_months = Some(1);
    }
    if let Some(c) = INSTALLMENT_RE.captures(&text)
        && let Ok(n) = c[1].parse::<i32>()
            && (1..=60).contains(&n) {
                out.installment_months = Some(n);
            }

    if let Some(c) = DATETIME_RE.captures(&text) {
        let (month, day): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
        out.date = infer_date(month, day, today);
        out.time = Some(format!("{:02}:{}", c[3].parse::<u32>().unwrap_or(0), &c[4]));
        // 같은 줄의 시각 뒤 → 없으면 다음 줄들 중 첫 번째 의미 있는 줄
        let end = c.get(0).unwrap().end();
        let rest_of_line = text[end..].split('\n').next().unwrap_or("");
        out.merchant = clean_merchant(rest_of_line).filter(|m| !is_noise_line(m));
        if out.merchant.is_none() {
            out.merchant = text[end..]
                .split('\n')
                .skip(1)
                .map(str::trim)
                .find(|l| !is_noise_line(l))
                .and_then(clean_merchant);
        }
    } else if let Some(c) = DATE_RE.captures(&text) {
        out.date = infer_date(c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0), today);
    }

    if out.merchant.is_none() {
        // 시각이 없는 형식: 의미 있는 줄 중 마지막 (보통 가맹점이 금액 뒤에 온다)
        out.merchant = text
            .split('\n')
            .map(str::trim)
            .filter(|l| !is_noise_line(l)).rfind(|l| !DATE_RE.is_match(l))
            .and_then(clean_merchant);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()
    }

    fn d(m: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, m, day)
    }

    #[test]
    fn shinhan_single_line() {
        let p = parse("[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원", today());
        assert_eq!(p.amount, Some(12_500));
        assert_eq!(p.merchant.as_deref(), Some("스타벅스"));
        assert_eq!(p.date, d(10, 23));
        assert_eq!(p.time.as_deref(), Some("12:34"));
        assert_eq!(p.installment_months, Some(1));
        assert_eq!(p.issuer.as_deref(), Some("신한"));
        assert_eq!(p.last4.as_deref(), Some("1234"));
        assert!(!p.cancel);
    }

    #[test]
    fn kb_multi_line_installment() {
        let p = parse(
            "[Web발신]\nKB국민카드1234승인\n홍*동님\n1,200,000원 03개월\n10/22 15:00\n애플스토어 가로수길\n누적1,500,000원",
            today(),
        );
        assert_eq!(p.amount, Some(1_200_000));
        assert_eq!(p.installment_months, Some(3));
        assert_eq!(p.merchant.as_deref(), Some("애플스토어 가로수길"));
        assert_eq!(p.issuer.as_deref(), Some("KB국민"));
        assert_eq!(p.last4.as_deref(), Some("1234"));
        assert_eq!(p.date, d(10, 22));
    }

    #[test]
    fn hyundai_multi_line() {
        let p = parse("현대카드 M 승인\n홍*동\n45,000원 일시불\n10/23 18:20\n이마트 성수점\n누적 456,780원", today());
        assert_eq!(p.amount, Some(45_000));
        assert_eq!(p.merchant.as_deref(), Some("이마트 성수점"));
        assert_eq!(p.issuer.as_deref(), Some("현대"));
        assert_eq!(p.last4, None);
    }

    #[test]
    fn hana_single_line_with_space_before_total() {
        let p = parse("하나카드(5678)승인 홍*동님 8,900원 일시불 10/23 08:15 GS25역삼점 누적 300,000원", today());
        assert_eq!(p.amount, Some(8_900));
        assert_eq!(p.merchant.as_deref(), Some("GS25역삼점"));
        assert_eq!(p.issuer.as_deref(), Some("하나"));
        assert_eq!(p.last4.as_deref(), Some("5678"));
    }

    #[test]
    fn cancellation() {
        let p = parse("[Web발신]\n신한카드(1234)승인취소 홍*동 12,500원(일시불)10/23 12:40 스타벅스", today());
        assert!(p.cancel);
        assert_eq!(p.amount, Some(12_500));
        assert_eq!(p.merchant.as_deref(), Some("스타벅스"));
    }

    #[test]
    fn year_rollover() {
        let jan = NaiveDate::from_ymd_opt(2027, 1, 2).unwrap();
        let p = parse("우리카드(1111)승인 홍*동님 10,000원 일시불 12/31 23:50 편의점", jan);
        assert_eq!(p.date, NaiveDate::from_ymd_opt(2026, 12, 31));
    }

    #[test]
    fn issuer_without_card_word_and_seconds() {
        let p = parse("삼성1234승인 홍*동\n12,000원 일시불\n10/23 23:50:12 편의점\n누적 100,000원", today());
        assert_eq!(p.issuer.as_deref(), Some("삼성"));
        assert_eq!(p.last4.as_deref(), Some("1234"));
        assert_eq!(p.merchant.as_deref(), Some("편의점"));
        assert_eq!(p.time.as_deref(), Some("23:50"));
        let masked = parse("하나(1*2*)승인 홍*동님 8,000원 일시불 10/23 08:00 김밥", today());
        assert_eq!(masked.issuer.as_deref(), Some("하나"));
        assert_eq!(masked.last4, None);
        // "하나로마트"는 카드사가 아니다
        assert_eq!(parse("10/23 12:00 하나로마트 5,000원", today()).issuer, None);
    }

    #[test]
    fn cancel_word_in_merchant_is_not_refund() {
        let p = parse("신한카드(1234)승인 홍*동 30,000원(일시불)10/23 12:34 취소없는여행사 누적1,000원", today());
        assert!(!p.cancel);
        assert!(parse("[취소] 우리카드(1111) 12,000원 10/23 12:00 편의점", today()).cancel);
        assert!(parse("KB국민카드1234 결제취소\n9,000원\n10/23 09:10\n김밥천국", today()).cancel);
    }

    #[test]
    fn total_amount_is_not_payment() {
        assert_eq!(parse_amount("누적 1,234,567원 결제 3,000원"), Some(3_000));
        assert_eq!(parse_amount("잔액 50,000원"), None);
    }
}
