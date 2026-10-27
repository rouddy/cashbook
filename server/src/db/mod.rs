//! DB 연결과 마이그레이션.

mod migration;

use std::path::Path;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;

pub use migration::Migrator;

/// DB에 연결하고 마이그레이션을 적용한다. SQLite 파일이면 상위 디렉터리를 만든다.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    if let Some(path) = sqlite_file_path(database_url)
        && let Some(parent) = Path::new(&path).parent()
            && !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| DbErr::Custom(format!("DB 디렉터리 생성 실패 {}: {e}", parent.display())))?;
            }
    let mut opts = ConnectOptions::new(database_url.to_owned());
    opts.sqlx_logging(false);
    let db = Database::connect(opts).await?;
    Migrator::up(&db, None).await?;
    Ok(db)
}

/// 테스트용 인메모리 DB.
#[cfg(test)]
pub async fn connect_in_memory() -> DatabaseConnection {
    let mut opts = ConnectOptions::new("sqlite::memory:".to_owned());
    // 인메모리 DB는 커넥션마다 따로 생기므로 풀을 하나로 고정한다.
    opts.max_connections(1).min_connections(1).sqlx_logging(false);
    let db = Database::connect(opts).await.expect("in-memory sqlite");
    Migrator::up(&db, None).await.expect("migrations");
    db
}

/// `sqlite://data/cashbook.db?mode=rwc` → `data/cashbook.db`
fn sqlite_file_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("sqlite://").or_else(|| url.strip_prefix("sqlite:"))?;
    let path = rest.split('?').next().unwrap_or("");
    if path.is_empty() || path.starts_with(":memory:") {
        return None;
    }
    Some(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::sqlite_file_path;

    #[test]
    fn parses_sqlite_paths() {
        assert_eq!(sqlite_file_path("sqlite://data/cashbook.db?mode=rwc").as_deref(), Some("data/cashbook.db"));
        assert_eq!(sqlite_file_path("sqlite:///abs/x.db").as_deref(), Some("/abs/x.db"));
        assert_eq!(sqlite_file_path("sqlite::memory:"), None);
        assert_eq!(sqlite_file_path("mysql://x"), None);
    }
}
