//! 은행 계좌·잔고와 여유자금.

use async_graphql::{ComplexObject, Context, InputObject, Object, Result};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
    sea_query::Query,
};

use super::{config, db, lookup, trim_opt, uid, user_error};
use crate::domain::billing::INSTALLMENT_LOOKBACK_DAYS;
use crate::domain::funds::{self, AvailableFunds, FundsInput};
use crate::entity::{
    OfUser, account, account_balance,
    enums::{AccountKind, TxStatus},
    payment_method, transaction,
};

#[ComplexObject]
impl account::Model {
    /// 잔고 기록 이력 (최근 순)
    async fn history(&self, ctx: &Context<'_>, #[graphql(default = 24)] limit: u64) -> Result<Vec<account_balance::Model>> {
        Ok(account_balance::Entity::find()
            .filter(account_balance::Column::AccountId.eq(self.id))
            .order_by_desc(account_balance::Column::BalanceDate)
            .order_by_desc(account_balance::Column::Id)
            .limit(limit.min(500))
            .all(db(ctx))
            .await?)
    }

    /// 이 계좌에서 출금되는 결제수단
    async fn payment_methods(&self, ctx: &Context<'_>) -> Result<Vec<payment_method::Model>> {
        let mut v: Vec<payment_method::Model> = lookup(ctx)
            .payment_methods()
            .await?
            .values()
            .filter(|p| p.account_id == Some(self.id))
            .cloned()
            .collect();
        v.sort_by_key(|p| (p.archived, p.sort_order, p.id));
        Ok(v)
    }
}

#[derive(Default)]
pub struct AccountQuery;

#[Object]
impl AccountQuery {
    async fn accounts(&self, ctx: &Context<'_>, #[graphql(default = false)] include_archived: bool) -> Result<Vec<account::Model>> {
        let uid = uid(ctx)?;
        let mut q = account::Entity::find().of_user(uid);
        if !include_archived {
            q = q.filter(account::Column::Archived.eq(false));
        }
        Ok(q.order_by_asc(account::Column::Archived)
            .order_by_asc(account::Column::SortOrder)
            .order_by_asc(account::Column::Id)
            .all(db(ctx))
            .await?)
    }

    async fn account(&self, ctx: &Context<'_>, id: i32) -> Result<Option<account::Model>> {
        let uid = uid(ctx)?;
        Ok(account::Entity::find_by_id(id).of_user(uid).one(db(ctx)).await?)
    }

    /// 여유자금 = 은행 잔고 − 잔고 기준일 이후 결제될 카드대금 − 잔고 반영 전 계좌 출금
    async fn available_funds(&self, ctx: &Context<'_>) -> Result<AvailableFunds> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        let today = config(ctx).today();
        let accounts = account::Entity::find().of_user(uid).all(db).await?;
        let lookup = lookup(ctx);
        let pms: Vec<payment_method::Model> = lookup.payment_methods().await?.values().cloned().collect();
        let categories = lookup.categories().await?;
        // 최대 60개월 할부의 남은 청구분까지 보려고 5년 전부터 읽는다.
        let txs = transaction::Entity::find()
            .of_user(uid)
            .filter(transaction::Column::Status.ne(TxStatus::Skipped))
            .filter(transaction::Column::Date.gte(today - Duration::days(INSTALLMENT_LOOKBACK_DAYS)))
            .all(db)
            .await?;
        Ok(funds::compute(&FundsInput {
            today,
            accounts: &accounts,
            payment_methods: &pms,
            categories: &categories,
            transactions: &txs,
        }))
    }
}

#[derive(InputObject)]
pub struct AccountInput {
    pub name: String,
    pub bank: Option<String>,
    #[graphql(default_with = "AccountKind::Checking")]
    pub kind: AccountKind,
    pub account_number: Option<String>,
    pub balance: Option<i64>,
    pub balance_date: Option<NaiveDate>,
    #[graphql(default = true)]
    pub include_in_available: bool,
    pub interest_rate: Option<f64>,
    pub maturity_date: Option<NaiveDate>,
    pub memo: Option<String>,
    pub archived: Option<bool>,
}

