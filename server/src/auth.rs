//! 인증: 비밀번호 로그인(암호화 쿠키 세션)과 API 토큰(Bearer). 토큰은 자동 입력과 GraphQL을 모두 쓴다.
//!
//! `CASHBOOK_PASSWORD`가 없으면 인증을 끈다 (로컬 전용 실행). 이때는 루프백 주소로만 띄우도록 main에서 막는다.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

use chrono::Utc;
use rand::{Rng, distr::Alphanumeric};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::config::AppConfig;
use crate::entity::{api_token, user};

pub const SESSION_COOKIE: &str = "cashbook_session";
pub const SESSION_DAYS: i64 = 30;

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// `cb_` + 영숫자 40자
pub fn generate_token() -> String {
    let body: String = rand::rng().sample_iter(&Alphanumeric).take(40).map(char::from).collect();
    format!("cb_{body}")
}

pub async fn verify_token<C: ConnectionTrait>(db: &C, token: &str) -> Result<Option<api_token::Model>, DbErr> {
    let Some(found) = api_token::Entity::find()
        .filter(api_token::Column::TokenHash.eq(sha256_hex(token)))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let now = Utc::now();
    if found.last_used_at.is_none_or(|at| now - at > chrono::Duration::minutes(1)) {
        let mut active: api_token::ActiveModel = found.clone().into();
        active.last_used_at = Set(Some(now));
        active.update(db).await?;
    }
    Ok(Some(found))
}

/// 비밀번호를 바꾸면 기존 세션이 무효가 되도록 세션 값에 비밀번호 지문을 넣는다.
fn password_fingerprint(password: &str) -> String {
    sha256_hex(&format!("cashbook-session:{password}"))[..16].to_string()
}

pub fn session_value(password: &str) -> String {
    format!("v1:{}:{}", Utc::now().timestamp(), password_fingerprint(password))
}

pub fn session_valid(value: &str, password: &str) -> bool {
    let mut parts = value.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("v1"), Some(ts), Some(fp), None) => {
            let fresh = ts.parse::<i64>().is_ok_and(|ts| Utc::now().timestamp() - ts < SESSION_DAYS * 86_400);
            fresh && fp == password_fingerprint(password)
        }
        _ => false,
    }
}

/// 길이와 내용이 달라도 같은 시간이 걸리도록 해시끼리 비교한다.
pub fn check_password(input: &str, expected: &str) -> bool {
    let (a, b) = (Sha256::digest(input.as_bytes()), Sha256::digest(expected.as_bytes()));
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 로그인 실패 제한: IP마다 10분에 10번, 전체로는 10분에 100번.
/// 한 곳에서 비밀번호를 계속 틀려도 다른 곳(본인)의 로그인은 막히지 않는다.
#[derive(Default)]
pub struct LoginLimiter {
    per_ip: Mutex<HashMap<Option<IpAddr>, VecDeque<Instant>>>,
    global: Mutex<VecDeque<Instant>>,
}

impl LoginLimiter {
    const WINDOW: StdDuration = StdDuration::from_secs(600);
    const MAX_PER_IP: usize = 10;
    const MAX_GLOBAL: usize = 100;

    fn prune(q: &mut VecDeque<Instant>) {
        while q.front().is_some_and(|t| t.elapsed() > Self::WINDOW) {
            q.pop_front();
        }
    }

    pub fn allowed(&self, ip: Option<IpAddr>) -> bool {
        let mut global = self.global.lock().unwrap();
        Self::prune(&mut global);
        let mut per_ip = self.per_ip.lock().unwrap();
        per_ip.retain(|_, q| {
            Self::prune(q);
            !q.is_empty()
        });
        global.len() < Self::MAX_GLOBAL && per_ip.get(&ip).is_none_or(|q| q.len() < Self::MAX_PER_IP)
    }

    pub fn record_failure(&self, ip: Option<IpAddr>) {
        self.global.lock().unwrap().push_back(Instant::now());
        self.per_ip.lock().unwrap().entry(ip).or_default().push_back(Instant::now());
    }
}

/// 인증된 요청의 계정 (로그인 세션 또는 API 토큰). 인증이 꺼져 있으면 기존 데이터 계정(id 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auth {
    pub user_id: i32,
    pub is_admin: bool,
}

impl Auth {
    pub fn current_user(&self) -> crate::graphql::CurrentUser {
        crate::graphql::CurrentUser { id: self.user_id, is_admin: self.is_admin }
    }
}

/// 403 응답에 보여줄 이유 (요청 로컬 캐시에 남긴다)
pub struct ForbiddenReason(pub &'static str);

fn forbid<T>(req: &Request<'_>, reason: &'static str) -> Outcome<T, &'static str> {
    req.local_cache(|| ForbiddenReason(reason));
    Outcome::Error((Status::Forbidden, reason))
}

fn bearer_token<'a>(req: &'a Request<'_>) -> Option<&'a str> {
    req.headers()
        .get_one("Authorization")
        .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
        .or_else(|| req.headers().get_one("X-Api-Key"))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// "https://cashbook.example.com:8443" → "cashbook.example.com:8443"
fn origin_authority(origin: &str) -> Option<String> {
    let rest = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?.to_lowercase();
    (!authority.is_empty()).then_some(authority)
}

fn host_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.rsplit_once(':').map_or(host, |(h, _)| h)
}

