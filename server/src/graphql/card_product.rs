//! 카드 상품 API: 전월실적 목표·구간·혜택·연회비. 계정과 관계없이 함께 쓴다.
//! 누구나 만들 수 있고, 고치거나 지우는 건 만든 계정과 관리자만 한다. 고치면 그 상품을 쓰는 모든 계정의 실적·혜택 계산이 바뀐다.

use std::collections::HashSet;

use async_graphql::{ComplexObject, Context, InputObject, Object, Result};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set, TransactionTrait,
};

use super::{CurrentUser, current_user, db, lookup, trim_opt, user_error};
use crate::entity::{card_benefit, card_product, card_tier, enums::BenefitKind, payment_method};

/// 이 계정이 상품을 고칠 수 있는지: 만든 계정이거나 관리자
pub fn can_edit(user: &CurrentUser, product: &card_product::Model) -> bool {
    user.is_admin || product.created_by == Some(user.id)
}

#[ComplexObject]
impl card_product::Model {
    /// "카드사 상품명"
    async fn label(&self) -> String {
        self.display_name()
    }

    async fn benefits(&self, ctx: &Context<'_>) -> Result<Vec<card_benefit::Model>> {
        Ok(lookup(ctx).card_benefits_of(Some(self.id)).await?)
    }

    /// 전월실적 구간 (최소 실적 순)
    async fn tiers(&self, ctx: &Context<'_>) -> Result<Vec<card_tier::Model>> {
        Ok(lookup(ctx).tiers_of(Some(self.id)).await?)
    }

    /// 요청한 계정이 고칠 수 있는지 (만든 계정이거나 관리자)
    async fn can_edit(&self, ctx: &Context<'_>) -> Result<bool> {
        Ok(can_edit(&current_user(ctx)?, self))
    }

    /// 요청한 계정이 만든 상품인지
    async fn created_by_me(&self, ctx: &Context<'_>) -> Result<bool> {
        Ok(self.created_by == Some(current_user(ctx)?.id))
    }

    /// 이 상품을 고른 카드 수 (모든 계정, 가족카드 포함)
    async fn card_count(&self, ctx: &Context<'_>) -> Result<i64> {
        Ok(payment_method::Entity::find()
            .filter(payment_method::Column::CardProductId.eq(self.id))
            .count(db(ctx))
            .await? as i64)
    }
}

#[derive(InputObject, Clone)]
pub struct CardBenefitInput {
    /// 기존 혜택을 수정할 때 id
    pub id: Option<i32>,
    pub name: String,
    #[graphql(default)]
    pub kind: BenefitKind,
    /// 이용금액 1원당 혜택 (할인·포인트 0.1 = 10%, 마일리지 1/1500 = 1,500원당 1마일)
    pub rate: f64,
    pub min_amount: Option<i64>,
    pub per_tx_max: Option<i64>,
    pub monthly_cap: Option<i64>,
    #[graphql(default = true)]
    pub shares_card_limit: bool,
    #[graphql(default = false)]
    pub excludes_performance: bool,
    /// 다른 혜택과 함께 받을 수 있는 혜택
    #[graphql(default = false)]
    pub stackable: bool,
}

#[derive(InputObject, Clone)]
pub struct CardTierInput {
    pub min_spending: i64,
    pub benefit_limit: i64,
}

#[derive(InputObject)]
pub struct CardProductInput {
    /// 카드사
    pub issuer: Option<String>,
    /// 상품명
    pub name: String,
    pub annual_fee: Option<i64>,
    /// 전월실적 목표 금액
    pub monthly_target: Option<i64>,
    pub memo: Option<String>,
    /// 주면 혜택 목록을 이 내용으로 맞춘다 (id 있는 항목은 수정, 없는 항목은 추가, 빠진 항목은 삭제 — 삭제한 혜택은
    /// 그 혜택을 고른 모든 계정의 거래·반복 규칙에서도 빠진다)
    pub benefits: Option<Vec<CardBenefitInput>>,
    /// 주면 전월실적 구간을 이 내용으로 바꾼다
    pub tiers: Option<Vec<CardTierInput>>,
}

