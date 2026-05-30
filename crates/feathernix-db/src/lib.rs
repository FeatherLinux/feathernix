pub mod generations;
pub mod models;
pub mod schema;

use std::path::Path;

use diesel::connection::SimpleConnection;
use diesel::r2d2 as drr2d2;
use diesel::r2d2::ConnectionManager;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use r2d2::{CustomizeConnection, Pool, PooledConnection};
use thiserror::Error;

pub use diesel;
pub use diesel_migrations;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;
pub type DbConn = PooledConnection<ConnectionManager<SqliteConnection>>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database connection pool error: {0}")]
    Pool(#[from] r2d2::Error),
    #[error("query error: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("migration error: {0}")]
    Migration(String),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug)]
struct ConnectionInit;

impl CustomizeConnection<SqliteConnection, drr2d2::Error> for ConnectionInit {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), drr2d2::Error> {
        conn.batch_execute(
            "PRAGMA foreign_keys = ON; \
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(drr2d2::Error::QueryError)
    }
}

#[derive(Clone)]
pub struct Db {
    pool: DbPool,
}

impl Db {
    pub fn open<P: AsRef<Path>>(path: P) -> DbResult<Self> {
        if let Some(parent) = path.as_ref().parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).ok();
        }
        let url = path.as_ref().to_string_lossy().into_owned();
        Self::with_url(&url, 8)
    }

    pub fn open_in_memory() -> DbResult<Self> {
        Self::with_url(":memory:", 1)
    }

    fn with_url(url: &str, max_size: u32) -> DbResult<Self> {
        let manager = ConnectionManager::<SqliteConnection>::new(url);
        let pool = Pool::builder()
            .max_size(max_size)
            .connection_customizer(Box::new(ConnectionInit))
            .build(manager)
            .map_err(|e| DbError::Migration(e.to_string()))?;
        let db = Self { pool };
        db.run_migrations()?;
        Ok(db)
    }

    pub fn run_migrations(&self) -> DbResult<()> {
        let mut conn = self.pool.get()?;
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|e| DbError::Migration(e.to_string()))?;
        Ok(())
    }

    pub fn conn(&self) -> DbResult<DbConn> {
        Ok(self.pool.get()?)
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewRepository, Repository};
    use crate::schema::repositories::dsl as r;
    use chrono::Utc;
    use diesel::prelude::*;

    #[test]
    fn opens_in_memory_and_runs_migrations() {
        let db = Db::open_in_memory().expect("open db");
        let mut conn = db.conn().expect("conn");
        let count: i64 = r::repositories
            .count()
            .get_result(&mut conn)
            .expect("count repos");
        assert_eq!(count, 0);
    }

    #[test]
    fn insert_and_read_repository() {
        let db = Db::open_in_memory().expect("open db");
        let mut conn = db.conn().expect("conn");
        let now = Utc::now().naive_utc();

        let new_repo = NewRepository {
            name: "test",
            base_url: "https://example.com/repo",
            enabled: true,
            priority: 50,
            gpg_check: false,
            gpg_key_url: None,
            signature_required: false,
            last_refresh_at: None,
            etag: None,
            created_at: now,
        };

        diesel::insert_into(r::repositories)
            .values(&new_repo)
            .execute(&mut conn)
            .expect("insert");

        let row: Repository = r::repositories
            .filter(r::name.eq("test"))
            .first(&mut conn)
            .expect("select");

        assert_eq!(row.name, "test");
        assert_eq!(row.base_url, "https://example.com/repo");
        assert!(row.enabled);
        assert_eq!(row.priority, 50);
    }
}
