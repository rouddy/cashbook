//! MCP 도구 (조회 전용).
//!
//! 각 도구는 요청한 계정으로 GraphQL을 그대로 실행한다 — 계정 범위, 받은 장부 이체, 분할 묶음, 가족카드 합산 같은 규칙을
//! 다시 만들지 않는다. 결과는 AI가 읽기 좋게 바꾼다: id 옆에 이름, 거래의 성격(`kind`), null 필드 생략, 짧은 목록.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_graphql::Variables;
use chrono::NaiveDate;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::config::AppConfig;
use crate::domain::billing::BillingCycle;
use crate::domain::dates::YearMonth;
use crate::domain::summary::{UNCATEGORIZED, tx_kind};
use crate::entity::{OfUser, card_product, category, enums::CategoryKind, ledger, payment_method, transaction};
use crate::graphql::{CashbookSchema, CurrentUser, Lookup};

pub struct ToolCtx<'a> {
    pub schema: &'a CashbookSchema,
    pub db: &'a DatabaseConnection,
    pub config: &'a AppConfig,
    pub user: CurrentUser,
}

/// 도구 결과 (실패는 모델에게 보여줄 메시지)
type ToolResult = Result<Value, String>;

/// 모든 도구가 조회 전용 (ChatGPT는 readOnlyHint가 없는 도구를 쓰기로 보고 매번 확인을 받는다)
fn read_only(title: &str) -> Value {
    json!({ "title": title, "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false })
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": read_only(title),
    })
}

const MONTH: &str = "YYYY-MM (생략하면 이번 달)";

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "get_context",
            "가계부 기본 정보",
            "오늘 날짜·시간대, 장부와 장부별 지출/수입 카테고리(고정 여부·월 예산), 결제수단(카드 종류·가족카드·전월실적 목표·결제일), \
             계좌 잔액, 구성원 이름. 다른 도구에 넘길 id를 여기서 찾는다. 대화 처음에 한 번 부른다.",
            json!({}),
            &[],
        ),
        tool(
            "search_transactions",
            "거래 검색",
            "조건에 맞는 거래 목록과 합계. 기본은 확정 거래만, 최신순 50건. \
             kind: 지출 / 환불(카드의 − 금액) / 정산(카드가 아닌 결제의 − 금액: 대신 내거나 회사 물건을 사고 돌려받은 돈) / 수입 / 받은 이체 / 보낸 이체 / 장부 이체. \
             totals.spending은 지출 카테고리의 순합계(환불·정산 차감)이고 수입·장부 이체는 따로 센다(이체 합계는 ledgerId를 줄 때만). \
             기간을 안 주면 전체 기간을 찾는다.",
            json!({
                "month": { "type": "string", "description": "YYYY-MM. from/to보다 우선" },
                "from": { "type": "string", "description": "YYYY-MM-DD (포함)" },
                "to": { "type": "string", "description": "YYYY-MM-DD (포함)" },
                "ledgerId": { "type": "integer", "description": "장부. 다른 장부에서 이 장부로 받은 이체도 함께 나온다" },
                "categoryId": { "type": "integer" },
                "uncategorized": { "type": "boolean", "description": "true면 카테고리 없는 거래만" },
                "paymentMethodId": { "type": "integer" },
                "member": { "type": "string", "description": "구성원 이름" },
                "query": { "type": "string", "description": "내용·메모 검색어" },
                "status": { "type": "string", "enum": ["CONFIRMED", "PLANNED", "ALL"], "description": "기본 CONFIRMED (ALL = 확정+예정)" },
                "transfers": { "type": "string", "enum": ["INCLUDE", "EXCLUDE", "ONLY"], "description": "장부 이체 포함 여부. 기본 INCLUDE" },
                "sort": { "type": "string", "enum": ["DATE_DESC", "DATE_ASC", "AMOUNT_DESC"], "description": "기본 DATE_DESC" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "기본 50" },
                "offset": { "type": "integer", "minimum": 0 },
            }),
            &[],
        ),
        tool(
            "get_monthly_summary",
            "월 요약",
            "한 달의 소비(확정·예정, 고정·변동), 수입, 카테고리별 금액과 예산·평균 비교, 결제수단별·구성원별 합계, 최근 몇 달 추이. \
             ledgerId를 주면 그 장부의 받은/보낸 이체, 지출계획(배분·구성원 용돈)과 월잔액(수입 + 받은 이체 − 소비 − 보낸 이체 − 배분)도 계산한다. \
             ledgerId를 비우면 전체 장부 합계(장부 이체 제외).",
            json!({
                "month": { "type": "string", "description": MONTH },
                "ledgerId": { "type": "integer" },
                "averageMonths": { "type": "integer", "minimum": 1, "maximum": 36, "description": "평균을 낼 지난 달 수. 기본 6" },
                "historyMonths": { "type": "integer", "minimum": 1, "maximum": 60, "description": "추이에 넣을 지난 달 수. 기본 6" },
            }),
            &[],
        ),
        tool(
            "get_available_funds",
            "여유자금",
            "오늘 기준 쓸 수 있는 돈: 여유자금에 넣는 계좌 잔액 합계에서 아직 빠지지 않은 카드 대금, 잔액 기준일 이후 출금, \
             이번 달 남은 예정 입출금을 반영한다. 카드별 다음 결제일·금액 포함.",
            json!({}),
            &[],
        ),
        tool(
            "get_card_performance",
            "카드 전월실적·혜택",
            "카드별 그 달 이용금액, 실적(다음 달 혜택 구간을 정함), 목표·다음 구간까지 남은 금액, 이번 달 혜택 한도와 받은 혜택(할인·포인트·마일), \
             혜택별 사용량. 가족카드 이용은 본인 카드 실적에 합쳐진다. 어느 카드로 결제할지 고를 때 쓴다.",
            json!({
                "month": { "type": "string", "description": MONTH },
                "includeFamily": { "type": "boolean", "description": "가족카드도 따로 보기. 기본 false" },
            }),
            &[],
        ),
        tool(
            "get_card_statement",
            "카드 청구서",
            "카드 한 장의 결제월 청구서: 이용기간, 결제일, 청구 금액(확정·예정)과 청구되는 거래(할부는 회차 금액). \
             본인 카드 청구서에는 가족카드 이용도 들어간다.",
            json!({
                "paymentMethodId": { "type": "integer", "description": "카드 id (get_context)" },
                "month": { "type": "string", "description": "결제월 YYYY-MM (생략하면 다음 결제 청구서)" },
            }),
            &["paymentMethodId"],
        ),
        tool(
            "list_recurring_rules",
            "반복 거래 규칙",
            "매달(또는 몇 달마다) 생기는 고정 지출·수입·장부 이체 규칙과 다음 날짜. 이 규칙이 예정(PLANNED) 거래를 만든다.",
            json!({
                "ledgerId": { "type": "integer", "description": "이 장부의 규칙과 이 장부로 들어오는 이체 규칙" },
                "includeInactive": { "type": "boolean", "description": "꺼 둔 규칙도 보기. 기본 false" },
            }),
            &[],
        ),
    ]
}

