//! SQLite Database Layer.
//!
//! Обёртка над `rusqlite` + `r2d2_sqlite` pool. Все запросы идут через
//! prepared statements (`params![]`) — см. `docs/CLAUDE.md §5` (T6: SQL injection).
//!
//! Public API:
//! - [`Db`] — cheap-to-clone handle (Arc<Pool>).
//! - [`DbError`] / [`DbResult`] — общие ошибки слоя.
//! - Подмодули [`chains`], [`sessions`], [`results`], [`settings`] — CRUD.
//!
//! CRITICAL: seed-фразы и приватные ключи в БД не пишутся никогда.

use std::path::Path;
use std::sync::Arc;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use thiserror::Error;

pub mod chains;
pub mod results;
pub mod sessions;
pub mod settings;

/// Встроенный текст миграции `001_init.sql`.
/// Путь относительно `src-tauri/src/db/` → `../../migrations/001_init.sql`.
const MIGRATION_001_INIT: &str = include_str!("../../migrations/001_init.sql");

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("connection pool: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("migration failed: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid value: {0}")]
    Invalid(String),
}

pub type DbResult<T> = Result<T, DbError>;

// ---------------------------------------------------------------------------
// Db handle
// ---------------------------------------------------------------------------

/// Тонкий handle над r2d2 SQLite-пулом.
///
/// Клонирование дешёвое (только Arc::clone). Реально соединение берётся
/// через [`Db::conn`] и возвращается в пул при drop.
#[derive(Clone)]
pub struct Db {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl Db {
    /// Открыть БД на диске. Если файла нет — создаётся. Миграции применяются сразу.
    pub fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let manager = SqliteConnectionManager::file(path.as_ref()).with_init(pragma_foreign_keys);
        let pool = Pool::builder().max_size(8).build(manager)?;
        let db = Self {
            pool: Arc::new(pool),
        };
        db.apply_migrations()?;
        Ok(db)
    }

    /// In-memory БД — используется в тестах и временных операциях.
    ///
    /// NB: так как пул создаёт несколько соединений, а `:memory:` в SQLite —
    /// per-connection, используем shared cache + unique URI. Для простоты
    /// тестов фиксируем `max_size=1` — тесты коротко живут.
    pub fn in_memory() -> DbResult<Self> {
        let manager = SqliteConnectionManager::memory().with_init(pragma_foreign_keys);
        let pool = Pool::builder().max_size(1).build(manager)?;
        let db = Self {
            pool: Arc::new(pool),
        };
        db.apply_migrations()?;
        Ok(db)
    }

    /// Получить соединение из пула.
    pub fn conn(&self) -> DbResult<PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }

    /// Применить все миграции идемпотентно.
    ///
    /// Сейчас миграция ровно одна (`001_init.sql`) и сам SQL использует
    /// `CREATE TABLE IF NOT EXISTS` / `INSERT OR IGNORE`, поэтому повторное
    /// применение безопасно. Когда добавятся новые миграции — подключим
    /// таблицу `schema_migrations` и версию.
    pub fn apply_migrations(&self) -> DbResult<()> {
        let c = self.conn()?;
        c.execute_batch(MIGRATION_001_INIT)
            .map_err(|e| DbError::Migration(e.to_string()))?;
        Ok(())
    }

    /// Выполнить замыкание в транзакции. Авто-rollback при ошибке.
    pub fn with_tx<F, T>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> DbResult<T>,
    {
        let mut c = self.conn()?;
        let tx = c.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

fn pragma_foreign_keys(c: &mut Connection) -> rusqlite::Result<()> {
    c.execute_batch("PRAGMA foreign_keys = ON;")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: проверить что таблица существует.
    fn has_table(db: &Db, name: &str) -> bool {
        let c = db.conn().unwrap();
        let exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .unwrap();
        exists == 1
    }

    #[test]
    fn migrations_apply_clean_db() {
        let db = Db::in_memory().expect("open");
        for t in [
            "chains",
            "chain_endpoints",
            "chain_tokens",
            "check_sessions",
            "wallet_results",
            "app_settings",
        ] {
            assert!(has_table(&db, t), "table {t} must exist");
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Db::in_memory().expect("open");
        // Повторный вызов не должен падать.
        db.apply_migrations().expect("re-apply");
        db.apply_migrations().expect("re-apply 2");
        assert!(has_table(&db, "chains"));
    }

    #[test]
    fn foreign_keys_enabled() {
        let db = Db::in_memory().unwrap();
        let c = db.conn().unwrap();
        let fk: i64 = c
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1, "foreign_keys must be ON");
    }

    #[test]
    fn default_settings_seeded() {
        let db = Db::in_memory().unwrap();
        let c = db.conn().unwrap();
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM app_settings", [], |r| r.get(0))
            .unwrap();
        assert!(count >= 5, "default settings seed should insert 5 rows");
    }
}