/// 같은 상품인지 볼 때 쓰는 이름: 공백을 빼고 소문자로
fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

fn validate(input: &CardProductInput) -> Result<(Option<String>, String)> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(user_error("카드 상품 이름을 입력하세요"));
    }
    if input.annual_fee.is_some_and(|f| f < 0) {
        return Err(user_error("연회비는 0원 이상입니다"));
    }
    for b in input.benefits.iter().flatten() {
        if b.name.trim().is_empty() {
            return Err(user_error("혜택 이름을 입력하세요"));
        }
        if !(0.0..=1.0).contains(&b.rate) {
            return Err(user_error("혜택률은 0~1 사이(예: 0.1 = 10%)입니다"));
        }
    }
    if input.tiers.iter().flatten().any(|t| t.min_spending < 0 || t.benefit_limit < 0) {
        return Err(user_error("전월실적 구간 금액은 0원 이상입니다"));
    }
    Ok((trim_opt(input.issuer.clone()), name))
}

/// 카드사·이름이 같은(공백·대소문자 무시) 다른 상품
async fn find_same<C: ConnectionTrait>(
    db: &C,
    issuer: Option<&str>,
    name: &str,
    except: Option<i32>,
) -> Result<Option<card_product::Model>> {
    let key = (norm(issuer.unwrap_or("")), norm(name));
    Ok(card_product::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .find(|p| Some(p.id) != except && (norm(p.issuer.as_deref().unwrap_or("")), norm(&p.name)) == key))
}