/// 도구 실행. 없는 도구면 None.
pub async fn call(ctx: &ToolCtx<'_>, name: &str, args: Value) -> Option<ToolResult> {
    Some(match name {
        "get_context" => get_context(ctx).await,
        "search_transactions" => match parse(args) {
            Ok(a) => search_transactions(ctx, a).await,
            Err(e) => Err(e),
        },
        "get_monthly_summary" => match parse(args) {
            Ok(a) => monthly_summary(ctx, a).await,
            Err(e) => Err(e),
        },
        "get_available_funds" => available_funds(ctx).await,
        "get_card_performance" => match parse(args) {
            Ok(a) => card_performance(ctx, a).await,
            Err(e) => Err(e),
        },
        "get_card_statement" => match parse(args) {
            Ok(a) => card_statement(ctx, a).await,
            Err(e) => Err(e),
        },
        "list_recurring_rules" => match parse(args) {
            Ok(a) => recurring_rules(ctx, a).await,
            Err(e) => Err(e),
        },
        _ => return None,
    })
}

// ───────────────────────── 공용 ─────────────────────────

fn parse<T: DeserializeOwned + Default>(args: Value) -> Result<T, String> {
    if args.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(args).map_err(|e| format!("인자 오류: {e}"))
}

fn db_error(e: DbErr) -> String {
    format!("DB 오류: {e}")
}

