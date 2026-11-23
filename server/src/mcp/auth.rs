//! MCP 클라이언트를 위한 OAuth 2.1 인가 서버와 `/api/mcp` 인증.
//!
//! OpenAI Dots·ChatGPT 플러그인은 정적 API 키를 보내지 못하고 OAuth(또는 인증 없음)로만 붙으므로 가계부 서버가 직접
//! 인가 서버가 된다. 계정 확인은 기존 Google 로그인 세션을 쓴다.
//!
//! - 메타데이터: `/.well-known/oauth-protected-resource[/api/mcp]`(RFC 9728), `/.well-known/oauth-authorization-server`(RFC 8414)
//! - 동적 클라이언트 등록 `/api/oauth/register`(RFC 7591). 누구나 등록하지만 계정 데이터는 그 계정이 승인해야 읽는다.
//! - 승인 화면 `/oauth/authorize` (GET 화면, POST 허용·거부). `/api/` 밖이라 Google 로그인 뒤 돌아올 수 있다.
//! - 토큰 `/api/oauth/token`: authorization_code(PKCE S256 필수) + refresh_token(쓸 때마다 둘 다 교체)
//! - 발급한 토큰은 `cashbook:read` 범위이고 MCP 주소에만 쓸 수 있다(`/api/graphql`·자동 입력에는 못 쓴다).
//!
//! 인가 코드는 메모리에 5분만 둔다 (서버 한 대 — replicas 1. 재시작되면 다시 승인).
//! Google 로그인을 설정하지 않은 로컬 실행에서는 OAuth를 쓰지 않고 `/api/mcp`가 기존 데이터 계정(id 1)으로 동작한다.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Duration, Utc};
use rand::{Rng, distr::Alphanumeric};
use rocket::form::{Form, FromForm};
use rocket::http::{CookieJar, Header, RawStr, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::Redirect;
use rocket::response::content::RawHtml;
use rocket::serde::json::Json;
use rocket::{Responder, Route, State, get, post, routes};
use sea_orm::sea_query::Query;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::auth::{SESSION_COOKIE, bearer_token, host_allowed_without_auth, origin_allowed, session_user, sha256_hex, verify_token};
use crate::config::AppConfig;
use crate::entity::{oauth_client, oauth_grant, user};
use crate::graphql::CurrentUser;

/// 발급하는 유일한 범위: 조회
pub const SCOPE: &str = "cashbook:read";
const ACCESS_MINUTES: i64 = 60;
/// 이 기간 안에 한 번이라도 리프레시하면 연결이 이어진다 (Dots처럼 계속 도는 에이전트)
const REFRESH_DAYS: i64 = 90;
const CODE_MINUTES: i64 = 5;
/// 승인한 연결이 없는 클라이언트는 이 기간 뒤 지운다
const UNUSED_CLIENT_DAYS: i64 = 30;
const MAX_CLIENTS: u64 = 500;
const MAX_REDIRECT_URIS: usize = 10;

/// OAuth를 쓰는 서버의 기준 주소 (Google 로그인을 설정하지 않았으면 None)
pub fn issuer(config: &AppConfig) -> Option<&str> {
    config.google.as_ref().and(config.public_url.as_deref())
}

/// MCP 엔드포인트 = 토큰의 리소스(audience)
pub fn mcp_resource(base: &str) -> String {
    format!("{base}/api/mcp")
}

fn resource_metadata_url(base: &str) -> String {
    format!("{base}/.well-known/oauth-protected-resource/api/mcp")
}

/// 클라이언트가 보낸 `resource`가 이 MCP 서버를 가리키면 정규화한 리소스. 서버 주소만 보내도 받는다.
fn canonical_resource(base: &str, requested: &str) -> Option<String> {
    let r = requested.trim().trim_end_matches('/');
    let mcp = mcp_resource(base);
    (r == base || r == mcp).then_some(mcp)
}

fn random(len: usize) -> String {
    rand::rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

// ───────────────────────── 인가 코드 (메모리) ─────────────────────────

struct PendingCode {
    client: i32,
    redirect_uri: String,
    challenge: String,
    resource: String,
    user_id: i32,
    expires_at: i64,
}

/// 발급한 인가 코드 (코드 원문 대신 SHA-256을 키로). 한 번 쓰면 지운다.
#[derive(Default)]
pub struct CodeStore(Mutex<HashMap<String, PendingCode>>);

impl CodeStore {
    fn put(&self, code: &str, pending: PendingCode) {
        let now = Utc::now().timestamp();
        let mut map = self.0.lock().unwrap();
        map.retain(|_, c| c.expires_at > now);
        map.insert(sha256_hex(code), pending);
    }

    fn take(&self, code: &str) -> Option<PendingCode> {
        let now = Utc::now().timestamp();
        self.0.lock().unwrap().remove(&sha256_hex(code)).filter(|c| c.expires_at > now)
    }
}

// ───────────────────────── 메타데이터 ─────────────────────────

fn protected_resource_json(base: &str) -> Value {
    json!({
        "resource": mcp_resource(base),
        "authorization_servers": [base],
        "scopes_supported": [SCOPE],
        "bearer_methods_supported": ["header"],
        "resource_name": "가계부",
    })
}

#[get("/.well-known/oauth-protected-resource")]
fn protected_resource(state: &State<AppState>) -> Option<Json<Value>> {
    issuer(&state.config).map(|b| Json(protected_resource_json(b)))
}

#[get("/.well-known/oauth-protected-resource/api/mcp")]
fn protected_resource_mcp(state: &State<AppState>) -> Option<Json<Value>> {
    issuer(&state.config).map(|b| Json(protected_resource_json(b)))
}

#[get("/.well-known/oauth-authorization-server")]
fn authorization_server(state: &State<AppState>) -> Option<Json<Value>> {
    let base = issuer(&state.config)?;
    Some(Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/api/oauth/token"),
        "registration_endpoint": format!("{base}/api/oauth/register"),
        "scopes_supported": [SCOPE],
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "authorization_response_iss_parameter_supported": true,
    })))
}

// ───────────────────────── 동적 클라이언트 등록 ─────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct RegisterRequest {
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}

