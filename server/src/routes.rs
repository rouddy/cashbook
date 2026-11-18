//! Rocket 라우트
//! - `/api/graphql` (POST, JSON) — 인증 필요
//! - `/api/graphiql` — GraphQL 탐색기
//! - `/api/session`, `/api/login`, `/api/logout`
//! - `/api/ingest` (POST, JSON 또는 text/plain) — 자동 입력, 인증 필요(API 토큰 또는 로그인 세션)
//! - `/api/ingest/options` (GET) — 자동 입력 규칙에서 고를 장부·카테고리·결제수단 이름 (같은 인증)
//! - `/api/health`
//! - 그 외 경로는 React 빌드(`CASHBOOK_WEB_DIR`)를 서빙하고, 없는 경로는 index.html (SPA 라우팅)

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use async_graphql::http::GraphiQLSource;
use async_graphql_rocket::{GraphQLRequest, GraphQLResponse};
use rocket::fs::NamedFile;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::response::content::RawHtml;
use rocket::serde::json::Json;
use rocket::{Catcher, Request, Route, State, catch, catchers, get, post, routes};
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::auth::{
    Auth, ForbiddenReason, FullAccess, SESSION_COOKIE, SESSION_DAYS, check_password, session_valid, session_value,
};
use crate::domain::ingest::{self, IngestError, IngestInput, IngestOutcome};
use crate::graphql::{CashbookSchema, Lookup};

type ApiError = (Status, Json<Value>);

fn api_error(status: Status, message: impl Into<String>) -> ApiError {
    (status, Json(json!({ "ok": false, "error": message.into() })))
}

#[post("/graphql", data = "<request>", format = "json")]
async fn graphql(schema: &State<CashbookSchema>, state: &State<AppState>, _auth: FullAccess, request: GraphQLRequest) -> GraphQLResponse {
    request.data(Lookup::new(state.db.clone())).execute(schema.inner()).await
}

#[get("/graphiql")]
fn graphiql() -> RawHtml<String> {
    RawHtml(GraphiQLSource::build().endpoint("/api/graphql").finish())
}

#[get("/health")]
fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    auth_required: bool,
    authenticated: bool,
}

fn session_info(state: &AppState, cookies: &CookieJar<'_>) -> SessionInfo {
    match state.config.password.as_deref() {
        None => SessionInfo { auth_required: false, authenticated: true },
        Some(pw) => SessionInfo {
            auth_required: true,
            authenticated: cookies.get_private(SESSION_COOKIE).is_some_and(|c| session_valid(c.value(), pw)),
        },
    }
}

#[get("/session")]
fn session(state: &State<AppState>, cookies: &CookieJar<'_>) -> Json<SessionInfo> {
    Json(session_info(state, cookies))
}

#[derive(Deserialize)]
struct LoginBody {
    password: String,
}

#[post("/login", data = "<body>", format = "json")]
fn login(
    state: &State<AppState>,
    cookies: &CookieJar<'_>,
    client_ip: Option<IpAddr>,
    body: Json<LoginBody>,
) -> Result<Json<SessionInfo>, ApiError> {
    let Some(pw) = state.config.password.as_deref() else {
        return Ok(Json(session_info(state, cookies)));
    };
    if !state.login_limiter.allowed(client_ip) {
        return Err(api_error(Status::TooManyRequests, "로그인 시도가 너무 많습니다. 잠시 후 다시 시도하세요"));
    }
    if !check_password(&body.password, pw) {
        state.login_limiter.record_failure(client_ip);
        return Err(api_error(Status::Unauthorized, "비밀번호가 맞지 않습니다"));
    }
    let cookie = Cookie::build((SESSION_COOKIE, session_value(pw)))
        .http_only(true)
        .secure(state.config.secure_cookie)
        .same_site(rocket::http::SameSite::Strict)
        .max_age(rocket::time::Duration::days(SESSION_DAYS))
        .expires(rocket::time::OffsetDateTime::now_utc() + rocket::time::Duration::days(SESSION_DAYS));
    cookies.add_private(cookie);
    Ok(Json(SessionInfo { auth_required: true, authenticated: true }))
}

