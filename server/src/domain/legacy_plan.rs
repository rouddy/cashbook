//! 예전 지출계획(월 수입·배분 고정 항목, '합계 제외' 카테고리)을 장부·반복 규칙으로 옮기는 규칙.
//! DB 마이그레이션(`m20261120_000007_convert_legacy_plan`)과 스프레드시트 가져오기가 같은 규칙을 쓴다.
//!
//! - 합계 제외 카테고리 → 같은 이름의 장부로 (카테고리·거래·반복 규칙째). 마이그레이션·가져오기 쪽에서 처리한다.
//! - 배분 항목 → 이름이 맞는 다른 장부로 보내는 매월 이체 ([`transfer_target`]). 맞는 장부가 없으면(저축 등) 배분으로 남긴다.
//! - 월 수입 항목 → 그 장부의 '정기수익' 수입 항목 + 매월 반복 수입. 단 이체를 받게 된 장부에서
//!   월 수입 합계가 받는 이체 합계와 같으면(용돈 장부의 용돈) 이체가 그 수입을 대신하므로 지운다.
//! - 구성원 용돈은 그대로 둔다.

use std::collections::HashMap;

/// 정기 수입(월급 등)을 담는 수입 항목 이름
pub const REGULAR_INCOME_CATEGORY: &str = "정기수익";
/// 옮긴 월 수입의 반복 날짜
pub const INCOME_DAY: i32 = 10;
/// 옮긴 장부 이체의 반복 날짜
pub const TRANSFER_DAY: i32 = 1;

