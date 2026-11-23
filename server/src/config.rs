//! 환경 변수 설정. 루트의 `.env`도 읽는다 (dotenvy).

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::domain::dates::today_in;

/// Google 로그인(OAuth 2.0 / OpenID Connect) 설정
#[derive(Clone, Debug)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
    /// `CASHBOOK_PUBLIC_URL` + `/api/auth/google/callback`. Google Cloud 콘솔의 승인된 리디렉션 URI에 같은 값을 등록한다.
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
}

impl GoogleConfig {
    pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
    pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
    pub const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// `DATABASE_URL` (기본: `sqlite://data/cashbook.db?mode=rwc`). MySQL은 `mysql://사용자:비밀번호@호스트:3306/DB`
    pub database_url: String,
    /// `CASHBOOK_GOOGLE_CLIENT_ID`·`CASHBOOK_GOOGLE_CLIENT_SECRET`·`CASHBOOK_PUBLIC_URL`.
    /// 설정하면 Google 로그인과 API 토큰 인증을 요구하고, 없으면 인증 없이 기존 데이터 계정(id 1)으로 동작한다(로컬 전용).
    pub google: Option<GoogleConfig>,
    /// `CASHBOOK_PUBLIC_URL` (끝의 `/` 제외). Google 로그인 리디렉션과 MCP OAuth(발급자·리소스 주소)의 기준
    pub public_url: Option<String>,
    /// `CASHBOOK_OWNER_EMAIL`. 이 Google 계정이 처음 로그인하면 기존 데이터 계정(id 1)을 넘겨받는다. 관리자이기도 하다.
    pub owner_email: Option<String>,
    /// `CASHBOOK_ADMIN_EMAILS` (쉼표 구분). 모든 카드 상품을 고칠 수 있는 계정
    pub admin_emails: Vec<String>,
    /// 예전 `CASHBOOK_PASSWORD`가 남아 있는지 (더 쓰지 않는다고 알린다)
    pub legacy_password_set: bool,
    /// `CASHBOOK_TZ` (기본: Asia/Seoul). "오늘" 판단 기준.
    pub timezone: chrono_tz::Tz,
    /// `CASHBOOK_WEB_DIR` (기본: `../web/dist` 또는 `web/dist` 중 있는 곳). React 빌드 결과물.
    pub web_dir: PathBuf,
    /// `CASHBOOK_PLAN_MONTHS` (기본: 2). 반복 규칙으로 이번 달 + N개월치 예정 거래를 만든다.
    pub plan_months_ahead: u32,
    /// `CASHBOOK_SECRET_FILE` (기본: `data/secret.key`). `ROCKET_SECRET_KEY`가 없을 때 세션 쿠키 암호화 키를 저장한다.
    pub secret_key_file: PathBuf,
    /// `CASHBOOK_ALLOW_INSECURE=1`이면 비밀번호 없이 외부 주소로 띄우는 것을 허용한다.
    pub allow_insecure: bool,
    /// `CASHBOOK_SECURE_COOKIE=1`이거나 `CASHBOOK_PUBLIC_URL`이 https면 세션 쿠키에 Secure를 붙인다.
    pub secure_cookie: bool,
    /// `CASHBOOK_ALLOWED_ORIGINS` (쉼표 구분, 예: `https://cashbook.example.com`).
    /// 브라우저 요청의 Origin이 Host와 다를 때 허용할 출처. 프록시가 Host를 바꾸는 경우에 쓴다.
    pub allowed_origins: Vec<String>,
    /// `CASHBOOK_ALLOWED_HOSTS` (쉼표 구분). 비밀번호 없이 로컬로 띄울 때 localhost 외에 허용할 Host.
    pub allowed_hosts: Vec<String>,
}

