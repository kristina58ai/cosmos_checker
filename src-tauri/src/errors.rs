//! Общие типы ошибок приложения.
//!
//! Конкретные ошибки модулей (KeyError, DbError, TransportError, ...)
//! живут в своих модулях и конвертируются сюда через `From`.

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("{0}")]
    Other(String),
}

/// Serializable-обёртка, которую можно вернуть в Tauri IPC
/// (Tauri v2 требует `Serialize + Send` на ответ).
#[derive(Debug, Serialize)]
pub struct AppErrorResponse {
    pub kind: &'static str,
    pub message: String,
}

impl From<AppError> for AppErrorResponse {
    fn from(err: AppError) -> Self {
        let kind = match &err {
            AppError::Crypto(_) => "crypto",
            AppError::Db(_) => "db",
            AppError::Transport(_) => "transport",
            AppError::Io(_) => "io",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::Other(_) => "other",
        };
        Self {
            kind,
            message: err.to_string(),
        }
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
