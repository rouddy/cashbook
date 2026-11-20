//! 장부, 카테고리, 지출계획 항목, 구성원.

use std::collections::BTreeSet;

use async_graphql::{Context, InputObject, MaybeUndefined, Object, Result};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};

use super::{DeleteResult, db, lookup, trim_opt, user_error};
use crate::entity::{
    category,
    enums::{CategoryKind, PlanItemKind},
    ledger, plan_item, recurring_rule, transaction,
};

#[derive(Default)]
pub struct LedgerQuery;

#[Object]
impl LedgerQuery {
    /// 장부 목록
    async fn ledgers(&self, ctx: &Context<'_>) -> Result<Vec<ledger::Model>> {
        Ok(ledger::Entity::find()
            .order_by_asc(ledger::Column::SortOrder)
            .order_by_asc(ledger::Column::Id)
            .all(db(ctx))
            .await?)
    }

    /// 카테고리 목록 (장부 순서 → 정렬 순서)
    async fn categories(
        &self,
        ctx: &Context<'_>,
        ledger_id: Option<i32>,
        #[graphql(default = false)] include_archived: bool,
    ) -> Result<Vec<category::Model>> {
        let mut q = category::Entity::find();
        if let Some(id) = ledger_id {
            q = q.filter(category::Column::LedgerId.eq(id));
        }
        if !include_archived {
            q = q.filter(category::Column::Archived.eq(false));
        }
        Ok(q.order_by_asc(category::Column::LedgerId)
            .order_by_asc(category::Column::SortOrder)
            .order_by_asc(category::Column::Id)
            .all(db(ctx))
            .await?)
    }

    /// 지출계획 고정 항목 (배분/구성원 용돈)
    async fn plan_items(&self, ctx: &Context<'_>, ledger_id: Option<i32>) -> Result<Vec<plan_item::Model>> {
        let mut q = plan_item::Entity::find();
        if let Some(id) = ledger_id {
            q = q.filter(plan_item::Column::LedgerId.eq(id));
        }
        Ok(q.order_by_asc(plan_item::Column::LedgerId)
            .order_by_asc(plan_item::Column::SortOrder)
            .order_by_asc(plan_item::Column::Id)
            .all(db(ctx))
            .await?)
    }

    /// 거래·계획에 쓰인 구성원 이름
    async fn members(&self, ctx: &Context<'_>) -> Result<Vec<String>> {
        let db = db(ctx);
        let mut set: BTreeSet<String> = transaction::Entity::find()
            .select_only()
            .column(transaction::Column::Member)
            .filter(transaction::Column::Member.is_not_null())
            .distinct()
            .into_tuple::<Option<String>>()
            .all(db)
            .await?
            .into_iter()
            .flatten()
            .filter(|m| !m.trim().is_empty())
            .collect();
        for p in plan_item::Entity::find().filter(plan_item::Column::Member.is_not_null()).all(db).await? {
            if let Some(m) = p.member.filter(|m| !m.trim().is_empty()) {
                set.insert(m);
            }
        }
        Ok(set.into_iter().collect())
    }
}

#[derive(InputObject)]
pub struct LedgerInput {
    pub name: String,
    pub description: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(InputObject)]
pub struct CategoryInput {
    pub ledger_id: i32,
    pub name: String,
    #[graphql(default_with = "CategoryKind::Expense")]
    pub kind: CategoryKind,
    pub monthly_budget: Option<i64>,
    #[graphql(default)]
    pub is_fixed: bool,
    pub color: Option<String>,
}

#[derive(InputObject)]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub kind: Option<CategoryKind>,
    pub monthly_budget: MaybeUndefined<i64>,
    pub is_fixed: Option<bool>,
    pub color: MaybeUndefined<String>,
    pub archived: Option<bool>,
    pub sort_order: Option<i32>,
}

#[derive(InputObject)]
pub struct PlanItemInput {
    pub ledger_id: i32,
    pub kind: PlanItemKind,
    pub name: String,
    pub amount: i64,
    pub member: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Default)]
pub struct LedgerMutation;