async fn record_balance<C: sea_orm::ConnectionTrait>(
    db: &C,
    account_id: i32,
    balance: i64,
    date: NaiveDate,
    memo: Option<String>,
) -> Result<(), sea_orm::DbErr> {
    account_balance::ActiveModel {
        account_id: Set(account_id),
        balance: Set(balance),
        balance_date: Set(date),
        memo: Set(memo),
        created_at: Set(Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

#[derive(Default)]
pub struct AccountMutation;

#[Object]
impl AccountMutation {
    async fn create_account(&self, ctx: &Context<'_>, input: AccountInput) -> Result<account::Model> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("계좌 이름을 입력하세요"));
        }
        let today = config(ctx).today();
        let count = account::Entity::find().of_user(uid).count(db).await?;
        let now = Utc::now();
        let balance = input.balance.unwrap_or(0);
        let balance_date = input.balance_date.or(input.balance.map(|_| today));
        let txn = db.begin().await?;
        let saved = account::ActiveModel {
            user_id: Set(uid),
            name: Set(name),
            bank: Set(trim_opt(input.bank)),
            kind: Set(input.kind),
            account_number: Set(trim_opt(input.account_number)),
            balance: Set(balance),
            balance_date: Set(balance_date),
            include_in_available: Set(input.include_in_available),
            interest_rate: Set(input.interest_rate),
            maturity_date: Set(input.maturity_date),
            memo: Set(trim_opt(input.memo)),
            sort_order: Set(count as i32),
            archived: Set(input.archived.unwrap_or(false)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        if let Some(date) = balance_date {
            record_balance(&txn, saved.id, balance, date, None).await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn update_account(&self, ctx: &Context<'_>, id: i32, input: AccountInput) -> Result<account::Model> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        let found = account::Entity::find_by_id(id).of_user(uid).one(db).await?.ok_or_else(|| user_error("계좌가 없습니다"))?;
        let today = config(ctx).today();
        let txn = db.begin().await?;
        // 잔고 값을 보냈을 때만 잔고 변경으로 본다 (이름 등만 고칠 때 기준일이 바뀌지 않게)
        let balance_changed = match input.balance {
            Some(b) => b != found.balance || input.balance_date.is_some_and(|d| Some(d) != found.balance_date),
            None => false,
        };
        let mut active: account::ActiveModel = found.clone().into();
        let name = input.name.trim().to_string();
        if !name.is_empty() {
            active.name = Set(name);
        }
        active.bank = Set(trim_opt(input.bank));
        active.kind = Set(input.kind);
        active.account_number = Set(trim_opt(input.account_number));
        active.include_in_available = Set(input.include_in_available);
        active.interest_rate = Set(input.interest_rate);
        active.maturity_date = Set(input.maturity_date);
        active.memo = Set(trim_opt(input.memo));
        if let Some(a) = input.archived {
            active.archived = Set(a);
        }
        if balance_changed {
            let balance = input.balance.unwrap_or(found.balance);
            let date = input.balance_date.unwrap_or(today);
            active.balance = Set(balance);
            active.balance_date = Set(Some(date));
            record_balance(&txn, id, balance, date, None).await?;
        }
        active.updated_at = Set(Utc::now());
        let saved = active.update(&txn).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 잔고 입력. 날짜를 비우면 오늘 기준으로 기록한다.
    async fn update_account_balance(
        &self,
        ctx: &Context<'_>,
        id: i32,
        balance: i64,
        date: Option<NaiveDate>,
        memo: Option<String>,
    ) -> Result<account::Model> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        let found = account::Entity::find_by_id(id).of_user(uid).one(db).await?.ok_or_else(|| user_error("계좌가 없습니다"))?;
        let date = date.unwrap_or_else(|| config(ctx).today());
        let txn = db.begin().await?;
        record_balance(&txn, id, balance, date, trim_opt(memo)).await?;
        // 과거 날짜로 기록하면 현재 잔고는 바꾸지 않는다.
        let saved = if found.balance_date.is_none_or(|d| date >= d) {
            let mut active: account::ActiveModel = found.into();
            active.balance = Set(balance);
            active.balance_date = Set(Some(date));
            active.updated_at = Set(Utc::now());
            active.update(&txn).await?
        } else {
            found
        };
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    async fn delete_account_balance(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let uid = uid(ctx)?;
        let mine = Query::select().column(account::Column::Id).from(account::Entity).and_where(account::Column::UserId.eq(uid)).to_owned();
        Ok(account_balance::Entity::delete_many()
            .filter(account_balance::Column::Id.eq(id))
            .filter(account_balance::Column::AccountId.in_subquery(mine))
            .exec(db(ctx))
            .await?
            .rows_affected
            > 0)
    }

    /// 계좌 삭제 (연결된 카드의 결제 계좌는 비워진다)
    async fn delete_account(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let uid = uid(ctx)?;
        let res = account::Entity::delete_many().of_user(uid).filter(account::Column::Id.eq(id)).exec(db(ctx)).await?;
        lookup(ctx).invalidate();
        Ok(res.rows_affected > 0)
    }

    async fn reorder_accounts(&self, ctx: &Context<'_>, ids: Vec<i32>) -> Result<bool> {
        let uid = uid(ctx)?;
        let db = db(ctx);
        let txn = db.begin().await?;
        for (i, id) in ids.iter().enumerate() {
            account::Entity::update_many()
                .of_user(uid)
                .col_expr(account::Column::SortOrder, (i as i32).into())
                .filter(account::Column::Id.eq(*id))
                .exec(&txn)
                .await?;
        }
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(true)
    }
}
