//! Transport Layer — Cosmos API client (REST + gRPC) с fallback chain.
//!
//! Архитектура:
//! - [`rest::RestClient`] — REST поверх `reqwest` с настраиваемым timeout.
//! - [`grpc::GrpcClient`] — gRPC-клиент (stub до stage 5b, см. `grpc.rs`).
//! - [`endpoints::EndpointPool`] — round-robin пул с пометкой unhealthy.
//! - [`fallback::query_wallet`] — высокоуровневый запрос данных кошелька
//!   со всей цепочкой fallback: gRPC → REST(pool) → cosmos.directory.
//!
//! Таймаут на один запрос: 5s (см. [`rest::DEFAULT_TIMEOUT`]).
//! На уровне пула endpoint'ы ротируются round-robin; при сетевой ошибке
//! endpoint помечается unhealthy и пропускается в последующих выборках.
//!
//! Ошибки парсинга (например, несовместимый формат REST-ответа) не
//! считаются endpoint-level — они не приводят к пометке unhealthy.

use thiserror::Error;

pub mod endpoints;
pub mod fallback;
pub mod grpc;
pub mod rest;
pub mod types;

pub use endpoints::EndpointPool;
pub use fallback::{query_wallet, query_wallet_partial, TransportPools};
pub use grpc::GrpcClient;
pub use rest::RestClient;
pub use types::{
    Coin, DecCoin, Delegation, Rewards, UnbondingDelegation, UnbondingEntry, ValidatorReward,
    WalletData,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("http: {0}")]
    Http(#[source] reqwest::Error),

    #[error("connect: {0}")]
    Connect(String),

    #[error("http status {status} at {url}")]
    HttpStatus { url: String, status: u16 },

    #[error("request timed out")]
    Timeout,

    #[error("parse: {0}")]
    Parse(String),

    #[error("gRPC unavailable (stub; will be enabled in stage 5b)")]
    GrpcUnavailable,

    #[error("all endpoints failed: {0:?}")]
    AllEndpointsFailed(Vec<String>),
}

pub type TransportResult<T> = Result<T, TransportError>;
