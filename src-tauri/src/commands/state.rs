//! Shared application state for Tauri IPC commands.
//!
//! [`AppState`] — единый контейнер, который держит:
//! - [`Db`] handle (Arc'd pool).
//! - [`Registry`] (chain registry + кеш в SQLite).
//! - `pending_imports` — классифицированные входы (адреса/seed'ы/privkey'и)
//!   после [`super::import::import_wallets`]. Хранятся только в памяти, в БД
//!   никогда не попадают (см. CLAUDE.md §5 T1).
//! - `pending_proxies` — последний распарсенный proxy-лист.
//! - `running_checks` — активные проверки (session_id → [`CancellationToken`]).
//!
//! Тесты конструируют `AppState::new_in_memory()` (без сетевых зависимостей).

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::chain_registry::{Registry, RegistryError};
use crate::db::{Db, DbError};
use crate::file_io::{FileIoError, InputEntry};
use crate::proxy::{Proxy, ProxyError};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Ошибка, пробрасываемая из IPC-команд на фронтенд.
///
/// На стороне JS это будет обычный `Error { code, message }`. `code` —
/// машинно-читаемый enum-вариант, `message` — человекочитаемое описание.
#[derive(Debug, Error, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum CommandError {
    #[error("db: {0}")]
    Db(String),

    #[error("file io: {0}")]
    FileIo(String),

    #[error("proxy: {0}")]
    Proxy(String),

    #[error("registry: {0}")]
    Registry(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    Invalid(String),

    #[error("internal: {0}")]
    Internal(String),
}

pub type CommandResult<T> = Result<T, CommandError>;

// ---- convertions ----------------------------------------------------------

impl From<DbError> for CommandError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound(m) => CommandError::NotFound(m),
            DbError::Invalid(m) => CommandError::Invalid(m),
            other => CommandError::Db(other.to_string()),
        }
    }
}

impl From<FileIoError> for CommandError {
    fn from(e: FileIoError) -> Self {
        CommandError::FileIo(e.to_string())
    }
}

impl From<ProxyError> for CommandError {
    fn from(e: ProxyError) -> Self {
        CommandError::Proxy(e.to_string())
    }
}

impl From<RegistryError> for CommandError {
    fn from(e: RegistryError) -> Self {
        match e {
            RegistryError::NotFound(m) => CommandError::NotFound(m),
            other => CommandError::Registry(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Shared state, клонируется дёшево (всё под Arc).
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub registry: Arc<Registry>,
    pub pending_imports: Arc<Mutex<HashMap<i64, Vec<InputEntry>>>>,
    pub pending_proxies: Arc<Mutex<Vec<Proxy>>>,
    pub running_checks: Arc<Mutex<HashMap<i64, CancellationToken>>>,
    next_import_id: Arc<AtomicI64>,
}

impl AppState {
    pub fn new(db: Db, registry: Registry) -> Self {
        Self {
            db,
            registry: Arc::new(registry),
            pending_imports: Arc::new(Mutex::new(HashMap::new())),
            pending_proxies: Arc::new(Mutex::new(Vec::new())),
            running_checks: Arc::new(Mutex::new(HashMap::new())),
            next_import_id: Arc::new(AtomicI64::new(1)),
        }
    }

    /// In-memory state для тестов. Registry с дефолтными настройками, но сеть
    /// ни разу не дёргается, пока тест не вызовет `refresh_chain`.
    pub fn new_in_memory() -> CommandResult<Self> {
        let db = Db::in_memory().map_err(CommandError::from)?;
        let registry = Registry::with_defaults(db.clone())?;
        Ok(Self::new(db, registry))
    }

    /// Выдаёт следующий id для `pending_imports`. Стартует с 1.
    pub fn next_import_token(&self) -> i64 {
        self.next_import_id.fetch_add(1, Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_error_serde_round_trip() {
        let e = CommandError::NotFound("chain".into());
        let s = serde_json::to_string(&e).unwrap();
        let back: CommandError = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn import_tokens_are_monotonic() {
        let st = AppState::new_in_memory().unwrap();
        let a = st.next_import_token();
        let b = st.next_import_token();
        assert!(b > a);
    }

    #[test]
    fn from_db_not_found_preserves_code() {
        let e: CommandError = DbError::NotFound("x".into()).into();
        assert!(matches!(e, CommandError::NotFound(_)));
    }
}