/// 요청한 계정으로 GraphQL 실행. 오류(입력 오류 포함)는 첫 메시지.
async fn gql(ctx: &ToolCtx<'_>, doc: &str, vars: Value) -> ToolResult {
    let req = async_graphql::Request::new(doc)
        .variables(Variables::from_json(vars))
        .data(Lookup::new(ctx.db.clone(), ctx.user.id))
        .data(ctx.user);
    let res = ctx.schema.execute(req).await;
    if let Some(e) = res.errors.into_iter().next() {
        return Err(e.message);
    }
    res.data.into_json().map_err(|e| e.to_string())
}

fn month_arg(ctx: &ToolCtx<'_>, s: Option<&str>) -> Result<YearMonth, String> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(YearMonth::of(ctx.config.today())),
        Some(s) => YearMonth::parse(s).ok_or_else(|| format!("month는 YYYY-MM 형식이어야 합니다: {s}")),
    }
}

fn date_arg(name: &str, s: Option<&str>) -> Result<Option<NaiveDate>, String> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map(Some).map_err(|_| format!("{name}는 YYYY-MM-DD 형식이어야 합니다: {s}")),
    }
}

/// null 필드를 지운다 (토큰 절약)
fn strip(v: Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(map.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k, strip(v))).collect()),
        Value::Array(items) => Value::Array(items.into_iter().map(strip).collect()),
        v => v,
    }
}

fn round(v: &Value) -> Value {
    v.as_f64().map_or(Value::Null, |f| json!(f.round() as i64))
}

fn id_of(v: &Value) -> Option<i32> {
    v.as_i64().map(|n| n as i32)
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// 계정의 장부·카테고리·결제수단 이름 (보관한 것 포함)
struct Names {
    ledgers: Arc<HashMap<i32, ledger::Model>>,
    categories: Arc<HashMap<i32, category::Model>>,
    payment_methods: Arc<HashMap<i32, payment_method::Model>>,
    products: Arc<HashMap<i32, card_product::Model>>,
}

impl Names {
    async fn load(lookup: &Lookup) -> Result<Self, String> {
        Ok(Self {
            ledgers: lookup.ledgers().await.map_err(db_error)?,
            categories: lookup.categories().await.map_err(db_error)?,
            payment_methods: lookup.payment_methods().await.map_err(db_error)?,
            products: lookup.products().await.map_err(db_error)?,
        })
    }

    fn ledger(&self, id: Option<i32>) -> Value {
        id.and_then(|id| self.ledgers.get(&id)).map_or(Value::Null, |l| json!(l.name))
    }

    fn category(&self, id: Option<i32>) -> Value {
        id.and_then(|id| self.categories.get(&id)).map_or(Value::Null, |c| json!(c.name))
    }

    fn payment_method(&self, id: Option<i32>) -> Value {
        id.and_then(|id| self.payment_methods.get(&id)).map_or(Value::Null, |p| json!(p.name))
    }

    fn product(&self, pm: Option<&payment_method::Model>) -> Value {
        pm.and_then(|p| p.card_product_id).and_then(|id| self.products.get(&id)).map_or(Value::Null, |p| json!(p.name))
    }
}

// ───────────────────────── get_context ─────────────────────────

const CONTEXT_QUERY: &str = r#"query {
  appInfo { today timezone }
  ledgers { id name isDefault description }
  categories { id ledgerId name kind isFixed monthlyBudget }
  paymentMethods { id name kind issuer parentId productName monthlyTarget usagePeriodLabel paymentDay accountId }
  accounts { id name bank kind balance balanceDate includeInAvailable }
  members
}"#;