fn list(name: &str) -> Vec<String> {
    var(name)
        .map(|v| v.split(',').map(|s| s.trim().trim_end_matches('/').to_lowercase()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// `CASHBOOK_WEB_DIR`이 없으면 서버를 어디서 실행했든 찾을 수 있게 흔한 위치를 차례로 본다.
fn default_web_dir() -> PathBuf {
    ["../web/dist", "web/dist"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("index.html").is_file())
        .unwrap_or_else(|| PathBuf::from("../web/dist"))
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn flag(name: &str) -> bool {
    matches!(var(name).as_deref(), Some("1" | "true" | "TRUE" | "yes" | "on"))
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let timezone = var("CASHBOOK_TZ")
            .unwrap_or_else(|| "Asia/Seoul".into())
            .parse::<chrono_tz::Tz>()
            .map_err(|e| format!("CASHBOOK_TZ: {e}"))?;
        let plan_months_ahead = match var("CASHBOOK_PLAN_MONTHS") {
            Some(v) => v.parse().map_err(|_| "CASHBOOK_PLAN_MONTHS는 0 이상의 정수여야 합니다".to_string())?,
            None => 2,
        };
        let public_url = var("CASHBOOK_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string());
        let google = match (var("CASHBOOK_GOOGLE_CLIENT_ID"), var("CASHBOOK_GOOGLE_CLIENT_SECRET")) {
            (None, None) => None,
            (Some(client_id), Some(client_secret)) => {
                let base = public_url.clone().ok_or(
                    "Google 로그인에는 CASHBOOK_PUBLIC_URL(예: https://cashbook.example.com)이 필요합니다".to_string(),
                )?;
                if !base.starts_with("https://") && !base.starts_with("http://") {
                    return Err("CASHBOOK_PUBLIC_URL은 http:// 또는 https:// 로 시작해야 합니다".into());
                }
                Some(GoogleConfig {
                    client_id,
                    client_secret,
                    redirect_uri: format!("{base}/api/auth/google/callback"),
                    // 시험용(가짜 Google 서버)으로만 바꾼다
                    auth_url: var("CASHBOOK_GOOGLE_AUTH_URL").unwrap_or_else(|| GoogleConfig::AUTH_URL.into()),
                    token_url: var("CASHBOOK_GOOGLE_TOKEN_URL").unwrap_or_else(|| GoogleConfig::TOKEN_URL.into()),
                    userinfo_url: var("CASHBOOK_GOOGLE_USERINFO_URL").unwrap_or_else(|| GoogleConfig::USERINFO_URL.into()),
                })
            }
            _ => return Err("CASHBOOK_GOOGLE_CLIENT_ID와 CASHBOOK_GOOGLE_CLIENT_SECRET을 함께 설정하세요".into()),
        };
        let secure_cookie = flag("CASHBOOK_SECURE_COOKIE") || public_url.as_deref().is_some_and(|u| u.starts_with("https://"));
        Ok(Self {
            database_url: var("DATABASE_URL").unwrap_or_else(|| "sqlite://data/cashbook.db?mode=rwc".into()),
            google,
            public_url,
            owner_email: var("CASHBOOK_OWNER_EMAIL").map(|e| e.to_lowercase()),
            admin_emails: list("CASHBOOK_ADMIN_EMAILS"),
            legacy_password_set: var("CASHBOOK_PASSWORD").is_some(),
            timezone,
            web_dir: var("CASHBOOK_WEB_DIR").map(PathBuf::from).unwrap_or_else(default_web_dir),
            plan_months_ahead,
            secret_key_file: var("CASHBOOK_SECRET_FILE").unwrap_or_else(|| "data/secret.key".into()).into(),
            allow_insecure: flag("CASHBOOK_ALLOW_INSECURE"),
            secure_cookie,
            allowed_origins: list("CASHBOOK_ALLOWED_ORIGINS"),
            allowed_hosts: list("CASHBOOK_ALLOWED_HOSTS"),
        })
    }

    pub fn auth_enabled(&self) -> bool {
        self.google.is_some()
    }

    /// 모든 카드 상품을 고칠 수 있는 계정인지 (`CASHBOOK_ADMIN_EMAILS` 또는 `CASHBOOK_OWNER_EMAIL`)
    pub fn is_admin_email(&self, email: Option<&str>) -> bool {
        let Some(email) = email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()) else { return false };
        self.owner_email.as_deref() == Some(email.as_str()) || self.admin_emails.contains(&email)
    }

    pub fn today(&self) -> NaiveDate {
        today_in(self.timezone)
    }
}

#[cfg(test)]
impl AppConfig {
    pub fn for_tests() -> Self {
        Self {
            database_url: "sqlite::memory:".into(),
            google: None,
            public_url: None,
            owner_email: None,
            admin_emails: vec![],
            legacy_password_set: false,
            timezone: chrono_tz::Asia::Seoul,
            web_dir: "../web/dist".into(),
            plan_months_ahead: 2,
            secret_key_file: "data/secret.key".into(),
            allow_insecure: false,
            secure_cookie: false,
            allowed_origins: vec![],
            allowed_hosts: vec![],
        }
    }
}
