//! 반복 규칙 → 예정(PLANNED) 거래 생성.
//!
//! 이번 달부터 `months_ahead`개월 뒤까지, 규칙마다 (규칙, 월) 하나씩 예정 거래를 만든다.
//! (recurring_rule_id, recurring_month) 유니크 인덱스로 중복을 막고, 건너뛴 달은 SKIPPED로 남겨 다시 만들지 않는다.

use std::collections::HashSet;

use chrono::{NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect, Set,
};

use super::dates::YearMonth;
use crate::entity::{
    enums::{TxSource, TxStatus},
    recurring_rule, transaction,
};

pub fn occurs_in(rule: &recurring_rule::Model, month: YearMonth) -> bool {
    let Some(start) = YearMonth::parse(&rule.start_month) else {
        return false;
    };
    if month < start {
        return false;
    }
    if let Some(end) = rule.end_month.as_deref().and_then(YearMonth::parse)
        && month > end {
            return false;
        }
    start.months_until(month) % rule.interval_months.max(1) == 0
}

/// 활성 규칙의 예정 거래를 채운다. 새로 만든 개수를 돌려준다.
pub async fn ensure_planned<C: ConnectionTrait>(
    db: &C,
    today: NaiveDate,
    months_ahead: u32,
    only_rule: Option<i32>,
) -> Result<usize, DbErr> {
    let first = YearMonth::of(today);
    let last = first.add_months(months_ahead as i32);
    let mut query = recurring_rule::Entity::find().filter(recurring_rule::Column::Active.eq(true));
    if let Some(id) = only_rule {
        query = query.filter(recurring_rule::Column::Id.eq(id));
    }
    let rules = query.all(db).await?;
    if rules.is_empty() {
        return Ok(0);
    }
    let existing: HashSet<(i32, String)> = transaction::Entity::find()
        .select_only()
        .column(transaction::Column::RecurringRuleId)
        .column(transaction::Column::RecurringMonth)
        .filter(transaction::Column::RecurringRuleId.is_not_null())
        .filter(transaction::Column::RecurringMonth.gte(first.to_string()))
        .into_tuple::<(Option<i32>, Option<String>)>()
        .all(db)
        .await?
        .into_iter()
        .filter_map(|(rule, month)| Some((rule?, month?)))
        .collect();

    let now = Utc::now();
    let mut created = 0;
    for rule in &rules {
        for month in first.range_inclusive(last) {
            if !occurs_in(rule, month) || existing.contains(&(rule.id, month.to_string())) {
                continue;
            }
            transaction::ActiveModel {
                ledger_id: Set(rule.ledger_id),
                date: Set(month.day(rule.day_of_month.max(1) as u32)),
                amount: Set(rule.amount),
                category_id: Set(rule.category_id),
                payment_method_id: Set(rule.payment_method_id),
                description: Set(rule.description.clone()),
                memo: Set(rule.memo.clone()),
                member: Set(rule.member.clone()),
                status: Set(TxStatus::Planned),
                installment_months: Set(1),
                exclude_from_performance: Set(rule.exclude_from_performance),
                recurring_rule_id: Set(Some(rule.id)),
                recurring_month: Set(Some(month.to_string())),
                transfer_ledger_id: Set(rule.transfer_ledger_id),
                source: Set(TxSource::Recurring),
                external_id: Set(None),
                raw_text: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
            created += 1;
        }
    }
    Ok(created)
}

/// 규칙이 바뀌었을 때: 이번 달 이후 아직 예정 상태인 항목을 지우고 다시 만든다.
/// 확정/건너뜀 처리된 항목은 그대로 둔다.
pub async fn regenerate<C: ConnectionTrait>(
    db: &C,
    rule_id: i32,
    today: NaiveDate,
    months_ahead: u32,
) -> Result<usize, DbErr> {
    remove_future_planned(db, rule_id, today).await?;
    ensure_planned(db, today, months_ahead, Some(rule_id)).await
}

pub async fn remove_future_planned<C: ConnectionTrait>(db: &C, rule_id: i32, today: NaiveDate) -> Result<u64, DbErr> {
    let res = transaction::Entity::delete_many()
        .filter(transaction::Column::RecurringRuleId.eq(rule_id))
        .filter(transaction::Column::Status.eq(TxStatus::Planned))
        .filter(transaction::Column::RecurringMonth.gte(YearMonth::of(today).to_string()))
        .exec(db)
        .await?;
    Ok(res.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(start: &str, end: Option<&str>, interval: i32) -> recurring_rule::Model {
        recurring_rule::Model {
            id: 1,
            ledger_id: 1,
            description: "보험".into(),
            amount: 10_000,
            category_id: None,
            payment_method_id: None,
            day_of_month: 31,
            interval_months: interval,
            start_month: start.into(),
            end_month: end.map(str::to_string),
            member: None,
            memo: None,
            match_keyword: None,
            match_amount_min: None,
            match_amount_max: None,
            exclude_from_performance: false,
            transfer_ledger_id: None,
            active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn ym(s: &str) -> YearMonth {
        YearMonth::parse(s).unwrap()
    }

    #[test]
    fn occurrence_rules() {
        let monthly = rule("2026-09", None, 1);
        assert!(!occurs_in(&monthly, ym("2026-08")));
        assert!(occurs_in(&monthly, ym("2026-09")));
        assert!(occurs_in(&monthly, ym("2027-03")));
        let yearly = rule("2026-03", Some("2028-12"), 12);
        assert!(occurs_in(&yearly, ym("2027-03")));
        assert!(!occurs_in(&yearly, ym("2027-04")));
        assert!(!occurs_in(&yearly, ym("2029-03")));
    }

    #[tokio::test]
    async fn generates_once_per_month_and_respects_skips() {
        let db = crate::db::connect_in_memory().await;
        let now = Utc::now();
        crate::entity::ledger::ActiveModel {
            name: Set("가계부".into()),
            description: Set(None),
            is_default: Set(true),
            sort_order: Set(0),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let mut r = rule("2026-09", None, 1);
        r.id = 0;
        let active: recurring_rule::ActiveModel = r.into();
        let mut active = active;
        active.id = sea_orm::ActiveValue::NotSet;
        let saved = active.insert(&db).await.unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert_eq!(ensure_planned(&db, today, 2, None).await.unwrap(), 3);
        assert_eq!(ensure_planned(&db, today, 2, None).await.unwrap(), 0);
        // 11월 항목을 건너뛰면 다시 생성되지 않는다
        let nov = transaction::Entity::find()
            .filter(transaction::Column::RecurringMonth.eq("2026-11"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(nov.date, NaiveDate::from_ymd_opt(2026, 11, 30).unwrap());
        let mut skip: transaction::ActiveModel = nov.into();
        skip.status = Set(TxStatus::Skipped);
        skip.update(&db).await.unwrap();
        assert_eq!(regenerate(&db, saved.id, today, 2).await.unwrap(), 2);
        let count = transaction::Entity::find()
            .filter(transaction::Column::RecurringRuleId.eq(saved.id))
            .all(&db)
            .await
            .unwrap()
            .len();
        assert_eq!(count, 3);
    }
}
