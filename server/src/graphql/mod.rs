//! GraphQL 스키마. 도메인별 Query/Mutation을 MergedObject로 합친다.

mod account;
mod ledger;
mod payment;
mod recurring;
mod summary;
mod token;
mod transaction;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use async_graphql::{Context, EmptySubscription, ErrorExtensions, MergedObject, Schema, SimpleObject};
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};

use crate::config::AppConfig;
use crate::entity::{
    account as account_entity, card_benefit, card_tier, category, ledger as ledger_entity, payment_method,
    recurring_rule_benefit, transaction as transaction_entity, transaction_benefit,
};


#[derive(MergedObject, Default)]
pub struct Query(
    ledger::LedgerQuery,
    payment::PaymentQuery,
    account::AccountQuery,
    transaction::TransactionQuery,
    recurring::RecurringQuery,
    summary::SummaryQuery,
    token::TokenQuery,
);

#[derive(MergedObject, Default)]
pub struct Mutation(
    ledger::LedgerMutation,
    payment::PaymentMutation,
    account::AccountMutation,
    transaction::TransactionMutation,
    recurring::RecurringMutation,
    token::TokenMutation,
);

pub type CashbookSchema = Schema<Query, Mutation, EmptySubscription>;

pub fn build_schema(db: DatabaseConnection, config: AppConfig) -> CashbookSchema {
    Schema::build(Query::default(), Mutation::default(), EmptySubscription)
        .data(db)
        .data(config)
        .limit_depth(16)
        .finish()
}

/// 요청마다 새로 만들어 `request.data(...)`로 넣는 캐시.
/// 카테고리/결제수단/혜택처럼 작은 테이블은 요청당 한 번만 읽고 거래 목록의 하위 필드에서 재사용한다.
pub struct Lookup {
    db: DatabaseConnection,
    ledgers: Slot<HashMap<i32, ledger_entity::Model>>,
    categories: Slot<HashMap<i32, category::Model>>,
    payment_methods: Slot<HashMap<i32, payment_method::Model>>,
    accounts: Slot<HashMap<i32, account_entity::Model>>,
    benefits: Slot<HashMap<i32, card_benefit::Model>>,
    tiers: Slot<HashMap<i32, Vec<card_tier::Model>>>,
    tx_benefits: Slot<HashMap<i32, Vec<i32>>>,
    rule_benefits: Slot<HashMap<i32, Vec<i32>>>,
    split_groups: Slot<HashMap<i32, Vec<transaction_entity::Model>>>,
}

type Slot<T> = Mutex<Option<Arc<T>>>;

async fn cached<T, F, Fut>(slot: &Slot<T>, load: F) -> Result<Arc<T>, DbErr>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, DbErr>>,
{
    if let Some(v) = slot.lock().unwrap().clone() {
        return Ok(v);
    }
    let v = Arc::new(load().await?);
    *slot.lock().unwrap() = Some(v.clone());
    Ok(v)
}