async fn save_children<C: ConnectionTrait>(
    db: &C,
    product_id: i32,
    benefits: Option<Vec<CardBenefitInput>>,
    tiers: Option<Vec<CardTierInput>>,
) -> Result<()> {
    if let Some(list) = benefits {
        let existing = card_benefit::Entity::find().filter(card_benefit::Column::CardProductId.eq(product_id)).all(db).await?;
        let keep: HashSet<i32> = list.iter().filter_map(|b| b.id).collect();
        let remove: Vec<i32> = existing.iter().map(|b| b.id).filter(|id| !keep.contains(id)).collect();
        if !remove.is_empty() {
            card_benefit::Entity::delete_many().filter(card_benefit::Column::Id.is_in(remove)).exec(db).await?;
        }
        for (i, b) in list.into_iter().enumerate() {
            let mut active = match b.id.and_then(|id| existing.iter().find(|e| e.id == id)) {
                Some(e) => card_benefit::ActiveModel::from(e.clone()),
                None => card_benefit::ActiveModel { card_product_id: Set(product_id), ..Default::default() },
            };
            active.name = Set(b.name.trim().to_string());
            active.kind = Set(b.kind);
            active.rate = Set(b.rate);
            active.min_amount = Set(b.min_amount.filter(|v| *v > 0));
            active.per_tx_max = Set(b.per_tx_max.filter(|v| *v > 0));
            active.monthly_cap = Set(b.monthly_cap.filter(|v| *v >= 0));
            active.shares_card_limit = Set(b.shares_card_limit);
            active.excludes_performance = Set(b.excludes_performance);
            active.stackable = Set(b.stackable);
            active.sort_order = Set(i as i32);
            active.save(db).await?;
        }
    }
    if let Some(list) = tiers {
        card_tier::Entity::delete_many().filter(card_tier::Column::CardProductId.eq(product_id)).exec(db).await?;
        for t in list {
            card_tier::ActiveModel {
                card_product_id: Set(product_id),
                min_spending: Set(t.min_spending),
                benefit_limit: Set(t.benefit_limit),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// 고칠 수 있는 상품을 찾는다 (없거나 권한이 없으면 오류)
async fn editable(ctx: &Context<'_>, id: i32) -> Result<card_product::Model> {
    let found = card_product::Entity::find_by_id(id).one(db(ctx)).await?.ok_or_else(|| user_error("카드 상품이 없습니다"))?;
    if !can_edit(&current_user(ctx)?, &found) {
        return Err(user_error("카드 상품은 만든 사람과 관리자만 고칠 수 있습니다"));
    }
    Ok(found)
}

#[derive(Default)]
pub struct CardProductQuery;

#[Object]
impl CardProductQuery {
    /// 카드 상품 목록 (카드사 → 이름 순). `search`가 있으면 카드사·이름에 그 글자가 들어간 것만 (공백·대소문자 무시)
    async fn card_products(&self, ctx: &Context<'_>, search: Option<String>) -> Result<Vec<card_product::Model>> {
        current_user(ctx)?;
        let key = search.as_deref().map(norm).filter(|s| !s.is_empty());
        let mut list: Vec<card_product::Model> = lookup(ctx)
            .products()
            .await?
            .values()
            .filter(|p| key.as_ref().is_none_or(|k| norm(&p.display_name()).contains(k.as_str())))
            .cloned()
            .collect();
        list.sort_by_key(|p| (norm(p.issuer.as_deref().unwrap_or("")), norm(&p.name), p.id));
        Ok(list)
    }

    async fn card_product(&self, ctx: &Context<'_>, id: i32) -> Result<Option<card_product::Model>> {
        current_user(ctx)?;
        Ok(lookup(ctx).products().await?.get(&id).cloned())
    }
}

#[derive(Default)]
pub struct CardProductMutation;

#[Object]
impl CardProductMutation {
    /// 카드 상품 추가. 카드사·이름이 같은 상품이 있으면 그것을 고르도록 거절한다.
    async fn create_card_product(&self, ctx: &Context<'_>, input: CardProductInput) -> Result<card_product::Model> {
        let user = current_user(ctx)?;
        let db = db(ctx);
        let (issuer, name) = validate(&input)?;
        if let Some(same) = find_same(db, issuer.as_deref(), &name, None).await? {
            return Err(user_error(format!("'{}' 카드 상품이 이미 있습니다. 목록에서 고르세요", same.display_name())));
        }
        let now = Utc::now();
        let txn = db.begin().await?;
        let saved = card_product::ActiveModel {
            issuer: Set(issuer),
            name: Set(name),
            annual_fee: Set(input.annual_fee),
            monthly_target: Set(input.monthly_target.filter(|t| *t > 0)),
            memo: Set(trim_opt(input.memo)),
            created_by: Set(Some(user.id)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        save_children(&txn, saved.id, input.benefits, input.tiers).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 카드 상품 수정 (만든 계정·관리자). 그 상품을 쓰는 모든 계정에 바로 반영된다.
    async fn update_card_product(&self, ctx: &Context<'_>, id: i32, input: CardProductInput) -> Result<card_product::Model> {
        let found = editable(ctx, id).await?;
        let db = db(ctx);
        let (issuer, name) = validate(&input)?;
        if let Some(same) = find_same(db, issuer.as_deref(), &name, Some(id)).await? {
            return Err(user_error(format!("'{}' 카드 상품이 이미 있습니다", same.display_name())));
        }
        let txn = db.begin().await?;
        let mut active: card_product::ActiveModel = found.into();
        active.issuer = Set(issuer);
        active.name = Set(name);
        active.annual_fee = Set(input.annual_fee);
        active.monthly_target = Set(input.monthly_target.filter(|t| *t > 0));
        active.memo = Set(trim_opt(input.memo));
        active.updated_at = Set(Utc::now());
        let saved = active.update(&txn).await?;
        save_children(&txn, saved.id, input.benefits, input.tiers).await?;
        txn.commit().await?;
        lookup(ctx).invalidate();
        Ok(saved)
    }

    /// 카드 상품 삭제 (만든 계정·관리자). 고른 카드가 하나라도 있으면 지우지 않는다.
    async fn delete_card_product(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        editable(ctx, id).await?;
        let db = db(ctx);
        let used = payment_method::Entity::find().filter(payment_method::Column::CardProductId.eq(id)).count(db).await?;
        if used > 0 {
            return Err(user_error(format!("카드 {used}개가 이 상품을 쓰고 있어 지울 수 없습니다")));
        }
        let res = card_product::Entity::delete_by_id(id).exec(db).await?;
        lookup(ctx).invalidate();
        Ok(res.rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::{Value, json};

    use crate::entity::{ledger, user};
    use crate::graphql::{CashbookSchema, as_user, build_schema};

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
    async fn products_are_shared_but_only_creator_or_admin_edits() {
        let db = crate::db::connect_in_memory().await;
        let other = user::ActiveModel { email: Set(Some("b@example.com".into())), created_at: Set(Utc::now()), ..Default::default() }
            .insert(&db)
            .await
            .unwrap()
            .id;
        for uid in [1, other] {
            ledger::ActiveModel {
                user_id: Set(uid),
                name: Set("가계부".into()),
                description: Set(None),
                is_default: Set(true),
                sort_order: Set(0),
                created_at: Set(Utc::now()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let schema = build_schema(db.clone(), crate::config::AppConfig::for_tests());
        let id = |v: &Value| v.as_i64().unwrap();

        // 관리자(계정 1)가 만든 상품, 계정 2가 만든 상품
        let p = ok(
            &schema,
            1,
            r#"mutation { createCardProduct(input: {issuer: "신한", name: "Deep Dream", monthlyTarget: 300000,
            benefits: [{name: "10%", rate: 0.1, sharesCardLimit: false}, {name: "5%", rate: 0.05}]}) { id benefits { id } } }"#,
        )
        .await["createCardProduct"]
            .clone();
        let (pid, b10, b5) = (id(&p["id"]), id(&p["benefits"][0]["id"]), id(&p["benefits"][1]["id"]));
        let q = id(&ok(&schema, other, r#"mutation { createCardProduct(input: {issuer: "KB", name: "My WE:SH"}) { id } }"#).await["createCardProduct"]
            ["id"]);

        // 카드사·이름이 같으면(공백·대소문자 무시) 새로 만들지 않고 고르게 한다
        let msg = err(&schema, other, r#"mutation { createCardProduct(input: {issuer: "신한 ", name: "deep dream"}) { id } }"#).await;
        assert!(msg.contains("이미 있습니다"), "{msg}");

        // 고치기는 만든 계정과 관리자만
        let list = ok(&schema, other, r#"{ cardProducts { label canEdit createdByMe } }"#).await;
        assert_eq!(
            list["cardProducts"],
            json!([{"label": "KB My WE:SH", "canEdit": true, "createdByMe": true}, {"label": "신한 Deep Dream", "canEdit": false, "createdByMe": false}])
        );
        let msg = err(&schema, other, &format!(r#"mutation {{ updateCardProduct(id: {pid}, input: {{name: "x"}}) {{ id }} }}"#)).await;
        assert!(msg.contains("만든 사람과 관리자"), "{msg}");
        let msg = err(&schema, other, &format!("mutation {{ deleteCardProduct(id: {pid}) }}")).await;
        assert!(msg.contains("만든 사람과 관리자"), "{msg}");
        ok(&schema, 1, &format!(r#"mutation {{ updateCardProduct(id: {q}, input: {{issuer: "KB국민", name: "My WE:SH", annualFee: 10000}}) {{ id }} }}"#)).await;
        let found = ok(&schema, other, &format!("{{ cardProducts(search: \"국민 my\") {{ id annualFee }} }}")).await;
        assert_eq!(found["cardProducts"], json!([{"id": q, "annualFee": 10000}]));

        // 두 계정이 같은 상품을 쓴다: 혜택은 같고, 실적은 계정마다 따로
        let card = |user: i32| {
            let schema = schema.clone();
            async move {
                let pm = id(&ok(&schema, user, &format!(r#"mutation {{ createPaymentMethod(input: {{name: "신한", kind: CREDIT_CARD, cardProductId: {pid}}}) {{ id }} }}"#))
                    .await["createPaymentMethod"]["id"]);
                let tx = id(&ok(
                    &schema,
                    user,
                    &format!(r#"mutation {{ createTransaction(input: {{date: "2026-11-05", amount: {}, paymentMethodId: {pm}, description: "마트", benefitIds: [{b10}]}}) {{ id }} }}"#, 100_000 * user),
                )
                .await["createTransaction"]["id"]);
                (pm, tx)
            }
        };
        let (pm1, _) = card(1).await;
        let (pm2, tx2) = card(other).await;
        let report = |user: i32| {
            let schema = schema.clone();
            async move { ok(&schema, user, r#"{ cardsMonthly(month: "2026-11") { paymentMethodId performance target discountTotal } }"#).await["cardsMonthly"].clone() }
        };
        assert_eq!(report(1).await, json!([{"paymentMethodId": pm1, "performance": 100000, "target": 300000, "discountTotal": 10000}]));
        assert_eq!(report(other).await, json!([{"paymentMethodId": pm2, "performance": 200000, "target": 300000, "discountTotal": 20000}]));
        let used = ok(&schema, other, &format!("{{ cardProduct(id: {pid}) {{ cardCount }} }}")).await;
        assert_eq!(used["cardProduct"]["cardCount"], 2);

        // 쓰는 카드가 있으면 지우지 않는다 (관리자라도)
        let msg = err(&schema, 1, &format!("mutation {{ deleteCardProduct(id: {pid}) }}")).await;
        assert!(msg.contains("카드 2개"), "{msg}");

        // 상품을 고치면 모든 계정에 반영된다 (지운 혜택은 거래에서도 빠진다)
        ok(&schema, 1, &format!(r#"mutation {{ updateCardProduct(id: {pid}, input: {{issuer: "신한", name: "Deep Dream", monthlyTarget: 500000,
            benefits: [{{id: {b5}, name: "5%", rate: 0.05}}]}}) {{ id }} }}"#)).await;
        assert_eq!(report(other).await[0]["target"], 500000);
        let t = ok(&schema, other, &format!("{{ transaction(id: {tx2}) {{ benefitIds }} }}")).await;
        assert_eq!(t["transaction"]["benefitIds"], json!([]));

        // 카드의 상품을 바꾸면 이전 상품의 혜택 지정은 지운다
        ok(&schema, other, &format!(r#"mutation {{ updateTransaction(id: {tx2}, input: {{benefitIds: [{b5}]}}) {{ id }} }}"#)).await;
        ok(&schema, other, &format!(r#"mutation {{ updatePaymentMethod(id: {pm2}, input: {{name: "신한", kind: CREDIT_CARD, cardProductId: {q}}}) {{ id }} }}"#)).await;
        let t = ok(&schema, other, &format!("{{ transaction(id: {tx2}) {{ benefitIds }} paymentMethod(id: {pm2}) {{ productName monthlyTarget }} }}")).await;
        assert_eq!(t, json!({"transaction": {"benefitIds": []}, "paymentMethod": {"productName": "My WE:SH", "monthlyTarget": null}}));

        // 아무도 안 쓰면 만든 계정이 지울 수 있다
        ok(&schema, other, &format!(r#"mutation {{ updatePaymentMethod(id: {pm2}, input: {{name: "신한", kind: CREDIT_CARD}}) {{ id }} }}"#)).await;
        assert_eq!(ok(&schema, other, &format!("mutation {{ deleteCardProduct(id: {q}) }}")).await["deleteCardProduct"], true);
    }
}
