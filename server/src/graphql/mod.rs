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
    OfUser, account as account_entity, card_benefit, card_tier, category, ledger as ledger_entity, payment_method,
    recurring_rule, recurring_rule_benefit, transaction as transaction_entity, transaction_benefit,
};
use sea_orm::sea_query::Query as SqlQuery;

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

/// 요청한 계정. 라우트가 인증한 뒤 요청마다 `request.data(...)`로 넣는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentUser {
    pub id: i32,
    /// 모든 카드 상품을 고칠 수 있다 (`CASHBOOK_ADMIN_EMAILS`, `CASHBOOK_OWNER_EMAIL`, 인증 없는 로컬 실행)
    pub is_admin: bool,
}

/// 요청마다 새로 만들어 `request.data(...)`로 넣는 캐시. 한 계정의 데이터만 담는다.
/// 카테고리/결제수단/혜택처럼 작은 테이블은 요청당 한 번만 읽고 거래 목록의 하위 필드에서 재사용한다.
pub struct Lookup {
    db: DatabaseConnection,
    user_id: i32,
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
    pub fn new(db: DatabaseConnection, user_id: i32) -> Self {
        Self {
            db,
            user_id,
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
            Ok(ledger_entity::Entity::find().of_user(self.user_id).all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn categories(&self) -> Result<Arc<HashMap<i32, category::Model>>, DbErr> {
        cached(&self.categories, || async {
            Ok(category::Entity::find().of_user(self.user_id).all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn payment_methods(&self) -> Result<Arc<HashMap<i32, payment_method::Model>>, DbErr> {
        cached(&self.payment_methods, || async {
            Ok(payment_method::Entity::find().of_user(self.user_id).all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
        })
        .await
    }

    pub async fn accounts(&self) -> Result<Arc<HashMap<i32, account_entity::Model>>, DbErr> {
        cached(&self.accounts, || async {
            Ok(account_entity::Entity::find().of_user(self.user_id).all(&self.db).await?.into_iter().map(|m| (m.id, m)).collect())
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
            let mine = SqlQuery::select()
                .column(transaction_entity::Column::Id)
                .from(transaction_entity::Entity)
                .and_where(transaction_entity::Column::UserId.eq(self.user_id))
                .to_owned();
            let rows = transaction_benefit::Entity::find()
                .filter(transaction_benefit::Column::TransactionId.in_subquery(mine))
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
            let mine = SqlQuery::select()
                .column(recurring_rule::Column::Id)
                .from(recurring_rule::Entity)
                .and_where(recurring_rule::Column::UserId.eq(self.user_id))
                .to_owned();
            let rows = recurring_rule_benefit::Entity::find()
                .filter(recurring_rule_benefit::Column::RecurringRuleId.in_subquery(mine))
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
                .of_user(self.user_id)
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
            let parents =
                transaction_entity::Entity::find().of_user(self.user_id).filter(C::Id.is_in(parent_ids)).all(&self.db).await?;
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

/// 요청한 계정. 라우트가 넣지 않았으면(인증 전) 오류.
pub(crate) fn current_user(ctx: &Context<'_>) -> Result<CurrentUser, async_graphql::Error> {
    ctx.data_opt::<CurrentUser>().copied().ok_or_else(|| async_graphql::Error::new("로그인이 필요합니다"))
}

/// 요청한 계정 id
pub(crate) fn uid(ctx: &Context<'_>) -> Result<i32, async_graphql::Error> {
    Ok(current_user(ctx)?.id)
}

/// 요청에 Lookup이 없으면(테스트 등) 요청한 계정으로 임시로 하나 만든다. 계정이 없으면 빈 데이터.
pub(crate) fn lookup<'a>(ctx: &Context<'a>) -> std::borrow::Cow<'a, Lookup> {
    match ctx.data_opt::<Lookup>() {
        Some(l) => std::borrow::Cow::Borrowed(l),
        None => std::borrow::Cow::Owned(Lookup::new(db(ctx).clone(), uid(ctx).unwrap_or(0))),
    }
}

impl Clone for Lookup {
    fn clone(&self) -> Self {
        Lookup::new(self.db.clone(), self.user_id)
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

/// 테스트: `id` 계정으로 보내는 요청 (id 1은 관리자)
#[cfg(test)]
pub(crate) fn as_user(query: &str, id: i32) -> async_graphql::Request {
    async_graphql::Request::new(query).data(CurrentUser { id, is_admin: id == crate::entity::user::LEGACY_USER_ID })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::{Value, json};

    use super::*;
    use crate::entity::user;

    async fn run(schema: &CashbookSchema, user: i32, query: &str) -> async_graphql::Response {
        schema.execute(as_user(query, user)).await
    }

    async fn ok(schema: &CashbookSchema, user: i32, query: &str) -> Value {
        let res = run(schema, user, query).await;
        assert!(res.errors.is_empty(), "{query}: {:?}", res.errors);
        res.data.into_json().unwrap()
    }

    async fn err(schema: &CashbookSchema, user: i32, query: &str) -> String {
        let res = run(schema, user, query).await;
        res.errors.first().unwrap_or_else(|| panic!("{query}: 오류가 나야 합니다")).message.clone()
    }

    #[tokio::test]
    async fn accounts_only_see_and_change_their_own_data() {
        let db = crate::db::connect_in_memory().await;
        let other = user::ActiveModel { email: Set(Some("b@example.com".into())), created_at: Set(Utc::now()), ..Default::default() }
            .insert(&db)
            .await
            .unwrap()
            .id;
        let schema = build_schema(db.clone(), AppConfig::for_tests());
        let id = |v: &Value| v.as_i64().unwrap();

        // 계정 1의 데이터
        let l1 = id(&ok(&schema, 1, r#"mutation { createLedger(input: {name: "가계부"}) { id } }"#).await["createLedger"]["id"]);
        let c1 = id(&ok(&schema, 1, &format!(r#"mutation {{ createCategory(input: {{ledgerId: {l1}, name: "외식"}}) {{ id }} }}"#)).await
            ["createCategory"]["id"]);
        let a1 = id(&ok(&schema, 1, r#"mutation { createAccount(input: {name: "월급통장", balance: 1000000}) { id } }"#).await["createAccount"]["id"]);
        let p1 = id(&ok(&schema, 1, &format!(r#"mutation {{ createPaymentMethod(input: {{name: "카드", kind: CREDIT_CARD, accountId: {a1}}}) {{ id }} }}"#))
            .await["createPaymentMethod"]["id"]);
        let t1 = id(&ok(
            &schema,
            1,
            &format!(r#"mutation {{ createTransaction(input: {{date: "2026-11-05", amount: 12000, categoryId: {c1}, paymentMethodId: {p1}, description: "점심", member: "송화"}}) {{ id }} }}"#),
        )
        .await["createTransaction"]["id"]);
        ok(&schema, 1, &format!(r#"mutation {{ createRecurringRule(input: {{ledgerId: {l1}, description: "보험", amount: 50000, dayOfMonth: 25, startMonth: "2026-11"}}) {{ id }} }}"#)).await;
        ok(&schema, 1, r#"mutation { createApiToken(name: "폰") { token } }"#).await;
        let text = "신한카드 승인 홍*동 9,000원 일시불 11/05 09:00 김밥천국";
        let first = ok(&schema, 1, &format!(r#"mutation {{ ingestTransaction(input: {{text: "{text}"}}) {{ created duplicate }} }}"#)).await;
        assert_eq!(first["ingestTransaction"], json!({"created": true, "duplicate": false}));

        // 계정 2는 같은 이름의 장부·결제수단을 따로 만들 수 있고, 계정 1의 데이터는 보이지 않는다
        let l2 = id(&ok(&schema, other, r#"mutation { createLedger(input: {name: "가계부"}) { id isDefault } }"#).await["createLedger"]["id"]);
        ok(&schema, other, r#"mutation { createPaymentMethod(input: {name: "카드", kind: CREDIT_CARD}) { id } }"#).await;
        let seen = ok(
            &schema,
            other,
            &format!(
                r#"{{ ledgers {{ id }} categories {{ id }} accounts {{ id }} apiTokens {{ id }} recurringRules {{ id }} members
                paymentMethods {{ name }} transaction(id: {t1}) {{ id }} paymentMethod(id: {p1}) {{ id }} account(id: {a1}) {{ id }}
                transactions(filter: {{month: "2026-11"}}) {{ totalCount }} monthlySummary(month: "2026-11") {{ spending }}
                availableFunds {{ accountsTotal }} cardsMonthly(month: "2026-11") {{ paymentMethodId }} }}"#
            ),
        )
        .await;
        let mut seen = seen;
        let cards = seen.as_object_mut().unwrap().remove("cardsMonthly").unwrap();
        assert_eq!(
            seen,
            json!({"ledgers": [{"id": l2}], "categories": [], "accounts": [], "apiTokens": [], "recurringRules": [], "members": [],
                "paymentMethods": [{"name": "카드"}], "transaction": null, "paymentMethod": null, "account": null,
                "transactions": {"totalCount": 0}, "monthlySummary": {"spending": 0}, "availableFunds": {"accountsTotal": 0}})
        );
        assert_eq!(cards.as_array().unwrap().len(), 1, "계정 2의 카드만: {cards}");
        assert_ne!(id(&cards[0]["paymentMethodId"]), p1);

        // 계정 1의 것을 쓰거나 고칠 수 없다
        for q in [
            format!(r#"mutation {{ updateTransaction(id: {t1}, input: {{memo: "x"}}) {{ id }} }}"#),
            format!("mutation {{ deleteTransaction(id: {t1}) }}"),
            format!("mutation {{ confirmTransaction(id: {t1}) {{ id }} }}"),
            format!(r#"mutation {{ splitTransaction(id: {t1}, ledgerId: {l2}, amount: 1000) {{ id }} }}"#),
            format!("mutation {{ moveTransactions(ids: [{t1}], ledgerId: {l2}) {{ moved }} }}"),
            format!("mutation {{ deleteLedger(id: {l1}) }}"),
            format!(r#"mutation {{ updateCategory(id: {c1}, input: {{name: "x"}}) {{ id }} }}"#),
            format!("mutation {{ deletePaymentMethod(id: {p1}) {{ deleted }} }}"),
            format!("mutation {{ updateAccountBalance(id: {a1}, balance: 1) {{ id }} }}"),
            format!(r#"mutation {{ createTransaction(input: {{date: "2026-11-05", amount: 1, categoryId: {c1}, description: "x"}}) {{ id }} }}"#),
            format!(r#"mutation {{ createTransaction(input: {{date: "2026-11-05", amount: 1, paymentMethodId: {p1}, description: "x"}}) {{ id }} }}"#),
            format!(r#"mutation {{ createTransaction(input: {{ledgerId: {l2}, date: "2026-11-05", amount: 1, transferLedgerId: {l1}, description: "x"}}) {{ id }} }}"#),
            format!(r#"mutation {{ createCategory(input: {{ledgerId: {l1}, name: "x"}}) {{ id }} }}"#),
            format!(r#"mutation {{ createPaymentMethod(input: {{name: "x", kind: CREDIT_CARD, accountId: {a1}}}) {{ id }} }}"#),
            format!(r#"mutation {{ createPlanItem(input: {{ledgerId: {l1}, kind: ALLOCATION, name: "x", amount: 1}}) {{ id }} }}"#),
            format!(r#"{{ statementLines(paymentMethodId: {p1}, month: "2026-11") {{ amount }} }}"#),
        ] {
            let msg = err(&schema, other, &q).await;
            assert!(msg.contains("없"), "{q}: {msg}");
        }
        // 지운 척해도 계정 1의 거래는 남아 있다
        let mine = ok(&schema, 1, &format!("{{ transaction(id: {t1}) {{ memo status }} ledgers {{ id }} }}")).await;
        assert_eq!(mine, json!({"transaction": {"memo": null, "status": "CONFIRMED"}, "ledgers": [{"id": l1}]}));

        // 같은 승인 문자라도 계정마다 따로 들어간다 (중복 방지는 계정 안에서만)
        let again = ok(&schema, other, &format!(r#"mutation {{ ingestTransaction(input: {{text: "{text}"}}) {{ created duplicate transaction {{ ledgerId }} }} }}"#)).await;
        assert_eq!(again["ingestTransaction"], json!({"created": true, "duplicate": false, "transaction": {"ledgerId": l2}}));
        let dup = ok(&schema, 1, &format!(r#"mutation {{ ingestTransaction(input: {{text: "{text}"}}) {{ duplicate }} }}"#)).await;
        assert_eq!(dup["ingestTransaction"]["duplicate"], true);
    }

    #[tokio::test]
    async fn requests_without_account_are_rejected() {
        let db = crate::db::connect_in_memory().await;
        let schema = build_schema(db, AppConfig::for_tests());
        let res = schema.execute("{ ledgers { id } }").await;
        assert_eq!(res.errors[0].message, "로그인이 필요합니다");
    }
}