async fn get_context(ctx: &ToolCtx<'_>) -> ToolResult {
    let d = gql(ctx, CONTEXT_QUERY, json!({})).await?;
    let names = |items: &Value| -> HashMap<i64, Value> {
        list(items).iter().filter_map(|i| Some((i["id"].as_i64()?, i["name"].clone()))).collect()
    };
    let accounts = names(&d["accounts"]);
    let cards = names(&d["paymentMethods"]);
    let categories = list(&d["categories"]);
    let ledgers: Vec<Value> = list(&d["ledgers"])
        .iter()
        .map(|l| {
            let of_kind = |kind: &str| -> Vec<Value> {
                categories
                    .iter()
                    .filter(|c| c["ledgerId"] == l["id"] && c["kind"] == kind)
                    .map(|c| {
                        json!({
                            "id": c["id"],
                            "name": c["name"],
                            "fixed": c["isFixed"].as_bool().filter(|f| *f),
                            "monthlyBudget": c["monthlyBudget"],
                        })
                    })
                    .collect()
            };
            json!({
                "id": l["id"],
                "name": l["name"],
                "isDefault": l["isDefault"],
                "description": l["description"],
                "expenseCategories": of_kind("EXPENSE"),
                "incomeCategories": of_kind("INCOME"),
            })
        })
        .collect();
    let payment_methods: Vec<Value> = list(&d["paymentMethods"])
        .iter()
        .map(|p| {
            json!({
                "id": p["id"],
                "name": p["name"],
                "kind": p["kind"],
                "issuer": p["issuer"],
                "familyCardOf": p["parentId"].as_i64().and_then(|id| cards.get(&id)).cloned(),
                "productName": p["productName"],
                "monthlyTarget": p["monthlyTarget"],
                "usagePeriod": p["usagePeriodLabel"],
                "paymentDay": p["paymentDay"],
                "account": p["accountId"].as_i64().and_then(|id| accounts.get(&id)).cloned(),
            })
        })
        .collect();
    let account_rows: Vec<Value> = list(&d["accounts"])
        .iter()
        .map(|a| {
            json!({
                "id": a["id"],
                "name": a["name"],
                "bank": a["bank"],
                "kind": a["kind"],
                "balance": a["balance"],
                "balanceDate": a["balanceDate"],
                "inAvailableFunds": a["includeInAvailable"],
            })
        })
        .collect();
    Ok(strip(json!({
        "today": d["appInfo"]["today"],
        "timezone": d["appInfo"]["timezone"],
        "ledgers": ledgers,
        "paymentMethods": payment_methods,
        "accounts": account_rows,
        "members": d["members"],
    })))
}

// ───────────────────────── search_transactions ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SearchArgs {
    month: Option<String>,
    from: Option<String>,
    to: Option<String>,
    ledger_id: Option<i32>,
    category_id: Option<i32>,
    uncategorized: Option<bool>,
    payment_method_id: Option<i32>,
    member: Option<String>,
    query: Option<String>,
    status: Option<String>,
    transfers: Option<String>,
    sort: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
}

const SEARCH_QUERY: &str = r#"query($filter: TransactionFilter!, $sort: TransactionSort!, $offset: Int!, $limit: Int!) {
  transactions(filter: $filter, sort: $sort, offset: $offset, limit: $limit) {
    totalCount confirmedTotal plannedTotal incomeTotal plannedIncomeTotal
    transferInTotal plannedTransferInTotal transferOutTotal plannedTransferOutTotal
    items { id }
  }
}"#;

fn upper(s: &Option<String>) -> Option<String> {
    s.as_deref().map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty())
}

/// 거래 한 건 (AI가 읽는 모양)
fn tx_row(t: &transaction::Model, n: &Names, ledger_id: Option<i32>, split: &HashSet<i32>) -> Value {
    let kind = tx_kind(t, &n.categories, &n.payment_methods, ledger_id);
    let transfer = t.transfer_ledger_id.is_some();
    let category = match (transfer, n.category(t.category_id)) {
        (true, _) => Value::Null,
        (false, Value::Null) => json!(UNCATEGORIZED),
        (false, name) => name,
    };
    let in_split = split.contains(&t.id) || t.split_parent_id.is_some();
    strip(json!({
        "id": t.id,
        "date": t.date,
        "amount": t.amount,
        "kind": kind.label(),
        "description": t.description,
        "memo": t.memo,
        "ledger": n.ledger(Some(t.ledger_id)),
        "transferTo": t.transfer_ledger_id.map(|id| n.ledger(Some(id))),
        "category": category,
        "paymentMethod": n.payment_method(t.payment_method_id),
        "member": t.member,
        "status": t.status,
        "installmentMonths": (t.installment_months > 1).then_some(t.installment_months),
        // 한 결제를 여러 장부·카테고리로 나눈 묶음 (같은 값끼리 한 결제)
        "splitGroup": in_split.then(|| t.split_parent_id.unwrap_or(t.id)),
        "excludedFromCardPerformance": t.exclude_from_performance.then_some(true),
    }))
}

