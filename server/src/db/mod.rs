//! DB 연결과 마이그레이션. `DATABASE_URL`로 SQLite(`sqlite://...`)와 MySQL(`mysql://...`)을 쓴다.

pub mod copy;
mod migration;

use std::path::Path;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;

pub use migration::Migrator;

/// 지원하는 DB 종류인지 확인한다. 오류 메시지에 URL(비밀번호 포함)을 그대로 넣지 않는다.
fn check_scheme(database_url: &str) -> Result<(), DbErr> {
    let scheme = database_url.split(':').next().unwrap_or("");
    match scheme {
        "sqlite" | "mysql" => Ok(()),
        "mariadb" => Err(DbErr::Custom("MariaDB는 mysql:// 로 지정하세요".into())),
        _ => Err(DbErr::Custom(format!(
            "지원하지 않는 DATABASE_URL 형식 '{scheme}:' — sqlite://경로?mode=rwc 또는 mysql://사용자:비밀번호@호스트:3306/DB"
        ))),
    }
}

/// DB에 연결하고 마이그레이션을 적용한다. SQLite 파일이면 상위 디렉터리를 만든다.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    check_scheme(database_url)?;
    if let Some(path) = sqlite_file_path(database_url)
        && let Some(parent) = Path::new(&path).parent()
        && !parent.as_os_str().is_empty()
    {
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

/// SQLite·MySQL에서 결과가 달라지는 SQL 조각. 직접 쓰는 SQL(`Expr::cust`)이나 집계는 여기 것을 쓴다.
pub mod sql {
    use sea_orm::DbBackend;
    use sea_orm::sea_query::{Alias, Expr, Func};

    /// 글자 수 함수 이름. MySQL의 `LENGTH`는 바이트 수(한글 3바이트)라 `CHAR_LENGTH`를 쓴다.
    pub fn char_length(backend: DbBackend) -> &'static str {
        match backend {
            DbBackend::MySql => "char_length",
            _ => "length",
        }
    }

    /// 정수 컬럼 합계를 i64로 읽을 수 있게. MySQL의 `SUM(BIGINT)`은 DECIMAL이라 SIGNED로 바꾼다.
    pub fn sum_i64(backend: DbBackend, expr: Expr) -> Expr {
        match backend {
            DbBackend::MySql => Func::cast_as(Func::sum(expr), Alias::new("SIGNED")).into(),
            _ => Func::sum(expr).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_scheme, sql, sqlite_file_path};
    use crate::entity::transaction;
    use sea_orm::sea_query::Expr;
    use sea_orm::{DbBackend, EntityTrait, QuerySelect, QueryTrait};

    #[test]
    fn parses_sqlite_paths() {
        assert_eq!(sqlite_file_path("sqlite://data/cashbook.db?mode=rwc").as_deref(), Some("data/cashbook.db"));
        assert_eq!(sqlite_file_path("sqlite:///abs/x.db").as_deref(), Some("/abs/x.db"));
        assert_eq!(sqlite_file_path("sqlite::memory:"), None);
        assert_eq!(sqlite_file_path("mysql://x"), None);
    }

    #[test]
    fn supported_schemes_without_leaking_url() {
        assert!(check_scheme("sqlite://data/cashbook.db?mode=rwc").is_ok());
        assert!(check_scheme("mysql://u:p@db:3306/cashbook").is_ok());
        let err = check_scheme("postgres://u:secret@db/cashbook").unwrap_err().to_string();
        assert!(err.contains("postgres:") && !err.contains("secret"));
    }

    #[test]
    fn sum_is_integer_on_both_backends() {
        let query = |backend| {
            transaction::Entity::find()
                .select_only()
                .column_as(sql::sum_i64(backend, Expr::col(transaction::Column::Amount)), "total")
                .build(backend)
                .to_string()
        };
        assert!(query(DbBackend::MySql).contains("CAST(SUM(`amount`) AS SIGNED)"));
        assert!(query(DbBackend::Sqlite).contains(r#"SUM("amount")"#));
    }
}
