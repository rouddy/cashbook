//! Rocket 라우트
//! - `/api/graphql` (POST, JSON) — 인증 필요
//! - `/api/graphiql` — GraphQL 탐색기
//! - `/api/session`, `/api/logout`
//! - `/api/auth/google` → Google 로그인 → `/api/auth/google/callback` (세션 쿠키를 만들고 화면으로 돌아간다)
//! - `/api/ingest` (POST, JSON 또는 text/plain) — 자동 입력, 인증 필요(API 토큰 또는 로그인 세션)
//! - `/api/ingest/options` (GET) — 자동 입력 규칙에서 고를 장부·카테고리·결제수단 이름 (같은 인증)
//! - `/api/health`
//! - 그 외 경로는 React 빌드(`CASHBOOK_WEB_DIR`)를 서빙하고, 없는 경로는 index.html (SPA 라우팅)

use std::path::{Path, PathBuf};

use async_graphql::http::GraphiQLSource;
use async_graphql_rocket::{GraphQLRequest, GraphQLResponse};
use rocket::fs::NamedFile;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::response::Redirect;
use rocket::response::content::RawHtml;
use rocket::serde::json::Json;
use rocket::{Catcher, Request, Route, State, catch, catchers, get, post, routes};
use sea_orm::{EntityTrait, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::auth::{Auth, ForbiddenReason, SESSION_COOKIE, SESSION_DAYS, session_user, session_value};
use crate::domain::ingest::{self, IngestError, IngestInput, IngestOutcome};
use crate::domain::users::{self, LoginError};
use crate::entity::user;
use crate::graphql::{CashbookSchema, Lookup};
use crate::oauth::{self, PendingLogin};

type ApiError = (Status, Json<Value>);

fn api_error(status: Status, message: impl Into<String>) -> ApiError {
    (status, Json(json!({ "ok": false, "error": message.into() })))
}

#[post("/graphql", data = "<request>", format = "json")]
async fn graphql(schema: &State<CashbookSchema>, state: &State<AppState>, auth: Auth, request: GraphQLRequest) -> GraphQLResponse {
    request.data(Lookup::new(state.db.clone(), auth.user_id)).data(auth.current_user()).execute(schema.inner()).await
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
struct SessionUser {
    id: i32,
    email: Option<String>,
    name: Option<String>,
    picture: Option<String>,
    is_admin: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    /// 로그인이 필요한지 (Google 로그인을 설정했는지)
    auth_required: bool,
    authenticated: bool,
    /// 로그인한 계정 (인증 없는 로컬 실행이면 기존 데이터 계정)
    user: Option<SessionUser>,
}

async fn session_info(state: &AppState, cookies: &CookieJar<'_>) -> SessionInfo {
    let user_id = match state.config.auth_enabled() {
        false => Some(user::LEGACY_USER_ID),
        true => cookies.get_private(SESSION_COOKIE).and_then(|c| session_user(c.value())),
    };
    let found = match user_id {
        Some(id) => user::Entity::find_by_id(id).one(&state.db).await.ok().flatten(),
        None => None,
    };
    let user = found.map(|u| SessionUser {
        id: u.id,
        is_admin: !state.config.auth_enabled() || state.config.is_admin_email(u.email.as_deref()),
        email: u.email,
        name: u.name,
        picture: u.picture,
    });
    SessionInfo { auth_required: state.config.auth_enabled(), authenticated: user.is_some(), user }
}

#[get("/session")]
async fn session(state: &State<AppState>, cookies: &CookieJar<'_>) -> Json<SessionInfo> {
    Json(session_info(state, cookies).await)
}

#[post("/logout")]
async fn logout(state: &State<AppState>, cookies: &CookieJar<'_>) -> Json<SessionInfo> {
    if !state.config.auth_enabled() {
        return Json(session_info(state, cookies).await);
    }
    // 같은 요청 안에서는 지운 쿠키가 아직 읽히므로 로그아웃 상태를 바로 돌려준다
    cookies.remove_private(Cookie::build(SESSION_COOKIE).path("/"));
    Json(SessionInfo { auth_required: true, authenticated: false, user: None })
}

/// Google 로그인 시작: 로그인 중 상태를 쿠키에 넣고 Google 로그인 화면으로 보낸다. `next`는 로그인 뒤 돌아갈 경로.
#[get("/auth/google?<next>")]
fn google_start(state: &State<AppState>, cookies: &CookieJar<'_>, next: Option<&str>) -> Redirect {
    let Some(google) = state.config.google.as_ref() else {
        return Redirect::to("/");
    };
    let pending = PendingLogin::new(next);
    let value = serde_json::to_string(&pending).expect("serialize pending login");
    cookies.add_private(
        Cookie::build((oauth::STATE_COOKIE, value))
            .path("/api/auth")
            .http_only(true)
            .secure(state.config.secure_cookie)
            .same_site(rocket::http::SameSite::Lax)
            .max_age(rocket::time::Duration::minutes(oauth::STATE_MINUTES)),
    );
    Redirect::to(oauth::authorize_url(google, &pending))
}

/// Google에서 돌아온 요청: state를 확인하고 계정에 로그인한 뒤 화면으로 돌아간다. 실패하면 로그인 화면에 이유를 보여 준다.
#[get("/auth/google/callback?<code>&<state>&<error>")]
async fn google_callback(
    app: &State<AppState>,
    cookies: &CookieJar<'_>,
    code: Option<&str>,
    state: Option<&str>,
    error: Option<&str>,
) -> Redirect {
    let fail = |msg: &str| Redirect::to(oauth::login_error_path(msg));
    let Some(google) = app.config.google.as_ref() else {
        return Redirect::to("/");
    };
    let pending = cookies
        .get_private(oauth::STATE_COOKIE)
        .and_then(|c| serde_json::from_str::<PendingLogin>(c.value()).ok());
    cookies.remove_private(Cookie::build(oauth::STATE_COOKIE).path("/api/auth"));
    if error.is_some() {
        return fail("Google 로그인을 취소했습니다");
    }
    let Some(pending) = pending.filter(|p| !p.expired() && Some(p.state.as_str()) == state) else {
        return fail("로그인 요청이 만료됐거나 맞지 않습니다. 다시 시도하세요");
    };
    let Some(code) = code.filter(|c| !c.is_empty()) else {
        return fail("Google 로그인 응답이 올바르지 않습니다");
    };
    let profile = match oauth::fetch_profile(&app.http, google, code, &pending.verifier).await {
        Ok(p) => p,
        Err(msg) => return fail(&msg),
    };
    let account = match users::login_google(&app.db, &app.config, &profile).await {
        Ok(u) => u,
        Err(LoginError::Rejected(msg)) => return fail(msg),
        Err(LoginError::Db(e)) => {
            eprintln!("로그인 계정 처리 실패: {e}");
            return fail("로그인 중 오류가 났습니다");
        }
    };
    cookies.add_private(
        Cookie::build((SESSION_COOKIE, session_value(account.id)))
            .path("/")
            .http_only(true)
            .secure(app.config.secure_cookie)
            .same_site(rocket::http::SameSite::Strict)
            .max_age(rocket::time::Duration::days(SESSION_DAYS)),
    );
    Redirect::to(pending.next)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IngestBody {
    Many(Vec<IngestInput>),
    One(Box<IngestInput>),
}

async fn ingest_one(state: &AppState, auth: Auth, input: IngestInput) -> Result<IngestOutcome, IngestError> {
    let txn = state.db.begin().await?;
    let out = ingest::ingest(&txn, auth.user_id, input, state.config.today()).await?;
    txn.commit().await?;
    Ok(out)
}

/// 자동 입력 응답. 단축어/자동화 앱에서 쓰기 쉽게 이름까지 풀어서 돌려준다.
async fn outcome_json(state: &AppState, auth: Auth, out: &IngestOutcome) -> Value {
    let lookup = Lookup::new(state.db.clone(), auth.user_id);
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
async fn ingest_json(state: &State<AppState>, auth: Auth, body: Json<IngestBody>) -> Result<Json<Value>, ApiError> {
    match body.into_inner() {
        IngestBody::One(input) => match ingest_one(state, auth, *input).await {
            Ok(out) => Ok(Json(outcome_json(state, auth, &out).await)),
            Err(IngestError::Invalid(msg)) => Err(api_error(Status::UnprocessableEntity, msg)),
            Err(IngestError::Db(e)) => Err(api_error(Status::InternalServerError, e.to_string())),
        },
        IngestBody::Many(inputs) => {
            if inputs.len() > 200 {
                return Err(api_error(Status::PayloadTooLarge, "한 번에 200건까지 보낼 수 있습니다"));
            }
            let mut results = Vec::with_capacity(inputs.len());
            for input in inputs {
                results.push(match ingest_one(state, auth, input).await {
                    Ok(out) => outcome_json(state, auth, &out).await,
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                });
            }
            Ok(Json(json!({ "ok": true, "results": results })))
        }
    }
}

/// 카드 승인 문자 원문을 그대로 보낼 때 (Content-Type: text/plain)
#[post("/ingest", data = "<text>", format = "text/plain", rank = 2)]
async fn ingest_text(state: &State<AppState>, auth: Auth, text: String) -> Result<Json<Value>, ApiError> {
    let input = IngestInput { text: Some(text), ..Default::default() };
    match ingest_one(state, auth, input).await {
        Ok(out) => Ok(Json(outcome_json(state, auth, &out).await)),
        Err(IngestError::Invalid(msg)) => Err(api_error(Status::UnprocessableEntity, msg)),
        Err(IngestError::Db(e)) => Err(api_error(Status::InternalServerError, e.to_string())),
    }
}

/// 자동 입력 규칙에서 고를 장부·카테고리·결제수단 이름 (안드로이드 앱 규칙 화면의 선택 목록).
#[get("/ingest/options")]
async fn ingest_options(state: &State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    let options = ingest::options(&state.db, auth.user_id).await.map_err(|e| api_error(Status::InternalServerError, e.to_string()))?;
    let mut body = serde_json::to_value(options).map_err(|e| api_error(Status::InternalServerError, e.to_string()))?;
    body["ok"] = json!(true);
    Ok(Json(body))
}

pub fn api() -> Vec<Route> {
    routes![graphql, graphiql, health, session, logout, google_start, google_callback, ingest_json, ingest_text, ingest_options]
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rocket::http::{ContentType, Header, Status};
    use rocket::local::asynchronous::Client;
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::config::{AppConfig, GoogleConfig};
    use crate::{AppState, build_rocket};

    /// 가짜 Google 토큰·userinfo 서버. 받은 요청을 기록하고, userinfo는 `profile`을 돌려준다.
    async fn mock_google(profile: Arc<Mutex<Value>>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let (profile, log) = (profile.clone(), log.clone());
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        req.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&req).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text[..end]
                                .lines()
                                .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                                .unwrap_or(0);
                            if req.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    let text = String::from_utf8_lossy(&req).to_string();
                    log.lock().unwrap().push(text.clone());
                    let (status, body) = if text.starts_with("POST /token") && text.contains("code=good") {
                        ("200 OK", json!({"access_token": "at-1", "token_type": "Bearer", "expires_in": 3600}).to_string())
                    } else if text.starts_with("POST /token") {
                        ("400 Bad Request", json!({"error": "invalid_grant"}).to_string())
                    } else if text.starts_with("GET /userinfo") && text.to_lowercase().contains("bearer at-1") {
                        ("200 OK", profile.lock().unwrap().to_string())
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let res = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(res.as_bytes()).await;
                });
            }
        });
        (base, seen)
    }

    async fn client(base: &str, db: sea_orm::DatabaseConnection) -> Client {
        let mut config = AppConfig::for_tests();
        config.google = Some(GoogleConfig {
            client_id: "cid".into(),
            client_secret: "csecret".into(),
            redirect_uri: "http://localhost/api/auth/google/callback".into(),
            auth_url: GoogleConfig::AUTH_URL.into(),
            token_url: format!("{base}/token"),
            userinfo_url: format!("{base}/userinfo"),
        });
        config.owner_email = Some("owner@example.com".into());
        let figment = rocket::figment::Figment::from(rocket::Config::debug_default())
            .merge(("secret_key", "1f".repeat(32)))
            .merge(("log_level", "off"));
        Client::tracked(build_rocket(figment, AppState::new(config, db))).await.unwrap()
    }

    async fn json_of(res: rocket::local::asynchronous::LocalResponse<'_>) -> Value {
        serde_json::from_str(&res.into_string().await.unwrap_or_default()).unwrap_or(Value::Null)
    }

    async fn graphql(c: &Client, query: &str, token: Option<&str>) -> (Status, Value) {
        let mut req = c.post("/api/graphql").header(ContentType::JSON).body(json!({ "query": query }).to_string());
        if let Some(t) = token {
            req = req.header(Header::new("Authorization", format!("Bearer {t}")));
        }
        let res = req.dispatch().await;
        (res.status(), json_of(res).await)
    }

    /// Google 로그인 화면으로 가는 주소에서 state
    async fn start_login(c: &Client, next: &str) -> String {
        let res = c.get(format!("/api/auth/google?next={next}")).dispatch().await;
        assert_eq!(res.status(), Status::SeeOther);
        let location = res.headers().get_one("Location").unwrap().to_string();
        assert!(location.starts_with(GoogleConfig::AUTH_URL), "{location}");
        let url = reqwest::Url::parse(&location).unwrap();
        url.query_pairs().find(|(k, _)| k == "state").unwrap().1.to_string()
    }

    async fn callback(c: &Client, code: &str, state: &str) -> String {
        let res = c.get(format!("/api/auth/google/callback?code={code}&state={state}")).dispatch().await;
        assert_eq!(res.status(), Status::SeeOther);
        res.headers().get_one("Location").unwrap().to_string()
    }

    #[tokio::test]
    async fn google_login_flow_sessions_and_tokens() {
        let db = crate::db::connect_in_memory().await;
        let profile = Arc::new(Mutex::new(json!({
            "sub": "g-owner", "email": "Owner@Example.com", "email_verified": true, "name": "주인", "picture": "https://example.com/o.png"
        })));
        let (base, seen) = mock_google(profile.clone()).await;
        let c = client(&base, db.clone()).await;

        // 로그인 전: 세션 없음, API는 401
        let s = json_of(c.get("/api/session").dispatch().await).await;
        assert_eq!(s, json!({"authRequired": true, "authenticated": false, "user": null}));
        assert_eq!(graphql(&c, "{ ledgers { name } }", None).await.0, Status::Unauthorized);

        // state가 다르면 로그인 화면에 이유를 보여 준다 (로그인 중 쿠키도 버린다)
        start_login(&c, "/cards").await;
        let to = callback(&c, "good", "wrong").await;
        assert!(to.starts_with("/login?error="), "{to}");
        let state = start_login(&c, "/cards").await;
        let to = callback(&c, "bad", &state).await;
        assert!(to.starts_with("/login?error="), "토큰 교환 실패: {to}");

        // 소유자 이메일로 로그인하면 기존 데이터 계정(id 1)을 넘겨받고 원래 가려던 화면으로 돌아간다
        let state = start_login(&c, "/cards").await;
        assert_eq!(callback(&c, "good", &state).await, "/cards");
        let token_request = seen.lock().unwrap().iter().rev().find(|r| r.starts_with("POST /token")).cloned().unwrap();
        for part in ["grant_type=authorization_code", "code_verifier=", "client_secret=csecret", "code=good"] {
            assert!(token_request.contains(part), "{part}: {token_request}");
        }
        let s = json_of(c.get("/api/session").dispatch().await).await;
        assert_eq!(
            s,
            json!({"authRequired": true, "authenticated": true, "user": {
                "id": 1, "email": "owner@example.com", "name": "주인", "picture": "https://example.com/o.png", "isAdmin": true}})
        );
        let (status, body) = graphql(&c, r#"mutation { createLedger(input: {name: "주인 장부"}) { name } }"#, None).await;
        assert_eq!((status, body["data"]["createLedger"]["name"].clone()), (Status::Ok, json!("주인 장부")));

        // 다른 사람은 새 계정 (기본 장부가 있다), 관리자 아님. 토큰도 그 계정 것
        let other = client(&base, db.clone()).await;
        *profile.lock().unwrap() = json!({"sub": "g-2", "email": "two@example.com", "email_verified": true, "name": "둘"});
        let state = start_login(&other, "/").await;
        assert_eq!(callback(&other, "good", &state).await, "/");
        let s = json_of(other.get("/api/session").dispatch().await).await;
        assert_eq!(s["user"]["isAdmin"], false, "{s}");
        let (_, body) = graphql(&other, r#"{ ledgers { name } } "#, None).await;
        assert_eq!(body["data"]["ledgers"], json!([{"name": "가계부"}]));
        let (_, body) = graphql(&other, r#"mutation { createApiToken(name: "폰") { token } }"#, None).await;
        let token = body["data"]["createApiToken"]["token"].as_str().unwrap().to_string();

        // 로그아웃하면 세션이 끝난다
        let s = json_of(other.post("/api/logout").dispatch().await).await;
        assert_eq!(s["authenticated"], false);
        assert_eq!(graphql(&other, "{ ledgers { name } }", None).await.0, Status::Unauthorized);

        // 토큰은 로그인 없이 그 계정으로 동작한다
        let (status, body) = graphql(&other, "{ ledgers { name } }", Some(&token)).await;
        assert_eq!((status, body["data"]["ledgers"].clone()), (Status::Ok, json!([{"name": "가계부"}])));
        let res = other
            .post("/api/ingest")
            .header(ContentType::JSON)
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .body(json!({"amount": 5000, "description": "커피"}).to_string())
            .dispatch()
            .await;
        assert_eq!(res.status(), Status::Ok);
        assert_eq!(json_of(res).await["transaction"]["ledger"], "가계부");
        let (status, _) = graphql(&other, "{ ledgers { name } }", Some("cb_wrong")).await;
        assert_eq!(status, Status::Unauthorized);
    }

    #[tokio::test]
    async fn open_redirects_are_blocked() {
        let db = crate::db::connect_in_memory().await;
        let (base, _) = mock_google(Arc::new(Mutex::new(json!({"sub": "g", "email": "a@example.com", "email_verified": true})))).await;
        let c = client(&base, db).await;
        let state = start_login(&c, "https://evil.example").await;
        assert_eq!(callback(&c, "good", &state).await, "/");
    }
}