async fn search_transactions(ctx: &ToolCtx<'_>, a: SearchArgs) -> ToolResult {
    let month = match a.month.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(m) => Some(YearMonth::parse(m).ok_or_else(|| format!("month는 YYYY-MM 형식이어야 합니다: {m}"))?),
        None => None,
    };
    let from = date_arg("from", a.from.as_deref())?;
    let to = date_arg("to", a.to.as_deref())?;
    let status = upper(&a.status).unwrap_or_else(|| "CONFIRMED".into());
    let statuses = match status.as_str() {
        "CONFIRMED" => json!(["CONFIRMED"]),
        "PLANNED" => json!(["PLANNED"]),
        "ALL" => json!(["CONFIRMED", "PLANNED"]),
        other => return Err(format!("status는 CONFIRMED, PLANNED, ALL 중 하나입니다: {other}")),
    };
    let transfers = match upper(&a.transfers).as_deref() {
        None | Some("INCLUDE") => Value::Null,
        Some("EXCLUDE") => json!(false),
        Some("ONLY") => json!(true),
        Some(other) => return Err(format!("transfers는 INCLUDE, EXCLUDE, ONLY 중 하나입니다: {other}")),
    };
    let sort = upper(&a.sort).unwrap_or_else(|| "DATE_DESC".into());
    if !["DATE_DESC", "DATE_ASC", "AMOUNT_DESC"].contains(&sort.as_str()) {
        return Err(format!("sort는 DATE_DESC, DATE_ASC, AMOUNT_DESC 중 하나입니다: {sort}"));
    }
    let limit = a.limit.unwrap_or(50).clamp(1, 200);
    let offset = a.offset.unwrap_or(0);
    let filter = json!({
        "month": month.map(|m| m.to_string()),
        "from": from,
        "to": to,
        "ledgerId": a.ledger_id,
        "categoryId": a.category_id,
        "uncategorized": a.uncategorized,
        "transfers": transfers,
        "paymentMethodId": a.payment_method_id,
        "statuses": statuses,
        "member": a.member.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        "search": a.query.as_deref().map(str::trim).filter(|s| !s.is_empty()),
    });
    let d = gql(ctx, SEARCH_QUERY, json!({ "filter": filter, "sort": sort, "offset": offset, "limit": limit })).await?;
    let page = &d["transactions"];
    let ids: Vec<i32> = list(&page["items"]).iter().filter_map(|i| id_of(&i["id"])).collect();
    let mut by_id: HashMap<i32, transaction::Model> = transaction::Entity::find()
        .of_user(ctx.user.id)
        .filter(transaction::Column::Id.is_in(ids.clone()))
        .all(ctx.db)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|t| (t.id, t))
        .collect();
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let split: HashSet<i32> = lookup.split_groups().await.map_err(db_error)?.keys().copied().collect();
    let rows: Vec<Value> = ids.iter().filter_map(|id| by_id.remove(id)).map(|t| tx_row(&t, &names, a.ledger_id, &split)).collect();
    let total = page["totalCount"].as_i64().unwrap_or(0);
    let with_planned = status != "CONFIRMED";
    let with_transfers = a.ledger_id.is_some();
    let totals = json!({
        "matched": total,
        "spending": page["confirmedTotal"],
        "plannedSpending": if with_planned { page["plannedTotal"].clone() } else { Value::Null },
        "income": page["incomeTotal"],
        "plannedIncome": if with_planned { page["plannedIncomeTotal"].clone() } else { Value::Null },
        "transferIn": if with_transfers { page["transferInTotal"].clone() } else { Value::Null },
        "transferOut": if with_transfers { page["transferOutTotal"].clone() } else { Value::Null },
        "plannedTransferIn": if with_transfers && with_planned { page["plannedTransferInTotal"].clone() } else { Value::Null },
        "plannedTransferOut": if with_transfers && with_planned { page["plannedTransferOutTotal"].clone() } else { Value::Null },
    });
    let returned = rows.len() as i64;
    Ok(strip(json!({
        "totals": totals,
        "returned": returned,
        "hasMore": (offset as i64) + returned < total,
        "transactions": rows,
    })))
}

// ───────────────────────── get_monthly_summary ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SummaryArgs {
    month: Option<String>,
    ledger_id: Option<i32>,
    average_months: Option<i32>,
    history_months: Option<i32>,
}

const SUMMARY_QUERY: &str = r#"query($month: YearMonth!, $ledgerId: Int, $avg: Int!, $hist: Int!) {
  monthlySummary(month: $month, ledgerId: $ledgerId, averageMonths: $avg, historyMonths: $hist) {
    month averageMonthsUsed spending plannedSpending fixedSpending variableSpending income plannedIncome
    transferIn plannedTransferIn transferOut plannedTransferOut
    averageSpending averageFixedSpending averageVariableSpending budgetTotal balance projectedBalance
    plan { allocations memberAllowances items { name kind amount member } }
    categories { categoryId name kind isFixed confirmed planned count average budget }
    paymentMethods { paymentMethodId name confirmed planned count }
    transfers { ledgerId incoming confirmed planned count }
    members { member spending planned allowance remaining }
    history { month spending plannedSpending income fixedSpending }
  }
}"#;