/// 브라우저가 다른 사이트에서 보낸 요청(CSRF)을 막는다. 자동화 앱처럼 Origin을 안 보내면 통과.
pub fn origin_allowed(config: &AppConfig, origin: Option<&str>, host: Option<&str>, forwarded_host: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    let origin_lc = origin.trim().trim_end_matches('/').to_lowercase();
    if config.allowed_origins.contains(&origin_lc) {
        return true;
    }
    let Some(authority) = origin_authority(&origin_lc) else { return false };
    [host, forwarded_host]
        .into_iter()
        .flatten()
        .any(|h| h.trim().eq_ignore_ascii_case(&authority))
}

/// 비밀번호 없이 띄웠을 때는 localhost 이름으로 온 요청만 받는다 (DNS 리바인딩 방지).
pub fn host_allowed_without_auth(config: &AppConfig, host: Option<&str>) -> bool {
    if config.allow_insecure {
        return true;
    }
    let Some(host) = host else { return false };
    let lc = host.trim().to_lowercase();
    let name = host_name(&lc);
    matches!(name, "localhost" | "127.0.0.1" | "::1") || name.ends_with(".localhost") || config.allowed_hosts.iter().any(|h| *h == lc || h == name)
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Auth {
    type Error = &'static str;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(state) = req.rocket().state::<AppState>() else {
            return Outcome::Error((Status::InternalServerError, "app state missing"));
        };
        let headers = req.headers();
        let host = headers.get_one("Host");
        if !origin_allowed(&state.config, headers.get_one("Origin"), host, headers.get_one("X-Forwarded-Host")) {
            return forbid(req, "다른 사이트에서 보낸 요청은 받지 않습니다 (CASHBOOK_ALLOWED_ORIGINS 참고)");
        }
        let Some(password) = state.config.password.as_deref() else {
            if !host_allowed_without_auth(&state.config, host) {
                return forbid(req, "비밀번호 없이 실행 중이라 localhost 주소로만 접속할 수 있습니다");
            }
            return Outcome::Success(Auth { user_id: user::LEGACY_USER_ID, is_admin: true });
        };
        if let Some(token) = bearer_token(req) {
            return match verify_token(&state.db, token).await {
                Ok(Some(t)) => Outcome::Success(Auth { user_id: t.user_id, is_admin: t.user_id == user::LEGACY_USER_ID }),
                Ok(None) => Outcome::Error((Status::Unauthorized, "invalid api token")),
                Err(_) => Outcome::Error((Status::InternalServerError, "database error")),
            };
        }
        if let Some(cookie) = req.cookies().get_private(SESSION_COOKIE)
            && session_valid(cookie.value(), password)
        {
            return Outcome::Success(Auth { user_id: user::LEGACY_USER_ID, is_admin: true });
        }
        Outcome::Error((Status::Unauthorized, "login required"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_passwords() {
        let v = session_value("pw");
        assert!(session_valid(&v, "pw"));
        assert!(!session_valid(&v, "other"));
        assert!(!session_valid("v1:0:abc", "pw"));
        assert!(!session_valid("garbage", "pw"));
        assert!(check_password("secret", "secret"));
        assert!(!check_password("secret", "secret2"));
        let t = generate_token();
        assert!(t.starts_with("cb_") && t.len() == 43);
    }

    #[test]
    fn limiter_is_per_ip() {
        let l = LoginLimiter::default();
        let attacker: Option<IpAddr> = "203.0.113.9".parse().ok();
        let me: Option<IpAddr> = "198.51.100.7".parse().ok();
        for _ in 0..10 {
            assert!(l.allowed(attacker));
            l.record_failure(attacker);
        }
        assert!(!l.allowed(attacker));
        assert!(l.allowed(me));
    }

    #[test]
    fn origin_and_host_checks() {
        let mut cfg = AppConfig::for_tests();
        // Origin이 없으면 (curl, 단축어 등) 통과
        assert!(origin_allowed(&cfg, None, Some("127.0.0.1:8000"), None));
        assert!(origin_allowed(&cfg, Some("http://127.0.0.1:8000"), Some("127.0.0.1:8000"), None));
        assert!(origin_allowed(&cfg, Some("http://LOCALHOST:5173"), Some("localhost:5173"), None));
        assert!(!origin_allowed(&cfg, Some("https://evil.example"), Some("127.0.0.1:8000"), None));
        assert!(!origin_allowed(&cfg, Some("null"), Some("127.0.0.1:8000"), None));
        // 프록시가 Host를 바꿔도 X-Forwarded-Host나 허용 목록으로 통과
        assert!(origin_allowed(&cfg, Some("https://cash.example.com"), Some("127.0.0.1:8000"), Some("cash.example.com")));
        cfg.allowed_origins = vec!["https://cash.example.com".into()];
        assert!(origin_allowed(&cfg, Some("https://cash.example.com/"), Some("127.0.0.1:8000"), None));

        assert!(host_allowed_without_auth(&cfg, Some("127.0.0.1:8000")));
        assert!(host_allowed_without_auth(&cfg, Some("localhost:5173")));
        assert!(host_allowed_without_auth(&cfg, Some("[::1]:8000")));
        assert!(!host_allowed_without_auth(&cfg, Some("attacker.example:8000")));
        assert!(!host_allowed_without_auth(&cfg, None));
        cfg.allowed_hosts = vec!["nas.local".into()];
        assert!(host_allowed_without_auth(&cfg, Some("nas.local:8000")));
    }
}
