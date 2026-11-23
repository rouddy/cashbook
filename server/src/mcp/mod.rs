//! MCP 서버 (Streamable HTTP, `/api/mcp`).
//!
//! 상태 없는 JSON-RPC: POST 한 번에 메시지 하나(또는 배열)를 받아 JSON으로 답한다. 서버가 먼저 보내는 알림이 없어
//! SSE 스트림·세션은 쓰지 않고 GET·DELETE는 405. 도구는 조회 전용이다(`tools`).
//! 인증은 `auth`: Google 로그인을 설정했으면 OAuth 액세스 토큰(OpenAI Dots·ChatGPT 플러그인 등) 또는 API 토큰,
//! 설정하지 않은 로컬 실행이면 인증 없이 기존 데이터 계정.

pub mod auth;
mod tools;

use rocket::data::{Data, ToByteUnit};
use rocket::http::Header;
use rocket::{Responder, Route, State, delete, get, post, routes};
use serde_json::{Value, json};

use crate::AppState;
use crate::graphql::CashbookSchema;
use auth::McpCaller;

/// 지원하는 MCP 규격 (첫 번째가 최신). 클라이언트가 이 중 하나를 요청하면 그대로, 아니면 최신으로 답한다.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// 클라이언트에게 주는 서버 안내. 처음 512자 안에서 뜻이 통하게 쓴다.
const INSTRUCTIONS: &str = "개인 가계부 조회 도구(읽기 전용, 금액은 원 단위 정수). \
먼저 get_context로 오늘 날짜와 장부·카테고리·결제수단 id를 확인하세요. \
지출 카테고리의 − 금액은 카드면 환불, 현금·계좌이체 등이면 남의 몫을 대신 내거나 회사 물건을 사고 돌려받은 정산입니다(kind). \
소비 합계는 환불·정산을 뺀 순액입니다. 장부 이체는 소비·수입이 아니고 전체 장부 합계에서 빠집니다. \
PLANNED는 반복 규칙 등으로 잡아 둔 아직 확정되지 않은 예정 거래입니다. \
카드 실적은 전월실적(이번 달 이용이 다음 달 혜택 구간을 정함) 기준입니다.";