async fn monthly_summary(ctx: &ToolCtx<'_>, a: SummaryArgs) -> ToolResult {
    let month = month_arg(ctx, a.month.as_deref())?;
    let vars = json!({
        "month": month.to_string(),
        "ledgerId": a.ledger_id,
        "avg": a.average_months.unwrap_or(6),
        "hist": a.history_months.unwrap_or(6),
    });
    let d = gql(ctx, SUMMARY_QUERY, vars).await?;
    let s = &d["monthlySummary"];
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let nonzero = |v: &Value| v.as_i64().is_some_and(|n| n != 0);
    let categories: Vec<Value> = list(&s["categories"])
        .iter()
        .filter(|c| nonzero(&c["confirmed"]) || nonzero(&c["planned"]) || !c["budget"].is_null())
        .map(|c| {
            json!({
                "categoryId": c["categoryId"],
                "name": c["name"],
                "kind": c["kind"],
                "fixed": c["isFixed"].as_bool().filter(|f| *f),
                "confirmed": c["confirmed"],
                "planned": c["planned"].as_i64().filter(|n| *n != 0),
                "count": c["count"],
                "monthlyAverage": round(&c["average"]),
                "budget": c["budget"],
            })
        })
        .collect();
    let transfers: Vec<Value> = list(&s["transfers"])
        .iter()
        .map(|t| {
            json!({
                "direction": if t["incoming"].as_bool() == Some(true) { "받은 이체" } else { "보낸 이체" },
                "otherLedger": names.ledger(id_of(&t["ledgerId"])),
                "confirmed": t["confirmed"],
                "planned": t["planned"],
                "count": t["count"],
            })
        })
        .collect();
    let ledger = match a.ledger_id {
        Some(id) => names.ledger(Some(id)),
        None => json!("전체 장부"),
    };
    let mut out = json!({
        "month": s["month"],
        "ledger": ledger,
        "spending": s["spending"],
        "plannedSpending": s["plannedSpending"],
        "fixedSpending": s["fixedSpending"],
        "variableSpending": s["variableSpending"],
        "income": s["income"],
        "plannedIncome": s["plannedIncome"],
        "average": {
            "months": s["averageMonthsUsed"],
            "spending": round(&s["averageSpending"]),
            "fixedSpending": round(&s["averageFixedSpending"]),
            "variableSpending": round(&s["averageVariableSpending"]),
        },
        "budgetTotal": s["budgetTotal"],
        "categories": categories,
        "paymentMethods": s["paymentMethods"],
        "members": s["members"],
        "history": s["history"],
    });
    if a.ledger_id.is_some() {
        let extra = json!({
            "transferIn": s["transferIn"],
            "plannedTransferIn": s["plannedTransferIn"],
            "transferOut": s["transferOut"],
            "plannedTransferOut": s["plannedTransferOut"],
            "transfers": transfers,
            "plan": s["plan"],
            "balance": s["balance"],
            "projectedBalance": s["projectedBalance"],
        });
        if let (Value::Object(o), Value::Object(e)) = (&mut out, extra) {
            o.extend(e);
        }
    }
    Ok(strip(out))
}

// ───────────────────────── get_available_funds ─────────────────────────

const FUNDS_QUERY: &str = r#"query {
  availableFunds {
    today accountsTotal balanceDate cardDue cardDuePlanned unreflectedOutflow plannedOutflow plannedInflow
    available availableAfterPlanned unconfiguredCardIds
    cards {
      paymentMethodId billingConfigured counted cutoffDate due duePlanned recentSpending
      nextPayment { month periodStart periodEnd paymentDate confirmedAmount plannedAmount closed }
    }
  }
}"#;

