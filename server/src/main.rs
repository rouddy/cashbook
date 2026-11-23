//! 가계부 서버
//!
//! ```text
//! cashbook [serve]                        서버 실행 (기본)
//! cashbook import <file.json> [--replace] [--user <id|이메일>]  스프레드시트 변환 JSON 가져오기 (기본: 기존 데이터 계정)
//! cashbook generate-planned               반복 규칙의 예정 거래 채우기
//! cashbook copy-db <원본 URL> <대상 URL>    DB 데이터 복사 (SQLite ↔ MySQL, 대상은 빈 DB)
//! ```

mod auth;
mod config;
mod db;
mod domain;
mod entity;
mod graphql;
mod oauth;
mod routes;

use std::error::Error;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use rand::RngCore;
use sea_orm::DatabaseConnection;

use crate::config::AppConfig;
use crate::domain::recurring;

/// Rocket이 관리하는 앱 상태
pub struct AppState {
    pub config: AppConfig,
    pub db: DatabaseConnection,
    /// Google 로그인 토큰 교환용
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(config: AppConfig, db: DatabaseConnection) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("cashbook/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Self { config, db, http }
    }
}

/// 라우트·스키마·상태를 붙인 Rocket (서버 실행과 테스트가 같이 쓴다)
pub fn build_rocket(figment: rocket::figment::Figment, state: AppState) -> rocket::Rocket<rocket::Build> {
    let schema = graphql::build_schema(state.db.clone(), state.config.clone());
    rocket::custom(figment)
        .manage(schema)
        .manage(state)
        .mount("/api", routes::api())
        .mount("/", routes::web())
        .register("/api", routes::api_catchers())
}

const USAGE: &str = "사용법:
  cashbook [serve]                        서버 실행
  cashbook import <file.json> [--replace] [--user <id|이메일>]
                                          스프레드시트 변환 JSON 가져오기 (기본 계정: id 1)
  cashbook generate-planned               반복 규칙의 예정 거래 채우기
  cashbook copy-db <원본 URL> <대상 URL>    DB 데이터 복사 (SQLite ↔ MySQL, 대상은 빈 DB)
                                          예: copy-db sqlite://data/cashbook.db mysql://cash:pw@db:3306/cashbook";

#[rocket::main]
async fn main() {
    dotenvy::dotenv().ok();
    if let Err(e) = run().await {
        eprintln!("오류: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let config = AppConfig::from_env()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(config).await,
        Some("import") => import(config, &args[1..]).await,
        Some("generate-planned") => {
            let db = db::connect(&config.database_url).await?;
            let n = recurring::ensure_planned(&db, config.today(), config.plan_months_ahead, None).await?;
            println!("예정 거래 {n}건을 만들었습니다");
            Ok(())
        }
        Some("copy-db") => {
            let (Some(from), Some(to)) = (args.get(1), args.get(2)) else {
                return Err(format!("원본과 대상 DB URL을 지정하세요\n{USAGE}").into());
            };
            println!("복사: {} → {}", db::copy::redact(from), db::copy::redact(to));
            for (table, n) in db::copy::run(from, to).await? {
                println!("  {table}: {n}");
            }
            println!("완료");
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("알 수 없는 명령 '{other}'\n{USAGE}").into()),
    }
}

/// `--user <id|이메일>`로 고른 계정 (없으면 기존 데이터 계정)
async fn import_user(db: &sea_orm::DatabaseConnection, value: Option<&str>) -> Result<i32, Box<dyn Error>> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let Some(v) = value else { return Ok(entity::user::LEGACY_USER_ID) };
    let found = match v.parse::<i32>() {
        Ok(id) => entity::user::Entity::find_by_id(id).one(db).await?,
        Err(_) => entity::user::Entity::find().filter(entity::user::Column::Email.eq(v.trim().to_lowercase())).one(db).await?,
    };
    Ok(found.ok_or_else(|| format!("계정 '{v}'를 찾을 수 없습니다"))?.id)
}

