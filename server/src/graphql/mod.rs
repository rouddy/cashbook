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
use sea_orm::{DatabaseConnection, DbErr, EntityTrait};

use crate::config::AppConfig;
use crate::entity::{account as account_entity, card_benefit, card_tier, category, ledger as ledger_entity, payment_method, transaction_benefit};


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
            for tb in transaction_benefit::Entity::find().all(&self.db).await? {
                map.entry(tb.transaction_id).or_default().push(tb.card_benefit_id);
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