async fn available_funds(ctx: &ToolCtx<'_>) -> ToolResult {
    let d = gql(ctx, FUNDS_QUERY, json!({})).await?;
    let f = &d["availableFunds"];
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let cards: Vec<Value> = list(&f["cards"])
        .iter()
        .map(|c| {
            json!({
                "paymentMethodId": c["paymentMethodId"],
                "card": names.payment_method(id_of(&c["paymentMethodId"])),
                "billingConfigured": c["billingConfigured"],
                "countedInAvailable": c["counted"],
                "unpaidDue": c["due"],
                "plannedDue": c["duePlanned"],
                "nextPayment": c["nextPayment"],
                "recentSpendingIfUnconfigured": if c["billingConfigured"] == json!(false) { c["recentSpending"].clone() } else { Value::Null },
            })
        })
        .collect();
    let unconfigured: Vec<Value> = list(&f["unconfiguredCardIds"]).iter().map(|id| names.payment_method(id_of(id))).collect();
    Ok(strip(json!({
        "today": f["today"],
        "available": f["available"],
        "availableAfterPlanned": f["availableAfterPlanned"],
        "accountsTotal": f["accountsTotal"],
        "balanceDate": f["balanceDate"],
        "cardDue": f["cardDue"],
        "cardDuePlanned": f["cardDuePlanned"],
        "unreflectedOutflow": f["unreflectedOutflow"],
        "plannedOutflow": f["plannedOutflow"],
        "plannedInflow": f["plannedInflow"],
        "cards": cards,
        "cardsWithoutBillingSchedule": unconfigured,
        "note": "available = 계좌 합계 − 미반영 카드대금 − 잔액 기준일 이후 출금. availableAfterPlanned는 이번 달 남은 예정 입출금까지 반영",
    })))
}

// ───────────────────────── get_card_performance ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CardArgs {
    month: Option<String>,
    include_family: Option<bool>,
}

const CARDS_QUERY: &str = r#"query($month: YearMonth!, $family: Boolean!) {
  cardsMonthly(month: $month, includeFamily: $family) {
    paymentMethodId spending plannedSpending transactionCount performance projectedPerformance previousPerformance
    performanceSteps target remainingToTarget currentLimit
    achievedTier { minSpending benefitLimit } nextTier { minSpending benefitLimit } remainingToNextTier
    benefitTotal discountTotal pointTotal mileageTotal limitUsed benefitRemaining
    remainingBenefits { kind amount }
    projectedBenefitTotal
    benefits { name kind rate monthlyCap stackable spending count amount remaining spendToMax }
  }
}"#;

async fn card_performance(ctx: &ToolCtx<'_>, a: CardArgs) -> ToolResult {
    let month = month_arg(ctx, a.month.as_deref())?;
    let d = gql(ctx, CARDS_QUERY, json!({ "month": month.to_string(), "family": a.include_family.unwrap_or(false) })).await?;
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let cards: Vec<Value> = list(&d["cardsMonthly"])
        .iter()
        .map(|c| {
            let pm = id_of(&c["paymentMethodId"]).and_then(|id| names.payment_methods.get(&id));
            let mut row = Map::new();
            row.insert("paymentMethodId".into(), c["paymentMethodId"].clone());
            row.insert("card".into(), pm.map_or(Value::Null, |p| json!(p.name)));
            row.insert("product".into(), names.product(pm));
            row.insert("familyCardOf".into(), names.payment_method(pm.and_then(|p| p.parent_id)));
            if let Value::Object(fields) = c {
                for (k, v) in fields {
                    if k != "paymentMethodId" {
                        row.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Object(row)
        })
        .collect();
    Ok(strip(json!({
        "month": month.to_string(),
        "cards": cards,
        "note": "performance = 이번 달 실적(다음 달 혜택 구간 판정), previousPerformance로 정해진 currentLimit가 이번 달 통합 혜택 한도. \
                 혜택 금액은 kind 단위(DISCOUNT 원, POINT 포인트, MILEAGE 마일)",
    })))
}

// ───────────────────────── get_card_statement ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StatementArgs {
    payment_method_id: Option<i32>,
    month: Option<String>,
}

const STATEMENT_QUERY: &str = r#"query($id: Int!, $month: YearMonth!) {
  paymentMethod(id: $id) { statements(from: $month, to: $month) { month periodStart periodEnd paymentDate confirmedAmount plannedAmount closed paid } }
  statementLines(paymentMethodId: $id, month: $month) {
    amount installmentNo installmentMonths planned
    transaction { id date amount description memo paymentMethodId categoryId }
  }
}"#;

async fn card_statement(ctx: &ToolCtx<'_>, a: StatementArgs) -> ToolResult {
    let id = a.payment_method_id.ok_or("paymentMethodId가 필요합니다 (get_context의 카드 id)")?;
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let pm = names.payment_methods.get(&id).ok_or("없는 결제수단입니다")?;
    if !pm.kind.is_card() {
        return Err(format!("'{}'은(는) 카드가 아닙니다", pm.name));
    }
    let cycle = BillingCycle::from_model(pm).ok_or_else(|| format!("'{}'은(는) 결제일·이용기간이 입력되지 않은 카드입니다", pm.name))?;
    let month = match a.month.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(m) => YearMonth::parse(m).ok_or_else(|| format!("month는 YYYY-MM 형식이어야 합니다: {m}"))?,
        None => cycle.next_statement(ctx.config.today()),
    };
    let d = gql(ctx, STATEMENT_QUERY, json!({ "id": id, "month": month.to_string() })).await?;
    let lines: Vec<Value> = list(&d["statementLines"])
        .iter()
        .map(|l| {
            let t = &l["transaction"];
            let months = l["installmentMonths"].as_i64().unwrap_or(1);
            let other_card = id_of(&t["paymentMethodId"]).filter(|c| *c != id);
            json!({
                "transactionId": t["id"],
                "date": t["date"],
                "description": t["description"],
                "memo": t["memo"],
                "category": names.category(id_of(&t["categoryId"])),
                "billed": l["amount"],
                "transactionAmount": if l["amount"] != t["amount"] { t["amount"].clone() } else { Value::Null },
                "installment": (months > 1).then(|| format!("{}/{}", l["installmentNo"], months)),
                "planned": l["planned"].as_bool().filter(|p| *p),
                "familyCard": other_card.map(|c| names.payment_method(Some(c))),
            })
        })
        .collect();
    let statement = list(&d["paymentMethod"]["statements"]).first().cloned().unwrap_or(Value::Null);
    let family = pm.parent_id.map(|p| format!("가족카드라 이 카드 이용분만 보여 줍니다. 대금은 본인 카드({}) 청구서로 함께 나갑니다", names.payment_method(Some(p)).as_str().unwrap_or("?")));
    Ok(strip(json!({
        "card": pm.name,
        "month": month.to_string(),
        "statement": statement,
        "lines": lines,
        "note": family,
    })))
}