#[post("/logout")]
fn logout(state: &State<AppState>, cookies: &CookieJar<'_>) -> Json<SessionInfo> {
    cookies.remove_private(SESSION_COOKIE);
    Json(SessionInfo { auth_required: state.config.auth_enabled(), authenticated: !state.config.auth_enabled() })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IngestBody {
    Many(Vec<IngestInput>),
    One(Box<IngestInput>),
}

async fn ingest_one(state: &AppState, input: IngestInput) -> Result<IngestOutcome, IngestError> {
    let txn = state.db.begin().await?;
    let out = ingest::ingest(&txn, input, state.config.today()).await?;
    txn.commit().await?;
    Ok(out)
}

/// 자동 입력 응답. 단축어/자동화 앱에서 쓰기 쉽게 이름까지 풀어서 돌려준다.
async fn outcome_json(state: &AppState, out: &IngestOutcome) -> Value {
    let lookup = Lookup::new(state.db.clone());
    let tx = &out.transaction;
    let category = match tx.category_id {
        Some(id) => lookup.categories().await.ok().and_then(|m| m.get(&id).map(|c| c.name.clone())),
        None => None,
    };
    let payment_method = match tx.payment_method_id {
        Some(id) => lookup.payment_methods().await.ok().and_then(|m| m.get(&id).map(|p| p.name.clone())),
        None => None,
    };
    let ledger = lookup.ledgers().await.ok().and_then(|m| m.get(&tx.ledger_id).map(|l| l.name.clone()));
    json!({
        "ok": true,
        "created": out.created,
        "duplicate": out.duplicate,
        "matchedPlanned": out.matched_planned,
        "warnings": out.warnings,
        "parsed": out.parsed,
        "transaction": {
            "id": tx.id,
            "date": tx.date,
            "amount": tx.amount,
            "description": tx.description,
            "ledger": ledger,
            "category": category,
            "paymentMethod": payment_method,
            "status": tx.status,
            "installmentMonths": tx.installment_months,
        }
    })
}

/// JSON 한 건 또는 배열. 배열이면 건별로 처리해 결과 목록을 돌려준다.
#[post("/ingest", data = "<body>", format = "json")]
async fn ingest_json(state: &State<AppState>, _auth: Auth, body: Json<IngestBody>) -> Result<Json<Value>, ApiError> {
    match body.into_inner() {
        IngestBody::One(input) => match ingest_one(state, *input).await {
            Ok(out) => Ok(Json(outcome_json(state, &out).await)),
            Err(IngestError::Invalid(msg)) => Err(api_error(Status::UnprocessableEntity, msg)),
            Err(IngestError::Db(e)) => Err(api_error(Status::InternalServerError, e.to_string())),
        },
        IngestBody::Many(inputs) => {
            if inputs.len() > 200 {
                return Err(api_error(Status::PayloadTooLarge, "한 번에 200건까지 보낼 수 있습니다"));
            }
            let mut results = Vec::with_capacity(inputs.len());
            for input in inputs {
                results.push(match ingest_one(state, input).await {
                    Ok(out) => outcome_json(state, &out).await,
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                });
            }
            Ok(Json(json!({ "ok": true, "results": results })))
        }
    }
}

/// 카드 승인 문자 원문을 그대로 보낼 때 (Content-Type: text/plain)
#[post("/ingest", data = "<text>", format = "text/plain", rank = 2)]
async fn ingest_text(state: &State<AppState>, _auth: Auth, text: String) -> Result<Json<Value>, ApiError> {
    let input = IngestInput { text: Some(text), ..Default::default() };
    match ingest_one(state, input).await {
        Ok(out) => Ok(Json(outcome_json(state, &out).await)),
        Err(IngestError::Invalid(msg)) => Err(api_error(Status::UnprocessableEntity, msg)),
        Err(IngestError::Db(e)) => Err(api_error(Status::InternalServerError, e.to_string())),
    }
}

/// 자동 입력 규칙에서 고를 장부·카테고리·결제수단 이름 (안드로이드 앱의 선택 목록). 자동 입력 전용 토큰으로도 읽는다.
#[get("/ingest/options")]
async fn ingest_options(state: &State<AppState>, _auth: Auth) -> Result<Json<Value>, ApiError> {
    let options = ingest::options(&state.db).await.map_err(|e| api_error(Status::InternalServerError, e.to_string()))?;
    let mut body = serde_json::to_value(options).map_err(|e| api_error(Status::InternalServerError, e.to_string()))?;
    body["ok"] = json!(true);
    Ok(Json(body))
}

pub fn api() -> Vec<Route> {
    routes![graphql, graphiql, health, session, login, logout, ingest_json, ingest_text, ingest_options]
}

const NOT_BUILT: &str = r#"<!doctype html><meta charset="utf-8"><title>가계부</title>
<body style="font-family:sans-serif;padding:40px;line-height:1.6">
<h1>웹 화면이 아직 빌드되지 않았습니다</h1>
<p><code>web</code> 디렉터리에서 <code>npm install &amp;&amp; npm run build</code>를 실행하거나,
개발 중이면 <code>npm run dev</code>로 띄운 주소를 여세요.</p>
<p>GraphQL: <a href="/api/graphiql">/api/graphiql</a></p></body>"#;

pub enum WebResponse {
    File(NamedFile),
    Missing(RawHtml<&'static str>),
}

impl<'r> rocket::response::Responder<'r, 'static> for WebResponse {
    fn respond_to(self, req: &'r Request<'_>) -> rocket::response::Result<'static> {
        match self {
            WebResponse::File(f) => f.respond_to(req),
            WebResponse::Missing(html) => html.respond_to(req),
        }
    }
}

