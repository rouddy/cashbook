//! 환경 변수 설정. 루트의 `.env`도 읽는다 (dotenvy).

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::domain::dates::today_in;

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// `DATABASE_URL` (기본: `sqlite://data/cashbook.db?mode=rwc`)
    pub database_url: String,
    /// `CASHBOOK_PASSWORD`. 설정하면 웹 UI 로그인과 API 토큰 인증을 요구한다.
    pub password: Option<String>,
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
    /// `CASHBOOK_SECURE_COOKIE=1`이면 세션 쿠키에 Secure를 붙인다 (HTTPS 리버스 프록시 뒤에서 운영할 때).
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
        Ok(Self {
            database_url: var("DATABASE_URL").unwrap_or_else(|| "sqlite://data/cashbook.db?mode=rwc".into()),
            password: var("CASHBOOK_PASSWORD"),
            timezone,
            web_dir: var("CASHBOOK_WEB_DIR").map(PathBuf::from).unwrap_or_else(default_web_dir),
            plan_months_ahead,
            secret_key_file: var("CASHBOOK_SECRET_FILE").unwrap_or_else(|| "data/secret.key".into()).into(),
            allow_insecure: flag("CASHBOOK_ALLOW_INSECURE"),
            secure_cookie: flag("CASHBOOK_SECURE_COOKIE"),
            allowed_origins: list("CASHBOOK_ALLOWED_ORIGINS"),
            allowed_hosts: list("CASHBOOK_ALLOWED_HOSTS"),
        })
    }

    pub fn auth_enabled(&self) -> bool {
        self.password.is_some()
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
            password: None,
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