pub struct LedgerInfo {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LegacyKind {
    Income,
    Allocation,
    /// 구성원 용돈 등 그대로 두는 항목
    Other,
}

pub struct LegacyItem {
    pub id: i32,
    pub ledger_id: i32,
    pub kind: LegacyKind,
    pub name: String,
    pub amount: i64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Conversion {
    /// 그 장부로 보내는 매월 이체
    Transfer { to: i32 },
    /// 정기수익 항목의 매월 반복 수입
    RegularIncome,
    /// 받는 이체가 대신하므로 지운다
    ReplacedByTransfer,
    /// 지출계획 항목으로 남긴다
    Keep,
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 배분 항목 이름으로 받을 장부를 찾는다 (항목이 있는 장부는 빼고).
/// 이름이 같은 장부 → 설명이 같은 장부 → 장부 이름(2자 이상)으로 시작하거나 끝나는 장부 순이고, 단계마다 하나뿐일 때만 고른다.
/// 예: '경조사비' → 경조사비, '재홍 용돈' → 설명이 '재홍 용돈'인 용돈, '여행적금' → 여행, '저축' → 없음
pub fn transfer_target(ledgers: &[LedgerInfo], item: &LegacyItem) -> Option<i32> {
    let name = norm(&item.name);
    if name.is_empty() {
        return None;
    }
    let others: Vec<&LedgerInfo> = ledgers.iter().filter(|l| l.id != item.ledger_id).collect();
    let unique = |matches: Vec<&&LedgerInfo>| (matches.len() == 1).then(|| matches[0].id);
    let by_name = others.iter().filter(|l| norm(&l.name) == name).collect();
    if let Some(id) = unique(by_name) {
        return Some(id);
    }
    let by_description = others.iter().filter(|l| l.description.as_deref().is_some_and(|d| norm(d) == name)).collect();
    if let Some(id) = unique(by_description) {
        return Some(id);
    }
    let by_affix = others
        .iter()
        .filter(|l| {
            let n = norm(&l.name);
            n.chars().count() >= 2 && (name.starts_with(&n) || name.ends_with(&n))
        })
        .collect();
    unique(by_affix)
}

/// 항목마다 어떻게 옮길지 정한다 (입력 순서대로).
pub fn convert(ledgers: &[LedgerInfo], items: &[LegacyItem]) -> Vec<(i32, Conversion)> {
    let mut out: Vec<(i32, Conversion)> = Vec::with_capacity(items.len());
    let mut received: HashMap<i32, i64> = HashMap::new();
    for item in items {
        let c = match item.kind {
            LegacyKind::Allocation => match transfer_target(ledgers, item) {
                Some(to) => {
                    *received.entry(to).or_default() += item.amount;
                    Conversion::Transfer { to }
                }
                None => Conversion::Keep,
            },
            LegacyKind::Income => Conversion::RegularIncome,
            LegacyKind::Other => Conversion::Keep,
        };
        out.push((item.id, c));
    }
    // 이체를 받게 된 장부의 월 수입이 받는 이체와 같으면 이체가 대신한다
    let mut income_total: HashMap<i32, i64> = HashMap::new();
    for item in items.iter().filter(|i| i.kind == LegacyKind::Income) {
        *income_total.entry(item.ledger_id).or_default() += item.amount;
    }
    for (id, c) in out.iter_mut() {
        if *c != Conversion::RegularIncome {
            continue;
        }
        let ledger = items.iter().find(|i| i.id == *id).map(|i| i.ledger_id);
        if let Some(l) = ledger
            && received.get(&l).is_some_and(|r| income_total.get(&l) == Some(r))
        {
            *c = Conversion::ReplacedByTransfer;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(id: i32, name: &str, description: Option<&str>) -> LedgerInfo {
        LedgerInfo { id, name: name.into(), description: description.map(str::to_string) }
    }

    fn item(id: i32, ledger_id: i32, kind: LegacyKind, name: &str, amount: i64) -> LegacyItem {
        LegacyItem { id, ledger_id, kind, name: name.into(), amount }
    }

    #[test]
    fn converts_sheet_plan() {
        let ledgers = [
            ledger(1, "가계부", Some("월급사용내역 (생활비)")),
            ledger(2, "용돈", Some("재홍 용돈")),
            ledger(3, "경조사비", None),
            ledger(4, "여행", None),
        ];
        let items = [
            item(1, 1, LegacyKind::Income, "월급", 4_500_000),
            item(2, 1, LegacyKind::Allocation, "재홍 용돈", 200_000),
            item(3, 1, LegacyKind::Other, "송화 용돈", 500_000),
            item(4, 1, LegacyKind::Allocation, "경조사비", 250_000),
            item(5, 1, LegacyKind::Allocation, "저축", 100_000),
            item(6, 1, LegacyKind::Allocation, "여행적금", 250_000),
            item(7, 2, LegacyKind::Income, "용돈", 200_000),
        ];
        assert_eq!(
            convert(&ledgers, &items),
            vec![
                (1, Conversion::RegularIncome),
                (2, Conversion::Transfer { to: 2 }),
                (3, Conversion::Keep),
                (4, Conversion::Transfer { to: 3 }),
                (5, Conversion::Keep),
                (6, Conversion::Transfer { to: 4 }),
                (7, Conversion::ReplacedByTransfer),
            ]
        );
    }

    #[test]
    fn ambiguous_or_own_ledger_is_not_a_target() {
        let ledgers = [ledger(1, "가계부", None), ledger(2, "여행", None), ledger(3, "여행비", None), ledger(4, "용돈", None)];
        // 여행으로 시작하는 장부가 둘이면 고르지 않는다
        assert_eq!(transfer_target(&ledgers, &item(1, 1, LegacyKind::Allocation, "여행비적금", 1)), None);
        // 이름이 같은 장부가 있으면 그것
        assert_eq!(transfer_target(&ledgers, &item(1, 1, LegacyKind::Allocation, "여행 비", 1)), Some(3));
        // 자기 장부로는 보내지 않는다
        assert_eq!(transfer_target(&ledgers, &item(1, 4, LegacyKind::Allocation, "용돈", 1)), None);
    }

    #[test]
    fn income_differs_from_transfer_stays_income() {
        let ledgers = [ledger(1, "가계부", None), ledger(2, "용돈", None)];
        let items = [item(1, 1, LegacyKind::Allocation, "용돈", 200_000), item(2, 2, LegacyKind::Income, "알바", 300_000)];
        assert_eq!(convert(&ledgers, &items), vec![(1, Conversion::Transfer { to: 2 }), (2, Conversion::RegularIncome)]);
    }
}