impl Lookup {
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            ledgers: Mutex::default(),
            categories: Mutex::default(),
            payment_methods: Mutex::default(),
            accounts: Mutex::default(),
            benefits: Mutex::default(),
            tiers: Mutex::default(),
            tx_benefits: Mutex::default(),
            rule_benefits: Mutex::default(),
            split_groups: Mutex::default(),
        }
    }

    /// 뮤테이션으로 데이터가 바뀐 뒤 호출한다.
    pub fn invalidate(&self) {
        *self.ledgers.lock().unwrap() = None;
        *self.categories.lock().unwrap() = None;
        *self.payment_methods.lock().unwrap() = None;
        *self.accounts.lock().unwrap() = None;
        *self.benefits.lock().unwrap() = None;
        *self.tiers.lock().unwrap() = None;
        *self.tx_benefits.lock().unwrap() = None;
        *self.rule_benefits.lock().unwrap() = None;
        *self.split_groups.lock().unwrap() = None;
    }

    pub async fn ledgers(&self) -> Result<Arc<HashMap<i32, ledger_entity::Model>>, DbErr> {
        cached(&self.ledgers, || async {
            Ok(ledger_entity::Entity::find().all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn categories(&self) -> Result<Arc<HashMap<i32, category::Model>>, DbErr> {
        cached(&self.categories, || async {
            Ok(category::Entity::find().all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn payment_methods(&self) -> Result<Arc<HashMap<i32, payment_method::Model>>, DbErr> {
        cached(&self.payment_methods, || async {
            Ok(payment_method::Entity::find().all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn accounts(&self) -> Result<Arc<HashMap<i32, account_entity::Model>>, DbErr> {
        cached(&self.accounts, || async {
            Ok(account_entity::Entity::find().all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn benefits(&self) -> Result<Arc<HashMap<i32, card_benefit::Model>>, DbErr> {
        cached(&self.benefits, || async {
            Ok(card_benefit::Entity::find().all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    /// 결제수단 id → 전월실적 구간 목록
    pub async fn tiers(&self) -> Result<Arc<HashMap<i32, Vec<card_tier::Model>>>, DbErr> {
        cached(&self.tiers, || async {
            let mut map: HashMap<i32, Vec<card_tier::Model>> = HashMap::new();
            for t in card_tier::Entity::find().all(&self.db).await? {
                map.entry(t.payment_method_id).or_default().push(t);
            }
            for v in map.values_mut() {
                v.sort_by_key(|t| t.min_spending);
            }
            Ok(map)
        })
        .await
    }

    /// 거래 id → 혜택 id 목록
    pub async fn tx_benefits(&self) -> Result<Arc<HashMap<i32, Vec<i32>>>, DbErr> {
        cached(&self.tx_benefits, || async {
            let mut map: HashMap<i32, Vec<i32>> = HashMap::new();
            // DB마다 기본 순서가 달라(SQLite 입력순, MySQL 기본 키순) 혜택 id 순으로 고정한다
            let rows = transaction_benefit::Entity::find()
                .order_by_asc(transaction_benefit::Column::TransactionId)
                .order_by_asc(transaction_benefit::Column::CardBenefitId)
                .all(&self.db)
                .await?;
            for tb in rows {
                map.entry(tb.transaction_id).or_default().push(tb.card_benefit_id);
            }
            Ok(map)
        })
        .await
    }

    /// 반복 규칙 id → 골라 둔 혜택 id 목록 (혜택 id 순)
    pub async fn rule_benefits(&self) -> Result<Arc<HashMap<i32, Vec<i32>>>, DbErr> {
        cached(&self.rule_benefits, || async {
            let mut map: HashMap<i32, Vec<i32>> = HashMap::new();
            let rows = recurring_rule_benefit::Entity::find()
                .order_by_asc(recurring_rule_benefit::Column::RecurringRuleId)
                .order_by_asc(recurring_rule_benefit::Column::CardBenefitId)
                .all(&self.db)
                .await?;
            for rb in rows {
                map.entry(rb.recurring_rule_id).or_default().push(rb.card_benefit_id);
            }
            Ok(map)
        })
        .await
    }

    /// 분할 묶음: 부모 거래 id → 부모와 자식들 (부모 먼저, 자식은 id 순). 분할한 결제만 담겨 작다.
    pub async fn split_groups(&self) -> Result<Arc<HashMap<i32, Vec<transaction_entity::Model>>>, DbErr> {
        use transaction_entity::Column as C;
        cached(&self.split_groups, || async {
            let children = transaction_entity::Entity::find()
                .filter(C::SplitParentId.is_not_null())
                .order_by_asc(C::Id)
                .all(&self.db)
                .await?;
            if children.is_empty() {
                return Ok(HashMap::new());
            }
            let mut parent_ids: Vec<i32> = children.iter().filter_map(|c| c.split_parent_id).collect();
            parent_ids.sort_unstable();
            parent_ids.dedup();
            let parents = transaction_entity::Entity::find().filter(C::Id.is_in(parent_ids)).all(&self.db).await?;
            let mut map: HashMap<i32, Vec<transaction_entity::Model>> = parents.into_iter().map(|p| (p.id, vec![p])).collect();
            for c in children {
                if let Some(group) = c.split_parent_id.and_then(|pid| map.get_mut(&pid)) {
                    group.push(c);
                }
            }
            Ok(map)
        })
        .await
    }

    pub async fn card_benefits_of(&self, payment_method_id: i32) -> Result<Vec<card_benefit::Model>, DbErr> {
        let mut v: Vec<card_benefit::Model> = self
            .benefits()
            .await?
            .values()
            .filter(|b| b.payment_method_id == payment_method_id)
            .cloned()
            .collect();
        v.sort_by_key(|b| (b.sort_order, b.id));
        Ok(v)
    }
}

pub(crate) fn db<'a>(ctx: &Context<'a>) -> &'a DatabaseConnection {
    ctx.data::<DatabaseConnection>().expect("DatabaseConnection not registered")
}

pub(crate) fn config<'a>(ctx: &Context<'a>) -> &'a AppConfig {
    ctx.data::<AppConfig>().expect("AppConfig not registered")
}

/// 요청에 Lookup이 없으면(테스트 등) 임시로 하나 만든다.
pub(crate) fn lookup<'a>(ctx: &Context<'a>) -> std::borrow::Cow<'a, Lookup> {
    match ctx.data_opt::<Lookup>() {
        Some(l) => std::borrow::Cow::Borrowed(l),
        None => std::borrow::Cow::Owned(Lookup::new(db(ctx).clone())),
    }
}

impl Clone for Lookup {
    fn clone(&self) -> Self {
        Lookup::new(self.db.clone())
    }
}

/// 사용자에게 보여줄 입력 오류.
pub(crate) fn user_error(msg: impl Into<String>) -> async_graphql::Error {
    async_graphql::Error::new(msg.into()).extend_with(|_, e| e.set("code", "BAD_USER_INPUT"))
}

/// 삭제 요청 결과. 거래가 연결된 항목은 지우지 않고 보관 처리한다.
#[derive(SimpleObject, Clone, Copy, Debug)]
pub struct DeleteResult {
    pub deleted: bool,
    pub archived: bool,
}

pub(crate) fn trim_opt(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