async fn import(config: AppConfig, args: &[String]) -> Result<(), Box<dyn Error>> {
    let replace = args.iter().any(|a| a == "--replace");
    let user = args.iter().position(|a| a == "--user").map(|i| args.get(i + 1).map(String::as_str));
    if user == Some(None) {
        return Err(format!("--user 뒤에 계정 id나 이메일을 적으세요\n{USAGE}").into());
    }
    let user_value = user.flatten();
    let path = args
        .iter()
        .find(|a| !a.starts_with("--") && Some(a.as_str()) != user_value)
        .ok_or_else(|| format!("가져올 JSON 파일을 지정하세요\n{USAGE}"))?;
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let data: domain::import::ImportData = serde_json::from_str(&raw).map_err(|e| format!("{path}: JSON 형식 오류: {e}"))?;
    let db = db::connect(&config.database_url).await?;
    let uid = import_user(&db, user_value).await?;
    let stats = domain::import::run(&db, uid, data, replace, config.today()).await?;
    let planned = recurring::ensure_planned(&db, config.today(), config.plan_months_ahead, None).await?;
    println!(
        "가져오기 완료: 장부 {}, 카테고리 {}, 결제수단 {}, 계좌 {}, 계획항목 {}, 반복규칙 {}, 거래 {} (혜택 표시 {}), 새 예정 거래 {}",
        stats.ledgers,
        stats.categories,
        stats.payment_methods,
        stats.accounts,
        stats.plan_items,
        stats.recurring_rules,
        stats.transactions,
        stats.transaction_benefits,
        planned
    );
    Ok(())
}

/// `ROCKET_SECRET_KEY`가 없으면 파일에 저장해 둔 키를 쓴다 (없으면 만든다). 재시작해도 로그인이 유지된다.
fn load_or_create_secret(path: &Path) -> Result<String, Box<dyn Error>> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let key = existing.trim().to_string();
        if key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(key);
        }
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let key = hex::encode(bytes);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

async fn serve(config: AppConfig) -> Result<(), Box<dyn Error>> {
    let mut figment = rocket::Config::figment();
    // 기본 설정에는 0으로 채운 키가 들어 있다(릴리스에서는 시작 거부, 디버그에서는 실행마다 임시 키).
    // ROCKET_SECRET_KEY/Rocket.toml로 키를 주지 않았으면 파일에 저장해 둔 키를 쓴다.
    let configured: rocket::Config = figment.extract()?;
    if configured.secret_key.is_zero() {
        figment = figment.merge(("secret_key", load_or_create_secret(&config.secret_key_file)?));
    }
    let address: IpAddr = figment.extract_inner("address")?;
    if config.legacy_password_set {
        eprintln!("⚠️  CASHBOOK_PASSWORD는 더 쓰지 않습니다. 로그인은 Google 계정으로 합니다 (CASHBOOK_GOOGLE_CLIENT_ID 등)");
    }
    match &config.google {
        Some(g) => {
            println!("Google 로그인 리디렉션 URI: {}", g.redirect_uri);
            if config.owner_email.is_none() {
                eprintln!("⚠️  CASHBOOK_OWNER_EMAIL이 없어 기존 데이터(계정 id 1)를 넘겨받을 계정이 없습니다");
            }
        }
        None => {
            if !address.is_loopback() && !config.allow_insecure {
                return Err(format!(
                    "Google 로그인 설정(CASHBOOK_GOOGLE_CLIENT_ID·CASHBOOK_GOOGLE_CLIENT_SECRET·CASHBOOK_PUBLIC_URL) 없이 \
                     {address} 로 열 수 없습니다. 로그인을 설정하거나 로컬(127.0.0.1)로 실행하세요. \
                     (정말 필요하면 CASHBOOK_ALLOW_INSECURE=1)"
                )
                .into());
            }
            eprintln!("⚠️  Google 로그인을 설정하지 않아 인증 없이 기본 계정(id 1)으로 실행합니다 ({address})");
        }
    }

    let db = db::connect(&config.database_url).await?;
    let created = recurring::ensure_planned(&db, config.today(), config.plan_months_ahead, None).await?;
    if created > 0 {
        println!("반복 규칙으로 예정 거래 {created}건을 만들었습니다");
    }

    // 날짜가 바뀌면 다음 달 예정 거래가 생기도록 주기적으로 채운다.
    {
        let db = db.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = recurring::ensure_planned(&db, config.today(), config.plan_months_ahead, None).await {
                    eprintln!("예정 거래 생성 실패: {e}");
                }
            }
        });
    }

    let _ = build_rocket(figment, AppState::new(config, db)).launch().await?;
    Ok(())
}