// ───────────────────────── list_recurring_rules ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RecurringArgs {
    ledger_id: Option<i32>,
    include_inactive: Option<bool>,
}

const RECURRING_QUERY: &str = r#"query($ledgerId: Int) {
  recurringRules(ledgerId: $ledgerId) {
    id ledgerId description amount categoryId paymentMethodId dayOfMonth intervalMonths startMonth endMonth
    member memo transferLedgerId active nextDate
  }
}"#;

async fn recurring_rules(ctx: &ToolCtx<'_>, a: RecurringArgs) -> ToolResult {
    let d = gql(ctx, RECURRING_QUERY, json!({ "ledgerId": a.ledger_id })).await?;
    let lookup = Lookup::new(ctx.db.clone(), ctx.user.id);
    let names = Names::load(&lookup).await?;
    let include_inactive = a.include_inactive.unwrap_or(false);
    let rules: Vec<Value> = list(&d["recurringRules"])
        .iter()
        .filter(|r| include_inactive || r["active"].as_bool() == Some(true))
        .map(|r| {
            let category = id_of(&r["categoryId"]).and_then(|id| names.categories.get(&id));
            let kind = if !r["transferLedgerId"].is_null() {
                "장부 이체"
            } else if category.is_some_and(|c| c.kind == CategoryKind::Income) {
                "수입"
            } else {
                "지출"
            };
            json!({
                "id": r["id"],
                "description": r["description"],
                "amount": r["amount"],
                "kind": kind,
                "ledger": names.ledger(id_of(&r["ledgerId"])),
                "transferTo": names.ledger(id_of(&r["transferLedgerId"])),
                "category": category.map(|c| c.name.clone()),
                "paymentMethod": names.payment_method(id_of(&r["paymentMethodId"])),
                "dayOfMonth": r["dayOfMonth"],
                "everyMonths": r["intervalMonths"].as_i64().filter(|n| *n > 1),
                "startMonth": r["startMonth"],
                "endMonth": r["endMonth"],
                "member": r["member"],
                "memo": r["memo"],
                "active": r["active"].as_bool().filter(|a| !*a),
                "nextDate": r["nextDate"],
            })
        })
        .collect();
    Ok(strip(json!({ "rules": rules })))
}
