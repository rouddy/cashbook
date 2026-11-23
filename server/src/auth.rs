//! 인증: Google 로그인 세션(암호화 쿠키, `oauth`)과 API 토큰(Bearer). 둘 다 계정 하나를 가리키고,
//! 토큰은 자동 입력과 GraphQL을 모두 쓴다.
//!
//! Google 로그인을 설정하지 않으면 인증을 끄고 기존 데이터 계정(id 1)으로 동작한다 (로컬 전용 실행).
//! 이때는 루프백 주소로만 띄우도록 main에서 막는다.

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

/// 세션 쿠키 값: 계정 id와 로그인 시각. 쿠키는 Rocket 비밀 키로 암호화·서명돼 바꿀 수 없다.
pub fn session_value(user_id: i32) -> String {
    format!("v2:{user_id}:{}", Utc::now().timestamp())
}

/// 세션 쿠키 값에서 계정 id (형식이 다르거나 `SESSION_DAYS`가 지났으면 None)
pub fn session_user(value: &str) -> Option<i32> {
    let mut parts = value.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("v2"), Some(id), Some(ts), None) => {
            let fresh = ts.parse::<i64>().is_ok_and(|ts| Utc::now().timestamp() - ts < SESSION_DAYS * 86_400);
            id.parse().ok().filter(|_| fresh)
        }
        _ => None,
    }
}

/// 인증된 요청의 계정 (로그인 세션 또는 API 토큰). 인증이 꺼져 있으면 기존 데이터 계정(id 1, 관리자).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auth {
    pub user_id: i32,
    pub is_admin: bool,
}

impl Auth {
    pub fn of(config: &AppConfig, user: &user::Model) -> Self {
        Self { user_id: user.id, is_admin: config.is_admin_email(user.email.as_deref()) }
    }

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

pub(crate) fn bearer_token<'a>(req: &'a Request<'_>) -> Option<&'a str> {
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
        if !state.config.auth_enabled() {
            if !host_allowed_without_auth(&state.config, host) {
                return forbid(req, "인증 없이 실행 중이라 localhost 주소로만 접속할 수 있습니다");
            }
            return Outcome::Success(Auth { user_id: user::LEGACY_USER_ID, is_admin: true });
        }
        let user_id = if let Some(token) = bearer_token(req) {
            match verify_token(&state.db, token).await {
                Ok(Some(t)) => Some(t.user_id),
                Ok(None) => return Outcome::Error((Status::Unauthorized, "invalid api token")),
                Err(_) => return Outcome::Error((Status::InternalServerError, "database error")),
            }
        } else {
            req.cookies().get_private(SESSION_COOKIE).and_then(|c| session_user(c.value()))
        };
        if let Some(id) = user_id {
            match user::Entity::find_by_id(id).one(&state.db).await {
                Ok(Some(u)) => return Outcome::Success(Auth::of(&state.config, &u)),
                Ok(None) => {}
                Err(_) => return Outcome::Error((Status::InternalServerError, "database error")),
            }
        }
        Outcome::Error((Status::Unauthorized, "login required"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_tokens() {
        assert_eq!(session_user(&session_value(7)), Some(7));
        assert_eq!(session_user("v2:7:0"), None, "오래된 세션");
        assert_eq!(session_user("v1:0:abc"), None, "비밀번호 시절 세션");
        assert_eq!(session_user("garbage"), None);
        let t = generate_token();
        assert!(t.starts_with("cb_") && t.len() == 43);
    }

    #[test]
    fn admins_are_owner_and_listed_emails() {
        let mut cfg = AppConfig::for_tests();
        cfg.owner_email = Some("owner@example.com".into());
        cfg.admin_emails = vec!["admin@example.com".into()];
        assert!(cfg.is_admin_email(Some("Owner@Example.com")));
        assert!(cfg.is_admin_email(Some("admin@example.com")));
        assert!(!cfg.is_admin_email(Some("someone@example.com")));
        assert!(!cfg.is_admin_email(None));
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