#[Object]
impl LedgerMutation {
    async fn create_ledger(&self, ctx: &Context<'_>, input: LedgerInput) -> Result<ledger::Model> {
        let db = db(ctx);
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("장부 이름을 입력하세요"));
        }
        let count = ledger::Entity::find().count(db).await?;
        let txn = db.begin().await?;
        if input.is_default == Some(true) {
            ledger::Entity::update_many()
                .col_expr(ledger::Column::IsDefault, false.into())
                .exec(&txn)
                .await?;
        }
        let saved = ledger::ActiveModel {
            name: Set(name),
            description: Set(trim_opt(input.description)),
            is_default: Set(input.is_default.unwrap_or(count == 0)),
            sort_order: Set(count as i32),
            created_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn update_ledger(&self, ctx: &Context<'_>, id: i32, input: LedgerInput) -> Result<ledger::Model> {
        let db = db(ctx);
        let found = ledger::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
        let txn = db.begin().await?;
        if input.is_default == Some(true) {
            ledger::Entity::update_many()
                .col_expr(ledger::Column::IsDefault, false.into())
                .exec(&txn)
                .await?;
        }
        let mut active: ledger::ActiveModel = found.into();
        let name = input.name.trim().to_string();
        if !name.is_empty() {
            active.name = Set(name);
        }
        active.description = Set(trim_opt(input.description));
        if let Some(d) = input.is_default {
            active.is_default = Set(d);
        }
        let saved = active.update(&txn).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 거래(받은 이체 포함)가 없고, 다른 장부의 반복 이체가 받지 않는 장부만 지울 수 있다.
    async fn delete_ledger(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let db = db(ctx);
        let used = transaction::Entity::find()
            .filter(Condition::any().add(transaction::Column::LedgerId.eq(id)).add(transaction::Column::TransferLedgerId.eq(id)))
            .count(db)
            .await?;
        if used > 0 {
            return Err(user_error(format!("거래 {used}건(받은 이체 포함)이 있는 장부는 지울 수 없습니다")));
        }
        let incoming = recurring_rule::Entity::find()
            .filter(recurring_rule::Column::TransferLedgerId.eq(id))
            .count(db)
            .await?;
        if incoming > 0 {
            return Err(user_error(format!("이 장부로 보내는 반복 이체 {incoming}개를 먼저 지우세요")));
        }
        let res = ledger::Entity::delete_by_id(id).exec(db).await?;
        lookup(ctx).invalidate();
        Ok(res.rows_affected > 0)
    }

    async fn create_category(&self, ctx: &Context<'_>, input: CategoryInput) -> Result<category::Model> {
        let db = db(ctx);
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("카테고리 이름을 입력하세요"));
        }
        ledger::Entity::find_by_id(input.ledger_id).one(db).await?.ok_or_else(|| user_error("장부가 없습니다"))?;
        let dup = category::Entity::find()
            .filter(category::Column::LedgerId.eq(input.ledger_id))
            .filter(category::Column::Name.eq(name.as_str()))
            .one(db)
            .await?;
        if let Some(existing) = dup {
            if existing.archived {
                let mut active: category::ActiveModel = existing.into();
                active.archived = Set(false);
                let saved = active.update(db).await?;
                lookup(ctx).invalidate();
                return Ok(saved);
            }
            return Err(user_error("같은 이름의 카테고리가 이미 있습니다"));
        }
        let count = category::Entity::find()
            .filter(category::Column::LedgerId.eq(input.ledger_id))
            .count(db)
            .await?;
        let saved = category::ActiveModel {
            ledger_id: Set(input.ledger_id),
            name: Set(name),
            kind: Set(input.kind),
            monthly_budget: Set(input.monthly_budget),
            is_fixed: Set(input.is_fixed),
            color: Set(trim_opt(input.color)),
            sort_order: Set(count as i32),
            archived: Set(false),
            ..Default::default()
        }
        .insert(db)
        .await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn update_category(&self, ctx: &Context<'_>, id: i32, input: CategoryPatch) -> Result<category::Model> {
        let db = db(ctx);
        let found = category::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?;
        let mut active: category::ActiveModel = found.into();
        if let Some(name) = input.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            active.name = Set(name);
        }
        if let Some(kind) = input.kind {
            active.kind = Set(kind);
        }
        match input.monthly_budget {
            MaybeUndefined::Value(v) => active.monthly_budget = Set(Some(v)),
            MaybeUndefined::Null => active.monthly_budget = Set(None),
            MaybeUndefined::Undefined => {}
        }
        if let Some(v) = input.is_fixed {
            active.is_fixed = Set(v);
        }
        match input.color {
            MaybeUndefined::Value(v) => active.color = Set(trim_opt(Some(v))),
            MaybeUndefined::Null => active.color = Set(None),
            MaybeUndefined::Undefined => {}
        }
        if let Some(v) = input.archived {
            active.archived = Set(v);
        }
        if let Some(v) = input.sort_order {
            active.sort_order = Set(v);
        }
        let saved = active.update(db).await.map_err(|e| match e {
            sea_orm::DbErr::Exec(_) | sea_orm::DbErr::Query(_) => user_error("같은 이름의 카테고리가 이미 있습니다"),
            other => other.into(),
        })?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 거래나 반복 규칙에 쓰인 카테고리는 보관 처리하고, 아니면 지운다.
    async fn delete_category(&self, ctx: &Context<'_>, id: i32) -> Result<DeleteResult> {
        let db = db(ctx);
        let found = category::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("카테고리가 없습니다"))?;
        let used = transaction::Entity::find().filter(transaction::Column::CategoryId.eq(id)).count(db).await?
            + recurring_rule::Entity::find().filter(recurring_rule::Column::CategoryId.eq(id)).count(db).await?;
        let result = if used > 0 {
            let mut active: category::ActiveModel = found.into();
            active.archived = Set(true);
            active.update(db).await?;
            DeleteResult { deleted: false, archived: true }
        } else {
            category::Entity::delete_by_id(id).exec(db).await?;
            DeleteResult { deleted: true, archived: false }
        };
        lookup(ctx).invalidate();
        Ok(result)
    }

    /// 주어진 순서대로 정렬 순서를 다시 매긴다.
    async fn reorder_categories(&self, ctx: &Context<'_>, ids: Vec<i32>) -> Result<bool> {
        let db = db(ctx);
        let txn = db.begin().await?;
        for (i, id) in ids.iter().enumerate() {
            category::Entity::update_many()
                .col_expr(category::Column::SortOrder, (i as i32).into())
                .filter(category::Column::Id.eq(*id))
                .exec(&txn)
                .await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(true)
    }

    async fn create_plan_item(&self, ctx: &Context<'_>, input: PlanItemInput) -> Result<plan_item::Model> {
        let db = db(ctx);
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("항목 이름을 입력하세요"));
        }
        if input.kind == PlanItemKind::MemberAllowance && trim_opt(input.member.clone()).is_none() {
            return Err(user_error("구성원 용돈은 구성원을 지정해야 합니다"));
        }
        let count = plan_item::Entity::find()
            .filter(plan_item::Column::LedgerId.eq(input.ledger_id))
            .count(db)
            .await?;
        Ok(plan_item::ActiveModel {
            ledger_id: Set(input.ledger_id),
            kind: Set(input.kind),
            name: Set(name),
            amount: Set(input.amount),
            member: Set(trim_opt(input.member)),
            sort_order: Set(input.sort_order.unwrap_or(count as i32)),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    async fn update_plan_item(&self, ctx: &Context<'_>, id: i32, input: PlanItemInput) -> Result<plan_item::Model> {
        let db = db(ctx);
        let found = plan_item::Entity::find_by_id(id).one(db).await?.ok_or_else(|| user_error("항목이 없습니다"))?;
        if input.kind == PlanItemKind::MemberAllowance && trim_opt(input.member.clone()).is_none() {
            return Err(user_error("구성원 용돈은 구성원을 지정해야 합니다"));
        }
        let mut active: plan_item::ActiveModel = found.into();
        active.ledger_id = Set(input.ledger_id);
        active.kind = Set(input.kind);
        let name = input.name.trim().to_string();
        if !name.is_empty() {
            active.name = Set(name);
        }
        active.amount = Set(input.amount);
        active.member = Set(trim_opt(input.member));
        if let Some(s) = input.sort_order {
            active.sort_order = Set(s);
        }
        Ok(active.update(db).await?)
    }

    async fn delete_plan_item(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        Ok(plan_item::Entity::delete_by_id(id).exec(db(ctx)).await?.rows_affected > 0)
    }
}