/// https, 또는 이 컴퓨터(localhost)로 돌아오는 http만. 조각(#)은 안 된다.
fn valid_redirect_uri(uri: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(uri) else { return false };
    if uri.len() > 512 || url.fragment().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

fn oauth_error(status: Status, error: &str, description: &str) -> (Status, Json<Value>) {
    (status, Json(json!({ "error": error, "error_description": description })))
}

/// 승인한 연결이 없는 오래된 클라이언트와 만료된 연결을 지운다
async fn cleanup(db: &DatabaseConnection) -> Result<(), DbErr> {
    let now = Utc::now();
    oauth_grant::Entity::delete_many().filter(oauth_grant::Column::RefreshExpiresAt.lt(now)).exec(db).await?;
    oauth_client::Entity::delete_many()
        .filter(oauth_client::Column::CreatedAt.lt(now - Duration::days(UNUSED_CLIENT_DAYS)))
        .filter(
            oauth_client::Column::Id.not_in_subquery(
                Query::select().column(oauth_grant::Column::OauthClientId).from(oauth_grant::Entity).to_owned(),
            ),
        )
        .exec(db)
        .await?;
    Ok(())
}

#[post("/oauth/register", data = "<body>")]
async fn register(state: &State<AppState>, body: String) -> (Status, Json<Value>) {
    if issuer(&state.config).is_none() {
        return oauth_error(Status::NotFound, "not_found", "OAuth를 쓰지 않는 서버입니다 (Google 로그인 미설정)");
    }
    let req: RegisterRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => return oauth_error(Status::BadRequest, "invalid_client_metadata", &format!("JSON 형식 오류: {e}")),
    };
    if req.redirect_uris.is_empty() || req.redirect_uris.len() > MAX_REDIRECT_URIS {
        return oauth_error(Status::BadRequest, "invalid_redirect_uri", "redirect_uris는 1~10개여야 합니다");
    }
    if let Some(bad) = req.redirect_uris.iter().find(|u| !valid_redirect_uri(u)) {
        return oauth_error(Status::BadRequest, "invalid_redirect_uri", &format!("https 또는 localhost 주소만 됩니다: {bad}"));
    }
    if req.grant_types.as_ref().is_some_and(|g| g.iter().any(|t| t != "authorization_code" && t != "refresh_token")) {
        return oauth_error(Status::BadRequest, "invalid_client_metadata", "grant_types는 authorization_code, refresh_token만 됩니다");
    }
    if req.response_types.as_ref().is_some_and(|r| r.iter().any(|t| t != "code")) {
        return oauth_error(Status::BadRequest, "invalid_client_metadata", "response_types는 code만 됩니다");
    }
    // RFC 7591: 생략하면 client_secret_basic
    let method = req.token_endpoint_auth_method.clone().unwrap_or_else(|| "client_secret_basic".into());
    let secret = match method.as_str() {
        "none" => None,
        "client_secret_basic" | "client_secret_post" => Some(format!("cbs_{}", random(40))),
        other => {
            return oauth_error(Status::BadRequest, "invalid_client_metadata", &format!("지원하지 않는 token_endpoint_auth_method: {other}"));
        }
    };
    let name: String = req.client_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("MCP 클라이언트").chars().take(100).collect();
    let db = &state.db;
    if let Err(e) = cleanup(db).await {
        eprintln!("OAuth 정리 실패: {e}");
    }
    match oauth_client::Entity::find().count(db).await {
        Ok(n) if n >= MAX_CLIENTS => {
            return oauth_error(Status::ServiceUnavailable, "temporarily_unavailable", "등록된 클라이언트가 너무 많습니다");
        }
        Ok(_) => {}
        Err(e) => return oauth_error(Status::InternalServerError, "server_error", &e.to_string()),
    }
    let client_id = format!("cbc_{}", random(32));
    let now = Utc::now();
    let saved = oauth_client::ActiveModel {
        client_id: Set(client_id.clone()),
        secret_hash: Set(secret.as_deref().map(sha256_hex)),
        name: Set(name.clone()),
        redirect_uris: Set(serde_json::to_string(&req.redirect_uris).expect("serialize uris")),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await;
    if let Err(e) = saved {
        return oauth_error(Status::InternalServerError, "server_error", &e.to_string());
    }
    let mut body = json!({
        "client_id": client_id,
        "client_id_issued_at": now.timestamp(),
        "client_name": name,
        "redirect_uris": req.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": method,
        "scope": SCOPE,
    });
    if let Some(s) = secret {
        body["client_secret"] = json!(s);
        body["client_secret_expires_at"] = json!(0);
    }
    (Status::Created, Json(body))
}

// ───────────────────────── 승인 화면 ─────────────────────────

/// 승인 요청 값 (GET 쿼리와 POST 폼이 같다. `decision`은 POST만)
#[derive(FromForm, Debug, Default, Clone)]
pub struct AuthorizeParams {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    scope: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    resource: Option<String>,
    decision: Option<String>,
}

impl AuthorizeParams {
    fn pairs(&self) -> Vec<(&'static str, &str)> {
        [
            ("response_type", &self.response_type),
            ("client_id", &self.client_id),
            ("redirect_uri", &self.redirect_uri),
            ("state", &self.state),
            ("scope", &self.scope),
            ("code_challenge", &self.code_challenge),
            ("code_challenge_method", &self.code_challenge_method),
            ("resource", &self.resource),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_deref().map(|v| (k, v)))
        .collect()
    }

    /// 이 승인 화면 주소 (로그인 뒤 돌아올 곳)
    fn page_path(&self) -> String {
        let mut url = reqwest::Url::parse("http://localhost/oauth/authorize").expect("static url");
        url.query_pairs_mut().extend_pairs(self.pairs());
        format!("{}?{}", url.path(), url.query().unwrap_or_default())
    }

    fn login_path(&self) -> String {
        let mut url = reqwest::Url::parse("http://localhost/api/auth/google").expect("static url");
        url.query_pairs_mut().append_pair("next", &self.page_path());
        format!("{}?{}", url.path(), url.query().unwrap_or_default())
    }
}

struct ValidRequest {
    client: oauth_client::Model,
    redirect_uri: String,
    state: Option<String>,
    challenge: String,
    resource: String,
}

enum Invalid {
    /// 클라이언트·돌아갈 주소를 믿을 수 없어 화면에 보여 준다
    Page(String),
    /// 클라이언트에게 오류를 돌려보낸다
    Redirect(String),
}

/// `redirect_uri`에 값을 붙인 주소 (이미 있는 쿼리는 둔다)
fn redirect_with(uri: &str, params: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse(uri).expect("등록할 때 확인한 주소");
    url.query_pairs_mut().extend_pairs(params);
    url.into()
}

fn error_redirect(base: &str, redirect_uri: &str, state: Option<&str>, error: &str, description: &str) -> String {
    let mut params = vec![("error", error), ("error_description", description), ("iss", base)];
    if let Some(s) = state {
        params.push(("state", s));
    }
    redirect_with(redirect_uri, &params)
}

fn valid_challenge(c: &str) -> bool {
    (43..=128).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'))
}

async fn validate(db: &DatabaseConnection, base: &str, p: &AuthorizeParams) -> Result<ValidRequest, Invalid> {
    let client_id = p.client_id.as_deref().filter(|c| !c.is_empty()).ok_or_else(|| Invalid::Page("client_id가 없습니다".into()))?;
    let client = oauth_client::Entity::find()
        .filter(oauth_client::Column::ClientId.eq(client_id))
        .one(db)
        .await
        .map_err(|e| Invalid::Page(format!("DB 오류: {e}")))?
        .ok_or_else(|| Invalid::Page("등록되지 않은 클라이언트입니다. 앱에서 연결을 처음부터 다시 시작하세요".into()))?;
    let registered = client.redirect_uris();
    let redirect_uri = match p.redirect_uri.as_deref() {
        Some(u) if registered.iter().any(|r| r == u) => u.to_string(),
        Some(_) => return Err(Invalid::Page("등록되지 않은 돌아갈 주소(redirect_uri)입니다".into())),
        None if registered.len() == 1 => registered[0].clone(),
        None => return Err(Invalid::Page("돌아갈 주소(redirect_uri)가 없습니다".into())),
    };
    let fail = |error: &str, description: &str| Invalid::Redirect(error_redirect(base, &redirect_uri, p.state.as_deref(), error, description));
    if p.response_type.as_deref() != Some("code") {
        return Err(fail("unsupported_response_type", "response_type=code만 지원합니다"));
    }
    let challenge = p.code_challenge.as_deref().filter(|c| valid_challenge(c)).ok_or_else(|| fail("invalid_request", "PKCE code_challenge가 필요합니다"))?;
    if p.code_challenge_method.as_deref() != Some("S256") {
        return Err(fail("invalid_request", "code_challenge_method는 S256이어야 합니다"));
    }
    let resource = match p.resource.as_deref() {
        Some(r) => canonical_resource(base, r).ok_or_else(|| fail("invalid_target", "이 서버의 MCP 주소가 아닙니다"))?,
        None => mcp_resource(base),
    };
    Ok(ValidRequest { challenge: challenge.to_string(), client, redirect_uri, state: p.state.clone(), resource })
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

const PAGE_STYLE: &str = "body{font-family:-apple-system,BlinkMacSystemFont,'Apple SD Gothic Neo','Malgun Gothic',sans-serif;background:#f5f6f8;color:#1f2328;margin:0}\
main{max-width:440px;margin:48px auto;padding:28px;background:#fff;border-radius:12px;box-shadow:0 1px 3px rgba(0,0,0,.12);line-height:1.6}\
h1{font-size:20px;margin:0 0 12px}ul{padding-left:20px}.muted{color:#656d76;font-size:14px}\
.row{display:flex;gap:8px;margin-top:20px}button,a.btn{flex:1;padding:10px;border-radius:8px;border:1px solid #d0d7de;background:#fff;font-size:15px;cursor:pointer;text-align:center;text-decoration:none;color:inherit}\
button.primary{background:#2f6feb;border-color:#2f6feb;color:#fff}button:disabled{opacity:.5;cursor:default}";

fn page(status: Status, title: &str, body: &str) -> AuthorizeReply {
    AuthorizeReply::Page((
        status,
        RawHtml(format!(
            "<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <title>{title}</title><style>{PAGE_STYLE}</style><body><main><h1>{title}</h1>{body}</main></body></html>",
            title = escape(title)
        )),
    ))
}

fn consent_page(p: &AuthorizeParams, req: &ValidRequest) -> AuthorizeReply {
    let host = reqwest::Url::parse(&req.redirect_uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let hidden: String = p
        .pairs()
        .into_iter()
        .map(|(k, v)| format!("<input type=\"hidden\" name=\"{k}\" value=\"{}\">", escape(v)))
        .collect();
    let body = format!(
        "<p><b>{name}</b>이(가) 가계부 데이터 조회를 요청합니다.</p>\
         <ul><li>거래 내역, 월 요약, 카드 실적·청구서, 여유자금, 반복 규칙을 읽습니다</li>\
         <li>입력·수정·삭제는 할 수 없습니다</li></ul>\
         <p class=\"muted\">허용하면 {host} 로 돌아갑니다. 연결은 설정 화면의 연결된 앱에서 끊을 수 있습니다.</p>\
         <p id=\"who\" class=\"muted\">로그인 정보를 확인하는 중…</p>\
         <form method=\"post\" action=\"/oauth/authorize\">{hidden}\
         <div class=\"row\"><button name=\"decision\" value=\"deny\">거부</button>\
         <button class=\"primary\" id=\"approve\" name=\"decision\" value=\"approve\" disabled>허용</button></div></form>\
         <div class=\"row\"><a class=\"btn\" id=\"login\" href=\"{login}\" hidden>Google로 로그인</a></div>\
         <script>fetch('/api/session').then(r=>r.json()).then(s=>{{const w=document.getElementById('who'),l=document.getElementById('login');\
         if(s.authenticated&&s.user){{w.textContent=(s.user.email||s.user.name||'')+' 계정으로 연결합니다';document.getElementById('approve').disabled=false;\
         l.textContent='다른 계정으로 로그인';l.hidden=false}}else{{w.textContent='먼저 이 가계부에 로그인하세요';l.hidden=false}}}})\
         .catch(()=>{{document.getElementById('who').textContent='로그인 정보를 확인하지 못했습니다'}})</script>",
        name = escape(&req.client.name),
        host = escape(&host),
        login = escape(&p.login_path()),
    );
    page(Status::Ok, "가계부 연결", &body)
}

/// 한 번 만들어 바로 돌려주는 응답이라 변형 크기 차이는 상관없다
#[allow(clippy::large_enum_variant)]
#[derive(Responder)]
pub enum AuthorizeReply {
    Page((Status, RawHtml<String>)),
    Redirect(Redirect),
}

#[get("/oauth/authorize?<params..>")]
async fn authorize_page(state: &State<AppState>, params: AuthorizeParams) -> AuthorizeReply {
    let Some(base) = issuer(&state.config) else {
        return page(Status::NotFound, "가계부 연결", "<p>이 서버는 외부 앱 연결(OAuth)을 쓰지 않습니다.</p>");
    };
    match validate(&state.db, base, &params).await {
        Ok(req) => consent_page(&params, &req),
        Err(Invalid::Page(msg)) => page(Status::BadRequest, "연결할 수 없습니다", &format!("<p>{}</p>", escape(&msg))),
        Err(Invalid::Redirect(to)) => AuthorizeReply::Redirect(Redirect::to(to)),
    }
}

/// 다른 사이트의 폼이 보낸 요청이 아닌지 (세션 쿠키도 SameSite=Strict라 다른 사이트에서는 실리지 않는다)
pub struct SameSiteForm(bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for SameSiteForm {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Infallible> {
        let Some(state) = req.rocket().state::<AppState>() else { return Outcome::Success(SameSiteForm(false)) };
        let h = req.headers();
        Outcome::Success(SameSiteForm(origin_allowed(&state.config, h.get_one("Origin"), h.get_one("Host"), h.get_one("X-Forwarded-Host"))))
    }
}

async fn session_account(db: &DatabaseConnection, cookies: &CookieJar<'_>) -> Option<i32> {
    let id = cookies.get_private(SESSION_COOKIE).and_then(|c| session_user(c.value()))?;
    user::Entity::find_by_id(id).one(db).await.ok().flatten().map(|u| u.id)
}

#[post("/oauth/authorize", data = "<form>")]
async fn authorize_submit(state: &State<AppState>, cookies: &CookieJar<'_>, same_site: SameSiteForm, form: Form<AuthorizeParams>) -> AuthorizeReply {
    let Some(base) = issuer(&state.config) else {
        return page(Status::NotFound, "가계부 연결", "<p>이 서버는 외부 앱 연결(OAuth)을 쓰지 않습니다.</p>");
    };
    if !same_site.0 {
        return page(Status::Forbidden, "연결할 수 없습니다", "<p>다른 사이트에서 보낸 요청은 받지 않습니다.</p>");
    }
    let params = form.into_inner();
    let req = match validate(&state.db, base, &params).await {
        Ok(r) => r,
        Err(Invalid::Page(msg)) => return page(Status::BadRequest, "연결할 수 없습니다", &format!("<p>{}</p>", escape(&msg))),
        Err(Invalid::Redirect(to)) => return AuthorizeReply::Redirect(Redirect::to(to)),
    };
    if params.decision.as_deref() != Some("approve") {
        let to = error_redirect(base, &req.redirect_uri, req.state.as_deref(), "access_denied", "사용자가 연결을 거부했습니다");
        return AuthorizeReply::Redirect(Redirect::to(to));
    }
    let Some(user_id) = session_account(&state.db, cookies).await else {
        return AuthorizeReply::Redirect(Redirect::to(params.login_path()));
    };
    let code = random(43);
    state.oauth_codes.put(
        &code,
        PendingCode {
            client: req.client.id,
            redirect_uri: req.redirect_uri.clone(),
            challenge: req.challenge,
            resource: req.resource,
            user_id,
            expires_at: Utc::now().timestamp() + CODE_MINUTES * 60,
        },
    );
    let mut out = vec![("code", code.as_str()), ("iss", base)];
    if let Some(s) = req.state.as_deref() {
        out.push(("state", s));
    }
    AuthorizeReply::Redirect(Redirect::to(redirect_with(&req.redirect_uri, &out)))
}

// ───────────────────────── 토큰 ─────────────────────────

#[derive(FromForm, Default)]
pub struct TokenForm {
    grant_type: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    resource: Option<String>,
}

/// `Authorization: Basic base64(client_id:client_secret)` (각각 폼 인코딩)
pub struct BasicCredentials(Option<(String, String)>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for BasicCredentials {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Infallible> {
        let parsed = req
            .headers()
            .get_one("Authorization")
            .and_then(|h| h.strip_prefix("Basic ").or_else(|| h.strip_prefix("basic ")))
            .and_then(|b| STANDARD.decode(b.trim()).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|pair| {
                let (id, secret) = pair.split_once(':')?;
                let decode = |s: &str| RawStr::new(s).url_decode().ok().map(|c| c.into_owned());
                Some((decode(id)?, decode(secret)?))
            });
        Outcome::Success(BasicCredentials(parsed))
    }
}

#[derive(Responder)]
#[response(content_type = "json")]
pub struct TokenReply {
    body: (Status, String),
    cache: Header<'static>,
    pragma: Header<'static>,
}

impl TokenReply {
    fn new(status: Status, body: Value) -> Self {
        Self { body: (status, body.to_string()), cache: Header::new("Cache-Control", "no-store"), pragma: Header::new("Pragma", "no-cache") }
    }

    fn error(status: Status, error: &str, description: &str) -> Self {
        Self::new(status, json!({ "error": error, "error_description": description }))
    }
}

fn token_error(error: &str, description: &str) -> TokenReply {
    let status = if error == "invalid_client" { Status::Unauthorized } else { Status::BadRequest };
    TokenReply::error(status, error, description)
}

async fn authenticate_client(
    db: &DatabaseConnection,
    basic: Option<(String, String)>,
    form: &TokenForm,
) -> Result<oauth_client::Model, TokenReply> {
    let (id, secret) = match basic {
        Some((id, secret)) => (id, Some(secret)),
        None => (form.client_id.clone().unwrap_or_default(), form.client_secret.clone()),
    };
    let client = oauth_client::Entity::find()
        .filter(oauth_client::Column::ClientId.eq(id))
        .one(db)
        .await
        .map_err(|e| TokenReply::error(Status::InternalServerError, "server_error", &e.to_string()))?
        .ok_or_else(|| token_error("invalid_client", "등록되지 않은 클라이언트입니다"))?;
    if let Some(hash) = &client.secret_hash
        && secret.as_deref().map(sha256_hex).as_ref() != Some(hash)
    {
        return Err(token_error("invalid_client", "client_secret이 맞지 않습니다"));
    }
    Ok(client)
}

struct IssuedTokens {
    access: String,
    refresh: String,
}

fn new_tokens() -> IssuedTokens {
    IssuedTokens { access: format!("cba_{}", random(40)), refresh: format!("cbr_{}", random(48)) }
}

fn token_body(t: &IssuedTokens) -> Value {
    json!({
        "access_token": t.access,
        "token_type": "Bearer",
        "expires_in": ACCESS_MINUTES * 60,
        "refresh_token": t.refresh,
        "scope": SCOPE,
    })
}

async fn exchange_code(state: &AppState, base: &str, client: &oauth_client::Model, form: &TokenForm) -> Result<Value, TokenReply> {
    let code = form.code.as_deref().ok_or_else(|| token_error("invalid_request", "code가 없습니다"))?;
    // 맞지 않아도 코드는 한 번 쓰면 끝
    let pending = state.oauth_codes.take(code).ok_or_else(|| token_error("invalid_grant", "인가 코드가 없거나 만료됐습니다"))?;
    if pending.client != client.id {
        return Err(token_error("invalid_grant", "다른 클라이언트의 인가 코드입니다"));
    }
    if form.redirect_uri.as_deref().is_some_and(|r| r != pending.redirect_uri) {
        return Err(token_error("invalid_grant", "redirect_uri가 승인 요청과 다릅니다"));
    }
    let verifier = form.code_verifier.as_deref().ok_or_else(|| token_error("invalid_request", "code_verifier가 없습니다"))?;
    if crate::oauth::challenge(verifier) != pending.challenge {
        return Err(token_error("invalid_grant", "PKCE 검증에 실패했습니다"));
    }
    if let Some(r) = form.resource.as_deref()
        && canonical_resource(base, r).as_deref() != Some(pending.resource.as_str())
    {
        return Err(token_error("invalid_target", "승인한 리소스와 다릅니다"));
    }
    let tokens = new_tokens();
    let now = Utc::now();
    oauth_grant::ActiveModel {
        user_id: Set(pending.user_id),
        oauth_client_id: Set(client.id),
        client_name: Set(client.name.clone()),
        scope: Set(SCOPE.into()),
        resource: Set(pending.resource),
        access_hash: Set(sha256_hex(&tokens.access)),
        access_expires_at: Set(now + Duration::minutes(ACCESS_MINUTES)),
        refresh_hash: Set(sha256_hex(&tokens.refresh)),
        refresh_expires_at: Set(now + Duration::days(REFRESH_DAYS)),
        created_at: Set(now),
        last_used_at: Set(None),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| TokenReply::error(Status::InternalServerError, "server_error", &e.to_string()))?;
    Ok(token_body(&tokens))
}

async fn refresh(state: &AppState, base: &str, client: &oauth_client::Model, form: &TokenForm) -> Result<Value, TokenReply> {
    let token = form.refresh_token.as_deref().ok_or_else(|| token_error("invalid_request", "refresh_token이 없습니다"))?;
    let db = &state.db;
    let server_error = |e: DbErr| TokenReply::error(Status::InternalServerError, "server_error", &e.to_string());
    let grant = oauth_grant::Entity::find()
        .filter(oauth_grant::Column::RefreshHash.eq(sha256_hex(token)))
        .one(db)
        .await
        .map_err(server_error)?
        .ok_or_else(|| token_error("invalid_grant", "연결이 끊겼거나 리프레시 토큰이 맞지 않습니다"))?;
    if grant.oauth_client_id != client.id {
        return Err(token_error("invalid_grant", "다른 클라이언트의 리프레시 토큰입니다"));
    }
    let now = Utc::now();
    if grant.refresh_expires_at < now {
        oauth_grant::Entity::delete_by_id(grant.id).exec(db).await.map_err(server_error)?;
        return Err(token_error("invalid_grant", "오래 쓰지 않아 연결이 만료됐습니다. 다시 연결하세요"));
    }
    if let Some(r) = form.resource.as_deref()
        && canonical_resource(base, r).as_deref() != Some(grant.resource.as_str())
    {
        return Err(token_error("invalid_target", "승인한 리소스와 다릅니다"));
    }
    let tokens = new_tokens();
    let mut active: oauth_grant::ActiveModel = grant.into();
    active.access_hash = Set(sha256_hex(&tokens.access));
    active.access_expires_at = Set(now + Duration::minutes(ACCESS_MINUTES));
    active.refresh_hash = Set(sha256_hex(&tokens.refresh));
    active.refresh_expires_at = Set(now + Duration::days(REFRESH_DAYS));
    active.update(db).await.map_err(server_error)?;
    Ok(token_body(&tokens))
}

#[post("/oauth/token", data = "<form>")]
async fn token(state: &State<AppState>, basic: BasicCredentials, form: Form<TokenForm>) -> TokenReply {
    let Some(base) = issuer(&state.config) else {
        return TokenReply::error(Status::NotFound, "not_found", "OAuth를 쓰지 않는 서버입니다");
    };
    let form = form.into_inner();
    let client = match authenticate_client(&state.db, basic.0, &form).await {
        Ok(c) => c,
        Err(reply) => return reply,
    };
    let result = match form.grant_type.as_deref() {
        Some("authorization_code") => exchange_code(state, base, &client, &form).await,
        Some("refresh_token") => refresh(state, base, &client, &form).await,
        _ => Err(token_error("unsupported_grant_type", "authorization_code, refresh_token만 지원합니다")),
    };
    match result {
        Ok(body) => TokenReply::new(Status::Ok, body),
        Err(reply) => reply,
    }
}

// ───────────────────────── /api/mcp 인증 ─────────────────────────

/// `/api/mcp`를 부른 쪽. 실패해도 라우트가 MCP 규격대로(401 + WWW-Authenticate) 답하도록 가드는 늘 성공한다.
pub enum McpCaller {
    User(CurrentUser),
    Unauthorized { invalid_token: bool },
    Forbidden(&'static str),
    Error,
}

impl McpCaller {
    /// 401에 붙이는 `WWW-Authenticate` (클라이언트가 여기서 OAuth 설정을 찾는다)
    pub fn challenge(config: &AppConfig, invalid_token: bool) -> String {
        let base = issuer(config).unwrap_or_default();
        let mut v = format!("Bearer resource_metadata=\"{}\", scope=\"{SCOPE}\"", resource_metadata_url(base));
        if invalid_token {
            v.push_str(", error=\"invalid_token\"");
        }
        v
    }
}

async fn caller_for_token(state: &AppState, token: &str) -> Result<Option<CurrentUser>, DbErr> {
    let config = &state.config;
    let as_current = |u: &user::Model| CurrentUser { id: u.id, is_admin: config.is_admin_email(u.email.as_deref()) };
    // 설정 화면에서 발급한 API 토큰(Claude Code 등 헤더를 직접 넣는 클라이언트)
    if token.starts_with("cb_") {
        let Some(t) = verify_token(&state.db, token).await? else { return Ok(None) };
        return Ok(user::Entity::find_by_id(t.user_id).one(&state.db).await?.as_ref().map(as_current));
    }
    let Some(base) = issuer(config) else { return Ok(None) };
    let Some(grant) = oauth_grant::Entity::find().filter(oauth_grant::Column::AccessHash.eq(sha256_hex(token))).one(&state.db).await? else {
        return Ok(None);
    };
    let now = Utc::now();
    if grant.access_expires_at < now || grant.resource != mcp_resource(base) || !grant.scope.split(' ').any(|s| s == SCOPE) {
        return Ok(None);
    }
    let Some(u) = user::Entity::find_by_id(grant.user_id).one(&state.db).await? else { return Ok(None) };
    if grant.last_used_at.is_none_or(|at| now - at > Duration::minutes(1)) {
        let mut active: oauth_grant::ActiveModel = grant.into();
        active.last_used_at = Set(Some(now));
        active.update(&state.db).await?;
    }
    Ok(Some(as_current(&u)))
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for McpCaller {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Infallible> {
        let Some(state) = req.rocket().state::<AppState>() else { return Outcome::Success(McpCaller::Error) };
        let h = req.headers();
        let host = h.get_one("Host");
        if !origin_allowed(&state.config, h.get_one("Origin"), host, h.get_one("X-Forwarded-Host")) {
            return Outcome::Success(McpCaller::Forbidden("다른 사이트에서 보낸 요청은 받지 않습니다 (CASHBOOK_ALLOWED_ORIGINS 참고)"));
        }
        if !state.config.auth_enabled() {
            if !host_allowed_without_auth(&state.config, host) {
                return Outcome::Success(McpCaller::Forbidden("인증 없이 실행 중이라 localhost 주소로만 접속할 수 있습니다"));
            }
            return Outcome::Success(McpCaller::User(CurrentUser { id: user::LEGACY_USER_ID, is_admin: true }));
        }
        let Some(token) = bearer_token(req) else { return Outcome::Success(McpCaller::Unauthorized { invalid_token: false }) };
        Outcome::Success(match caller_for_token(state, token).await {
            Ok(Some(u)) => McpCaller::User(u),
            Ok(None) => McpCaller::Unauthorized { invalid_token: true },
            Err(e) => {
                eprintln!("MCP 인증 확인 실패: {e}");
                McpCaller::Error
            }
        })
    }
}

/// `/api` 아래: 등록·토큰
pub fn api_routes() -> Vec<Route> {
    routes![register, token]
}

/// `/` 아래: 메타데이터·승인 화면
pub fn root_routes() -> Vec<Route> {
    routes![protected_resource, protected_resource_mcp, authorization_server, authorize_page, authorize_submit]
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rocket::http::{ContentType, Cookie, Header, Status};
    use rocket::local::asynchronous::{Client, LocalResponse};
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::{Value, json};

    use crate::auth::{SESSION_COOKIE, session_value};
    use crate::config::{AppConfig, GoogleConfig};
    use crate::entity::{ledger, user};
    use crate::{AppState, build_rocket};

    const BASE: &str = "https://cash.example.com";
    const CHATGPT: &str = "https://chatgpt.com/connector_platform_oauth_redirect";
    const VERIFIER: &str = "verifier-0123456789-abcdefghijklmnopqrstuvwxyz-ABCDEFG";

    async fn client() -> Client {
        let db = crate::db::connect_in_memory().await;
        // 계정 1(기존 데이터)과 2에 장부 하나씩
        user::ActiveModel { id: Set(2), email: Set(Some("two@example.com".into())), created_at: Set(Utc::now()), ..Default::default() }
            .insert(&db)
            .await
            .unwrap();
        for (uid, name) in [(1, "하나 장부"), (2, "둘 장부")] {
            ledger::ActiveModel {
                user_id: Set(uid),
                name: Set(name.into()),
                is_default: Set(false),
                sort_order: Set(0),
                created_at: Set(Utc::now()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let mut config = AppConfig::for_tests();
        config.google = Some(GoogleConfig {
            client_id: "cid".into(),
            client_secret: "csecret".into(),
            redirect_uri: format!("{BASE}/api/auth/google/callback"),
            auth_url: GoogleConfig::AUTH_URL.into(),
            token_url: "http://127.0.0.1:9/token".into(),
            userinfo_url: "http://127.0.0.1:9/userinfo".into(),
        });
        config.public_url = Some(BASE.into());
        let figment = rocket::figment::Figment::from(rocket::Config::debug_default())
            .merge(("secret_key", "3b".repeat(32)))
            .merge(("log_level", "off"));
        Client::untracked(build_rocket(figment, AppState::new(config, db))).await.unwrap()
    }

    fn form(pairs: &[(&str, &str)]) -> String {
        let mut url = reqwest::Url::parse("http://localhost/").unwrap();
        url.query_pairs_mut().extend_pairs(pairs);
        url.query().unwrap_or_default().to_string()
    }

    fn query_of(location: &str) -> Vec<(String, String)> {
        reqwest::Url::parse(&format!("http://localhost{location}").replace("http://localhosthttps://", "https://"))
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn param(location: &str, key: &str) -> Option<String> {
        query_of(location).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    async fn json_of(res: LocalResponse<'_>) -> Value {
        serde_json::from_str(&res.into_string().await.unwrap_or_default()).unwrap_or(Value::Null)
    }

    fn session(user_id: i32) -> Cookie<'static> {
        Cookie::new(SESSION_COOKIE, session_value(user_id))
    }

    async fn register(c: &Client, body: Value) -> (Status, Value) {
        let res = c.post("/api/oauth/register").header(ContentType::JSON).body(body.to_string()).dispatch().await;
        (res.status(), json_of(res).await)
    }

    fn authorize_params(client_id: &str) -> Vec<(&'static str, String)> {
        vec![
            ("response_type", "code".into()),
            ("client_id", client_id.into()),
            ("redirect_uri", CHATGPT.into()),
            ("state", "st-1".into()),
            ("scope", super::SCOPE.into()),
            ("code_challenge", crate::oauth::challenge(VERIFIER)),
            ("code_challenge_method", "S256".into()),
            ("resource", format!("{BASE}/api/mcp")),
        ]
    }

    fn pairs<'a>(p: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
        p.iter().map(|(k, v)| (*k, v.as_str())).collect()
    }

    /// 계정 `user`로 승인하고 받은 인가 코드
    async fn approve(c: &Client, client_id: &str, user: i32) -> String {
        let mut p = authorize_params(client_id);
        p.push(("decision", "approve".into()));
        let res = c.post("/oauth/authorize").header(ContentType::Form).private_cookie(session(user)).body(form(&pairs(&p))).dispatch().await;
        assert_eq!(res.status(), Status::SeeOther);
        let to = res.headers().get_one("Location").unwrap().to_string();
        assert!(to.starts_with(CHATGPT), "{to}");
        assert_eq!(param(&to, "state").as_deref(), Some("st-1"));
        assert_eq!(param(&to, "iss").as_deref(), Some(BASE));
        param(&to, "code").unwrap()
    }

    async fn token(c: &Client, body: &[(&str, &str)], basic: Option<&str>) -> (Status, Value) {
        let mut req = c.post("/api/oauth/token").header(ContentType::Form).body(form(body));
        if let Some(b) = basic {
            req = req.header(Header::new("Authorization", format!("Basic {b}")));
        }
        let res = req.dispatch().await;
        assert_eq!(res.headers().get_one("Cache-Control"), Some("no-store"));
        (res.status(), json_of(res).await)
    }

    async fn exchange(c: &Client, client_id: &str, code: &str, verifier: &str) -> (Status, Value) {
        let resource = format!("{BASE}/api/mcp");
        let body = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", CHATGPT),
            ("client_id", client_id),
            ("code_verifier", verifier),
            ("resource", resource.as_str()),
        ];
        token(c, &body, None).await
    }

    async fn mcp(c: &Client, bearer: Option<&str>, method: &str, params: Value) -> (Status, Option<String>, Value) {
        let mut req = c
            .post("/api/mcp")
            .header(ContentType::JSON)
            .body(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string());
        if let Some(t) = bearer {
            req = req.header(Header::new("Authorization", format!("Bearer {t}")));
        }
        let res = req.dispatch().await;
        let challenge = res.headers().get_one("WWW-Authenticate").map(str::to_string);
        (res.status(), challenge, json_of(res).await)
    }

    async fn ledger_names(c: &Client, access: &str) -> Vec<String> {
        let (status, _, v) = mcp(c, Some(access), "tools/call", json!({ "name": "get_context" })).await;
        assert_eq!(status, Status::Ok, "{v}");
        v["result"]["structuredContent"]["ledgers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test]
    async fn dots_style_oauth_connects_reads_refreshes_and_revokes() {
        let c = client().await;

        // 토큰 없이 부르면 401과 함께 보호 리소스 메타데이터 주소를 알려 준다
        let (status, challenge, _) = mcp(&c, None, "initialize", json!({})).await;
        assert_eq!(status, Status::Unauthorized);
        let challenge = challenge.unwrap();
        assert!(challenge.contains(&format!("resource_metadata=\"{BASE}/.well-known/oauth-protected-resource/api/mcp\"")), "{challenge}");
        let prm = json_of(c.get("/.well-known/oauth-protected-resource/api/mcp").dispatch().await).await;
        assert_eq!(prm["resource"], format!("{BASE}/api/mcp"));
        assert_eq!(prm["authorization_servers"], json!([BASE]));
        let meta = json_of(c.get("/.well-known/oauth-authorization-server").dispatch().await).await;
        assert_eq!(meta["issuer"], BASE);
        assert_eq!(meta["code_challenge_methods_supported"], json!(["S256"]));
        assert_eq!(meta["authorization_endpoint"], format!("{BASE}/oauth/authorize"));
        assert_eq!(meta["registration_endpoint"], format!("{BASE}/api/oauth/register"));
        assert_eq!(meta["authorization_response_iss_parameter_supported"], true);

        // 동적 등록: https나 localhost로만 돌아갈 수 있다
        let (status, v) = register(&c, json!({ "redirect_uris": ["http://evil.example/cb"] })).await;
        assert_eq!((status, v["error"].clone()), (Status::BadRequest, json!("invalid_redirect_uri")));
        let (status, v) = register(&c, json!({ "client_name": "<b>ChatGPT</b>", "redirect_uris": [CHATGPT], "token_endpoint_auth_method": "none" })).await;
        assert_eq!(status, Status::Created, "{v}");
        assert!(v.get("client_secret").is_none());
        let client_id = v["client_id"].as_str().unwrap().to_string();

        // 승인 화면: 이름은 이스케이프해서 보여 준다
        let p = authorize_params(&client_id);
        let res = c.get(format!("/oauth/authorize?{}", form(&pairs(&p)))).dispatch().await;
        assert_eq!(res.status(), Status::Ok);
        let html = res.into_string().await.unwrap();
        assert!(html.contains("&lt;b&gt;ChatGPT&lt;/b&gt;") && !html.contains("<b>ChatGPT</b>"), "{html}");
        assert!(html.contains("chatgpt.com"));
        // 등록하지 않은 주소로는 돌려보내지 않는다
        let mut bad = p.clone();
        bad[2].1 = "https://evil.example/cb".into();
        assert_eq!(c.get(format!("/oauth/authorize?{}", form(&pairs(&bad)))).dispatch().await.status(), Status::BadRequest);
        // PKCE가 없으면 클라이언트에게 오류를 돌려준다
        let no_pkce: Vec<_> = p.iter().filter(|(k, _)| *k != "code_challenge").cloned().collect();
        let res = c.get(format!("/oauth/authorize?{}", form(&pairs(&no_pkce)))).dispatch().await;
        let to = res.headers().get_one("Location").unwrap().to_string();
        assert_eq!((param(&to, "error").as_deref(), param(&to, "state").as_deref()), (Some("invalid_request"), Some("st-1")));

        // 로그인 전에 허용하면 Google 로그인으로 보냈다가 이 승인 화면으로 돌아온다
        let mut approve_params = p.clone();
        approve_params.push(("decision", "approve".into()));
        let res = c.post("/oauth/authorize").header(ContentType::Form).body(form(&pairs(&approve_params))).dispatch().await;
        let to = res.headers().get_one("Location").unwrap().to_string();
        assert!(to.starts_with("/api/auth/google?"), "{to}");
        let next = param(&to, "next").unwrap();
        assert!(next.starts_with("/oauth/authorize?"), "{next}");
        assert_eq!(crate::oauth::safe_next(Some(&next)), next, "로그인 뒤 돌아갈 경로로 받아들여야 한다");

        // 거부하면 access_denied
        let mut deny = p.clone();
        deny.push(("decision", "deny".into()));
        let res = c.post("/oauth/authorize").header(ContentType::Form).private_cookie(session(1)).body(form(&pairs(&deny))).dispatch().await;
        assert_eq!(param(res.headers().get_one("Location").unwrap(), "error").as_deref(), Some("access_denied"));
        // 다른 사이트의 폼은 막는다
        let res = c
            .post("/oauth/authorize")
            .header(ContentType::Form)
            .header(Header::new("Origin", "https://evil.example"))
            .header(Header::new("Host", "cash.example.com"))
            .private_cookie(session(1))
            .body(form(&pairs(&approve_params)))
            .dispatch()
            .await;
        assert_eq!(res.status(), Status::Forbidden);

        // 인가 코드: PKCE가 맞아야 하고 한 번만 쓴다
        let code = approve(&c, &client_id, 1).await;
        let (status, v) = exchange(&c, &client_id, &code, "wrong-verifier-wrong-verifier-wrong-verifier-000").await;
        assert_eq!((status, v["error"].clone()), (Status::BadRequest, json!("invalid_grant")));
        let (status, v) = exchange(&c, &client_id, &code, VERIFIER).await;
        assert_eq!((status, v["error"].clone()), (Status::BadRequest, json!("invalid_grant")), "틀린 시도로도 코드는 소모된다");
        let code = approve(&c, &client_id, 1).await;
        let (status, tokens) = exchange(&c, &client_id, &code, VERIFIER).await;
        assert_eq!(status, Status::Ok, "{tokens}");
        assert_eq!((tokens["token_type"].clone(), tokens["scope"].clone()), (json!("Bearer"), json!(super::SCOPE)));
        let (status, _) = exchange(&c, &client_id, &code, VERIFIER).await;
        assert_eq!(status, Status::BadRequest, "같은 코드는 다시 못 쓴다");
        let access = tokens["access_token"].as_str().unwrap().to_string();
        let refresh = tokens["refresh_token"].as_str().unwrap().to_string();

        // 승인한 계정의 데이터만 읽는다. MCP 말고 GraphQL에는 못 쓴다
        assert_eq!(ledger_names(&c, &access).await, ["하나 장부"]);
        let res = c
            .post("/api/graphql")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", format!("Bearer {access}")))
            .body(json!({ "query": "{ ledgers { name } }" }).to_string())
            .dispatch()
            .await;
        assert_eq!(res.status(), Status::Unauthorized);

        // 리프레시하면 두 토큰 모두 바뀌고 예전 것은 못 쓴다
        let body = [("grant_type", "refresh_token"), ("refresh_token", refresh.as_str()), ("client_id", client_id.as_str())];
        let (status, renewed) = token(&c, &body, None).await;
        assert_eq!(status, Status::Ok, "{renewed}");
        let (status, challenge, _) = mcp(&c, Some(&access), "ping", json!({})).await;
        assert_eq!(status, Status::Unauthorized);
        assert!(challenge.unwrap().contains("error=\"invalid_token\""));
        let (status, v) = token(&c, &body, None).await;
        assert_eq!((status, v["error"].clone()), (Status::BadRequest, json!("invalid_grant")));
        let access = renewed["access_token"].as_str().unwrap().to_string();
        assert_eq!(ledger_names(&c, &access).await, ["하나 장부"]);

        // 다른 계정이 승인하면 그 계정 데이터
        let code = approve(&c, &client_id, 2).await;
        let (_, other) = exchange(&c, &client_id, &code, VERIFIER).await;
        assert_eq!(ledger_names(&c, other["access_token"].as_str().unwrap()).await, ["둘 장부"]);

        // 설정 화면에서 연결을 끊으면 바로 못 쓴다 (다른 계정 연결은 그대로)
        let gql = |query: &'static str| {
            c.post("/api/graphql").header(ContentType::JSON).private_cookie(session(1)).body(json!({ "query": query }).to_string()).dispatch()
        };
        let apps = json_of(gql("{ connectedApps { id clientName scope lastUsedAt } }").await).await;
        let apps = apps["data"]["connectedApps"].as_array().unwrap().clone();
        assert_eq!(apps.len(), 1, "{apps:?}");
        assert_eq!((apps[0]["clientName"].clone(), apps[0]["scope"].clone()), (json!("<b>ChatGPT</b>"), json!(super::SCOPE)));
        assert!(apps[0]["lastUsedAt"].is_string());
        let id = apps[0]["id"].as_i64().unwrap();
        let res = c
            .post("/api/graphql")
            .header(ContentType::JSON)
            .private_cookie(session(1))
            .body(json!({ "query": format!("mutation {{ revokeConnectedApp(id: {id}) }}") }).to_string())
            .dispatch()
            .await;
        assert_eq!(json_of(res).await["data"]["revokeConnectedApp"], true);
        assert_eq!(mcp(&c, Some(&access), "ping", json!({})).await.0, Status::Unauthorized);
        assert_eq!(ledger_names(&c, other["access_token"].as_str().unwrap()).await, ["둘 장부"]);
    }

    #[tokio::test]
    async fn confidential_clients_must_send_their_secret() {
        let c = client().await;
        // token_endpoint_auth_method를 생략하면 client_secret_basic (RFC 7591)
        let (status, v) = register(&c, json!({ "client_name": "비밀 클라이언트", "redirect_uris": [CHATGPT] })).await;
        assert_eq!(status, Status::Created, "{v}");
        assert_eq!(v["token_endpoint_auth_method"], "client_secret_basic");
        let client_id = v["client_id"].as_str().unwrap().to_string();
        let secret = v["client_secret"].as_str().unwrap().to_string();

        let code = approve(&c, &client_id, 1).await;
        let body = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", CHATGPT),
            ("client_id", client_id.as_str()),
            ("code_verifier", VERIFIER),
        ];
        let (status, v) = token(&c, &body, None).await;
        assert_eq!((status, v["error"].clone()), (Status::Unauthorized, json!("invalid_client")));

        let code = approve(&c, &client_id, 1).await;
        let body = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", CHATGPT), ("code_verifier", VERIFIER)];
        use base64::Engine;
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:{secret}"));
        let (status, v) = token(&c, &body, Some(&basic)).await;
        assert_eq!(status, Status::Ok, "{v}");

        // client_secret_post도 된다
        let code = approve(&c, &client_id, 1).await;
        let body = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("code_verifier", VERIFIER),
        ];
        assert_eq!(token(&c, &body, None).await.0, Status::Ok);
    }
}