fn is_safe(path: &Path) -> bool {
    path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[get("/<path..>", rank = 20)]
async fn spa(path: PathBuf, state: &State<AppState>) -> Option<WebResponse> {
    if path.starts_with("api") || !is_safe(&path) {
        return None;
    }
    let root = &state.config.web_dir;
    let candidate = root.join(&path);
    if !path.as_os_str().is_empty() && candidate.is_file() {
        return NamedFile::open(candidate).await.ok().map(WebResponse::File);
    }
    // 확장자가 있는 정적 파일이 없으면 404 (index.html로 대신하지 않는다)
    if path.extension().is_some() {
        return None;
    }
    match NamedFile::open(root.join("index.html")).await {
        Ok(f) => Some(WebResponse::File(f)),
        Err(_) => Some(WebResponse::Missing(RawHtml(NOT_BUILT))),
    }
}

pub fn web() -> Vec<Route> {
    routes![spa]
}

#[catch(401)]
fn unauthorized() -> Json<Value> {
    Json(json!({ "ok": false, "error": "로그인이 필요하거나 API 토큰이 올바르지 않습니다" }))
}

#[catch(403)]
fn forbidden(req: &Request) -> Json<Value> {
    let reason = req.local_cache(|| ForbiddenReason("허용되지 않은 요청입니다")).0;
    Json(json!({ "ok": false, "error": reason }))
}

#[catch(404)]
fn not_found() -> Json<Value> {
    Json(json!({ "ok": false, "error": "없는 API입니다" }))
}

#[catch(415)]
fn unsupported() -> Json<Value> {
    Json(json!({ "ok": false, "error": "Content-Type은 application/json 이어야 합니다" }))
}

#[catch(422)]
fn unprocessable() -> Json<Value> {
    Json(json!({ "ok": false, "error": "요청 본문을 해석할 수 없습니다 (JSON 형식과 필드 타입을 확인하세요)" }))
}

#[catch(400)]
fn bad_request() -> Json<Value> {
    Json(json!({ "ok": false, "error": "잘못된 요청입니다" }))
}

pub fn api_catchers() -> Vec<Catcher> {
    catchers![unauthorized, forbidden, not_found, unsupported, unprocessable, bad_request]
}