#[derive(Responder)]
pub enum McpReply {
    #[response(status = 200, content_type = "json")]
    Ok(String),
    #[response(status = 202)]
    Accepted(()),
    #[response(status = 400, content_type = "json")]
    BadRequest(String),
    #[response(status = 401, content_type = "json")]
    Unauthorized(String, Header<'static>),
    #[response(status = 403, content_type = "json")]
    Forbidden(String),
    #[response(status = 500, content_type = "json")]
    Error(String),
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "cashbook", "title": "가계부", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

async fn call_tool(ctx: &tools::ToolCtx<'_>, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "도구 이름(name)이 없습니다".to_string()))?;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    match tools::call(ctx, name, args).await {
        None => Err((-32602, format!("없는 도구입니다: {name}"))),
        Some(Ok(v)) => Ok(json!({ "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v, "isError": false })),
        // 인자 오류 등은 모델이 읽고 고쳐서 다시 부르도록 도구 결과로 돌려준다
        Some(Err(msg)) => Ok(json!({ "content": [{ "type": "text", "text": msg }], "isError": true })),
    }
}

/// 메시지 하나를 처리한다. 알림·응답(id 없음, method 없음)에는 답하지 않는다.
async fn handle(ctx: &tools::ToolCtx<'_>, msg: Value) -> Option<Value> {
    let Value::Object(obj) = msg else { return Some(rpc_error(Value::Null, -32600, "JSON-RPC 메시지는 객체여야 합니다")) };
    let method = obj.get("method").and_then(Value::as_str)?;
    let id = obj.get("id").cloned()?;
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => call_tool(ctx, &params).await,
        _ => Err((-32601, format!("지원하지 않는 메서드입니다: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => rpc_error(id, code, &message),
    })
}

#[post("/mcp", data = "<body>")]
async fn mcp_post(caller: McpCaller, state: &State<AppState>, schema: &State<CashbookSchema>, body: Data<'_>) -> McpReply {
    let user = match caller {
        McpCaller::User(u) => u,
        McpCaller::Unauthorized { invalid_token } => {
            let challenge = McpCaller::challenge(&state.config, invalid_token);
            let body = json!({ "error": "invalid_token", "error_description": "로그인(OAuth) 또는 API 토큰이 필요합니다" });
            return McpReply::Unauthorized(body.to_string(), Header::new("WWW-Authenticate", challenge));
        }
        McpCaller::Forbidden(reason) => return McpReply::Forbidden(json!({ "error": reason }).to_string()),
        McpCaller::Error => return McpReply::Error(json!({ "error": "서버 오류" }).to_string()),
    };
    let raw = match body.open(256.kibibytes()).into_string().await {
        Ok(s) if s.is_complete() => s.into_inner(),
        _ => return McpReply::BadRequest(rpc_error(Value::Null, -32600, "요청 본문이 너무 크거나 읽을 수 없습니다").to_string()),
    };
    let msg: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return McpReply::BadRequest(rpc_error(Value::Null, -32700, &format!("JSON 해석 실패: {e}")).to_string()),
    };
    let ctx = tools::ToolCtx { schema, db: &state.db, config: &state.config, user };
    let out = match msg {
        Value::Array(items) => {
            let mut replies = Vec::new();
            for m in items {
                replies.extend(handle(&ctx, m).await);
            }
            (!replies.is_empty()).then_some(Value::Array(replies))
        }
        m => handle(&ctx, m).await,
    };
    match out {
        Some(v) => McpReply::Ok(v.to_string()),
        None => McpReply::Accepted(()),
    }
}

#[derive(Responder)]
#[response(status = 405)]
pub struct NotAllowed((), Header<'static>);

/// 서버가 먼저 보내는 스트림(SSE)과 세션은 쓰지 않는다
#[get("/mcp")]
fn mcp_get() -> NotAllowed {
    NotAllowed((), Header::new("Allow", "POST"))
}

#[delete("/mcp")]
fn mcp_delete() -> NotAllowed {
    NotAllowed((), Header::new("Allow", "POST"))
}

/// `/api` 아래
pub fn api_routes() -> Vec<Route> {
    let mut r = routes![mcp_post, mcp_get, mcp_delete];
    r.extend(auth::api_routes());
    r
}

/// `/` 아래
pub fn root_routes() -> Vec<Route> {
    auth::root_routes()
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Header, Status};
    use rocket::local::asynchronous::Client;
    use serde_json::{Value, json};

    use crate::config::AppConfig;
    use crate::{AppState, build_rocket};

    /// 인증 없는 로컬 실행 (기존 데이터 계정 id 1)
    async fn local() -> Client {
        let db = crate::db::connect_in_memory().await;
        let figment = rocket::figment::Figment::from(rocket::Config::debug_default())
            .merge(("secret_key", "2a".repeat(32)))
            .merge(("log_level", "off"));
        Client::tracked(build_rocket(figment, AppState::new(AppConfig::for_tests(), db))).await.unwrap()
    }

    async fn graphql(c: &Client, query: &str) -> Value {
        let res = c
            .post("/api/graphql")
            .header(ContentType::JSON)
            .header(Header::new("Host", "localhost"))
            .body(json!({ "query": query }).to_string())
            .dispatch()
            .await;
        let v: Value = serde_json::from_str(&res.into_string().await.unwrap()).unwrap();
        assert!(v["errors"].is_null(), "{query}: {v}");
        v["data"].clone()
    }

    async fn post(c: &Client, body: &str) -> (Status, Value) {
        let res = c.post("/api/mcp").header(ContentType::JSON).header(Header::new("Host", "localhost")).body(body).dispatch().await;
        let status = res.status();
        let text = res.into_string().await.unwrap_or_default();
        (status, if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap() })
    }

    async fn rpc(c: &Client, method: &str, params: Value) -> Value {
        let (status, v) = post(c, &json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params }).to_string()).await;
        assert_eq!(status, Status::Ok, "{v}");
        assert_eq!(v["id"], 7);
        v
    }

    /// 도구 결과 (isError, 내용)
    async fn call(c: &Client, name: &str, args: Value) -> (bool, Value) {
        let v = rpc(c, "tools/call", json!({ "name": name, "arguments": args })).await;
        let r = &v["result"];
        let is_error = r["isError"].as_bool().unwrap();
        let text = r["content"][0]["text"].as_str().unwrap();
        if is_error {
            return (true, json!(text));
        }
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), r["structuredContent"]);
        (false, r["structuredContent"].clone())
    }

    #[tokio::test]
    async fn protocol_basics() {
        let c = local().await;
        let v = rpc(&c, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } })).await;
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(v["result"]["serverInfo"]["name"], "cashbook");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert!(v["result"]["instructions"].as_str().unwrap().contains("get_context"));
        let v = rpc(&c, "initialize", json!({ "protocolVersion": "1999-01-01" })).await;
        assert_eq!(v["result"]["protocolVersion"], "2025-11-25", "모르는 버전이면 최신");

        // 알림에는 본문 없이 202
        let (status, v) = post(&c, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await;
        assert_eq!((status, v), (Status::Accepted, Value::Null));

        let tools = rpc(&c, "tools/list", json!({})).await["result"]["tools"].clone();
        let names: Vec<&str> = tools.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            ["get_context", "search_transactions", "get_monthly_summary", "get_available_funds", "get_card_performance", "get_card_statement", "list_recurring_rules"]
        );
        for t in tools.as_array().unwrap() {
            assert_eq!(t["annotations"]["readOnlyHint"], true, "{t}");
            assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        }

        assert_eq!(rpc(&c, "ping", json!({})).await["result"], json!({}));
        assert_eq!(rpc(&c, "resources/list", json!({})).await["error"]["code"], -32601);
        assert_eq!(rpc(&c, "tools/call", json!({ "name": "delete_everything" })).await["error"]["code"], -32602);

        // 배열로 보내면 답도 배열 (알림은 빠진다)
        let (status, v) = post(
            &c,
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#,
        )
        .await;
        assert_eq!(status, Status::Ok);
        assert_eq!(v.as_array().unwrap().iter().map(|r| r["id"].clone()).collect::<Vec<_>>(), [json!(1), json!(2)]);

        let (status, v) = post(&c, "{not json").await;
        assert_eq!((status, v["error"]["code"].clone()), (Status::BadRequest, json!(-32700)));
        assert_eq!(c.get("/api/mcp").header(Header::new("Host", "localhost")).dispatch().await.status(), Status::MethodNotAllowed);

        // 인증 없는 로컬 실행은 localhost 주소로만, 다른 사이트의 브라우저 요청은 막는다
        let res = c.post("/api/mcp").header(ContentType::JSON).header(Header::new("Host", "attacker.example")).body("{}").dispatch().await;
        assert_eq!(res.status(), Status::Forbidden);
        let res = c
            .post("/api/mcp")
            .header(ContentType::JSON)
            .header(Header::new("Host", "localhost"))
            .header(Header::new("Origin", "https://evil.example"))
            .body("{}")
            .dispatch()
            .await;
        assert_eq!(res.status(), Status::Forbidden);
        // 인증이 꺼져 있으면 OAuth 메타데이터도 없다
        assert_eq!(c.get("/.well-known/oauth-authorization-server").dispatch().await.status(), Status::NotFound);
    }

    #[tokio::test]
    async fn tools_read_account_data_with_names_and_kinds() {
        let c = local().await;
        let d = graphql(&c, r#"mutation { createLedger(input: {name: "테스트"}) { id } }"#).await;
        let ledger = d["createLedger"]["id"].as_i64().unwrap();
        let food = graphql(&c, &format!(r#"mutation {{ createCategory(input: {{ledgerId: {ledger}, name: "외식", monthlyBudget: 100000}}) {{ id }} }}"#)).await
            ["createCategory"]["id"]
            .as_i64()
            .unwrap();
        let salary = graphql(&c, &format!(r#"mutation {{ createCategory(input: {{ledgerId: {ledger}, name: "월급", kind: INCOME}}) {{ id }} }}"#)).await
            ["createCategory"]["id"]
            .as_i64()
            .unwrap();
        let card = graphql(
            &c,
            r#"mutation { createPaymentMethod(input: {name: "신한카드", kind: CREDIT_CARD, paymentDay: 15, closingDay: 31, closingMonthOffset: -1}) { id } }"#,
        )
        .await["createPaymentMethod"]["id"]
            .as_i64()
            .unwrap();
        let cash = graphql(&c, r#"mutation { createPaymentMethod(input: {name: "현금", kind: CASH}) { id } }"#).await["createPaymentMethod"]["id"]
            .as_i64()
            .unwrap();
        graphql(
            &c,
            &format!(
                r#"mutation {{ createTransactions(inputs: [
                  {{ledgerId: {ledger}, date: "2026-11-03", amount: 30000, categoryId: {food}, paymentMethodId: {card}, description: "저녁"}},
                  {{ledgerId: {ledger}, date: "2026-11-05", amount: -10000, categoryId: {food}, paymentMethodId: {card}, description: "저녁 취소"}},
                  {{ledgerId: {ledger}, date: "2026-11-07", amount: -20000, categoryId: {food}, paymentMethodId: {cash}, description: "회식비 정산"}},
                  {{ledgerId: {ledger}, date: "2026-11-08", amount: 3000000, categoryId: {salary}, description: "월급"}}
                ]) {{ id }} }}"#
            ),
        )
        .await;

        let (err, ctx) = call(&c, "get_context", json!({})).await;
        assert!(!err, "{ctx}");
        let l = ctx["ledgers"].as_array().unwrap().iter().find(|l| l["name"] == "테스트").unwrap().clone();
        assert_eq!(l["expenseCategories"], json!([{"id": food, "name": "외식", "monthlyBudget": 100000}]));
        assert_eq!(l["incomeCategories"], json!([{"id": salary, "name": "월급"}]));
        let pm = ctx["paymentMethods"].as_array().unwrap().iter().find(|p| p["id"] == card).unwrap().clone();
        assert_eq!((pm["name"].clone(), pm["kind"].clone()), (json!("신한카드"), json!("CREDIT_CARD")));
        assert!(ctx["today"].is_string());

        // 지출 카테고리의 − 금액: 카드면 환불, 현금이면 정산. 소비 합계는 순액
        let (err, s) = call(&c, "search_transactions", json!({ "month": "2026-11", "ledgerId": ledger, "sort": "date_asc" })).await;
        assert!(!err, "{s}");
        let rows: Vec<(String, String, Value)> = s["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["description"].as_str().unwrap().to_string(), t["kind"].as_str().unwrap().to_string(), t["paymentMethod"].clone()))
            .collect();
        assert_eq!(
            rows,
            [
                ("저녁".into(), "지출".into(), json!("신한카드")),
                ("저녁 취소".into(), "환불".into(), json!("신한카드")),
                ("회식비 정산".into(), "정산".into(), json!("현금")),
                ("월급".into(), "수입".into(), Value::Null),
            ]
        );
        assert_eq!(s["totals"]["spending"], 0);
        assert_eq!(s["totals"]["income"], 3_000_000);
        assert_eq!(s["totals"]["matched"], 4);
        assert_eq!(s["hasMore"], false);
        assert!(s["totals"].get("plannedSpending").is_none(), "확정만 찾으면 예정 합계는 빼고 보여 준다: {s}");
        let (_, s) = call(&c, "search_transactions", json!({ "query": "정산", "limit": 1 })).await;
        assert_eq!(s["transactions"][0]["category"], "외식");

        let (err, m) = call(&c, "get_monthly_summary", json!({ "month": "2026-11", "ledgerId": ledger })).await;
        assert!(!err, "{m}");
        assert_eq!((m["ledger"].clone(), m["spending"].clone(), m["income"].clone()), (json!("테스트"), json!(0), json!(3_000_000)));
        assert_eq!(m["balance"], 3_000_000, "장부를 정하면 월잔액");
        let cats: Vec<&str> = m["categories"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert!(cats.contains(&"외식") && cats.contains(&"월급"), "{cats:?}");
        let (_, all) = call(&c, "get_monthly_summary", json!({ "month": "2026-11" })).await;
        assert_eq!(all["ledger"], "전체 장부");
        assert!(all.get("balance").is_none());

        // 11월 이용분은 12월 결제 청구서 (환불 차감)
        let (err, st) = call(&c, "get_card_statement", json!({ "paymentMethodId": card, "month": "2026-12" })).await;
        assert!(!err, "{st}");
        assert_eq!(st["statement"]["confirmedAmount"], 20000);
        assert_eq!(st["statement"]["periodStart"], "2026-11-01");
        assert_eq!(st["lines"].as_array().unwrap().len(), 2);
        let (err, msg) = call(&c, "get_card_statement", json!({ "paymentMethodId": cash })).await;
        assert!(err && msg.as_str().unwrap().contains("카드가 아닙니다"), "{msg}");
        let (err, msg) = call(&c, "get_card_statement", json!({ "paymentMethodId": 9999 })).await;
        assert!(err, "{msg}");

        let (err, cards) = call(&c, "get_card_performance", json!({ "month": "2026-11" })).await;
        assert!(!err, "{cards}");
        let row = &cards["cards"][0];
        assert_eq!((row["card"].clone(), row["spending"].clone()), (json!("신한카드"), json!(20000)));

        let (err, f) = call(&c, "get_available_funds", json!({})).await;
        assert!(!err && f["available"].is_i64(), "{f}");
        let (err, r) = call(&c, "list_recurring_rules", json!({ "ledgerId": ledger })).await;
        assert_eq!((err, r["rules"].clone()), (false, json!([])));

        // 잘못된 인자는 모델이 고칠 수 있게 도구 오류로
        let (err, msg) = call(&c, "search_transactions", json!({ "month": "11월" })).await;
        assert!(err && msg.as_str().unwrap().contains("YYYY-MM"), "{msg}");
        let (err, msg) = call(&c, "search_transactions", json!({ "status": "DELETED" })).await;
        assert!(err, "{msg}");
        let (err, msg) = call(&c, "search_transactions", json!({ "ledgerId": "테스트" })).await;
        assert!(err && msg.as_str().unwrap().contains("인자 오류"), "{msg}");
    }
}
