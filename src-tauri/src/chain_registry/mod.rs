//! Chain Registry Manager.
//!
//! Загружает метаданные Cosmos SDK сетей из
//! [cosmos/chain-registry](https://github.com/cosmos/chain-registry)
//! (raw-файлы `chain.json` + `assetlist.json`) и кеширует их в SQLite.
//!
//! Публичный flow:
//! 1. [`Registry::get_chain`] — вернуть `ChainInfo` для сети
//!    (из кеша если свежий, иначе через сеть + запись в БД).
//! 2. [`Registry::list_cached`] — вернуть все закешированные сети.
//! 3. [`Registry::force_refresh`] — принудительно обновить одну сеть.
//!
//! Архитектура:
//! - [`fetcher::Fetcher`] — тонкая обёртка над `reqwest`, принимает base_url
//!   (позволяет подменять его на `wiremock` в тестах).
//! - [`parser`] — pure-функции `parse_chain_json` / `parse_assetlist_json`.
//!   Graceful к пропущенным полям.
//!
//! Персист: через уже существующий `db::chains`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::{self, Db};

pub mod fetcher;
pub mod parser;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json parse: {0}")]
    Json(#[from] serde_json::Error),

    #[error("db: {0}")]
    Db(#[from] db::DbError),

    #[error("malformed chain.json: {0}")]
    Malformed(String),

    #[error("chain not found: {0}")]
    NotFound(String),
}

pub type RegistryResult<T> = Result<T, RegistryError>;

// ---------------------------------------------------------------------------
// Data model (in-memory)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum EndpointKind {
    Grpc,
    Rest,
    Rpc,
}

impl EndpointKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grpc => "grpc",
            Self::Rest => "rest",
            Self::Rpc => "rpc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointInfo {
    pub kind: EndpointKind,
    pub address: String,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenInfo {
    pub denom: String,
    pub display_denom: String,
    pub exponent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainInfo {
    pub chain_id: String,
    pub chain_name: String,
    pub pretty_name: Option<String>,
    pub bech32_prefix: String,
    pub slip44: u32,
    pub logo_url: Option<String>,
    pub endpoints: Vec<EndpointInfo>,
    pub tokens: Vec<TokenInfo>,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Registry {
    db: Db,
    fetcher: fetcher::Fetcher,
    cache_ttl: chrono::Duration,
}

impl Registry {
    pub fn new(db: Db, fetcher: fetcher::Fetcher, cache_ttl_hours: i64) -> Self {
        Self {
            db,
            fetcher,
            cache_ttl: chrono::Duration::hours(cache_ttl_hours),
        }
    }

    /// Создать с дефолтами: TTL из `app_settings` (ключ
    /// `chain_registry_cache_hours`, дефолт 24).
    pub fn with_defaults(db: Db) -> RegistryResult<Self> {
        let hours = db::settings::get_i64(&db, "chain_registry_cache_hours")?.unwrap_or(24);
        Ok(Self::new(db, fetcher::Fetcher::new_github(), hours))
    }

    /// Получить сеть. Если в кеше свежая запись — без сети.
    pub async fn get_chain(
        &self,
        chain_name: &str,
        force_refresh: bool,
    ) -> RegistryResult<ChainInfo> {
        if !force_refresh {
            if let Some(cached) = self.load_from_cache(chain_name)? {
                if self.is_fresh(chain_name)? {
                    return Ok(cached);
                }
            }
        }
        self.fetch_and_cache(chain_name).await
    }

    pub async fn force_refresh(&self, chain_name: &str) -> RegistryResult<ChainInfo> {
        self.fetch_and_cache(chain_name).await
    }

    pub fn list_cached(&self) -> RegistryResult<Vec<ChainInfo>> {
        let rows = db::chains::list_chains(&self.db)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(self.row_to_info(r)?);
        }
        Ok(out)
    }

    // ---- private ----------------------------------------------------------

    async fn fetch_and_cache(&self, chain_name: &str) -> RegistryResult<ChainInfo> {
        let chain_json = self.fetcher.fetch_chain_json(chain_name).await?;
        let assetlist_json = self.fetcher.try_fetch_assetlist_json(chain_name).await?;

        let mut info = parser::parse_chain_json(&chain_json)?;
        if let Some(al) = assetlist_json {
            info.tokens = parser::parse_assetlist_json(&al)?;
        }

        self.persist(&info)?;
        Ok(info)
    }

    fn persist(&self, info: &ChainInfo) -> RegistryResult<()> {
        let row = db::chains::ChainRow {
            chain_id: info.chain_id.clone(),
            chain_name: info.chain_name.clone(),
            bech32_prefix: info.bech32_prefix.clone(),
            slip44: info.slip44,
            display_name: info.pretty_name.clone(),
            logo_url: info.logo_url.clone(),
        };
        db::chains::upsert_chain(&self.db, &row)?;

        let eps: Vec<_> = info
            .endpoints
            .iter()
            .map(|e| db::chains::NewEndpoint {
                endpoint_type: e.kind.as_str().to_owned(),
                address: e.address.clone(),
                provider: e.provider.clone(),
            })
            .collect();
        db::chains::replace_endpoints(&self.db, &info.chain_id, &eps)?;

        let toks: Vec<_> = info
            .tokens
            .iter()
            .map(|t| db::chains::NewToken {
                denom: t.denom.clone(),
                display_denom: t.display_denom.clone(),
                exponent: t.exponent,
            })
            .collect();
        db::chains::replace_tokens(&self.db, &info.chain_id, &toks)?;
        Ok(())
    }

    fn load_from_cache(&self, chain_name: &str) -> RegistryResult<Option<ChainInfo>> {
        // chain_name — не PK (им является chain_id), ищем среди всех.
        let rows = db::chains::list_chains(&self.db)?;
        let Some(row) = rows.into_iter().find(|r| r.chain_name == chain_name) else {
            return Ok(None);
        };
        Ok(Some(self.row_to_info(row)?))
    }

    fn row_to_info(&self, row: db::chains::ChainRow) -> RegistryResult<ChainInfo> {
        let eps = db::chains::list_endpoints(&self.db, &row.chain_id)?;
        let toks = db::chains::list_tokens(&self.db, &row.chain_id)?;
        Ok(ChainInfo {
            chain_id: row.chain_id,
            chain_name: row.chain_name,
            pretty_name: row.display_name,
            bech32_prefix: row.bech32_prefix,
            slip44: row.slip44,
            logo_url: row.logo_url,
            endpoints: eps
                .into_iter()
                .map(|e| EndpointInfo {
                    kind: match e.endpoint_type.as_str() {
                        "grpc" => EndpointKind::Grpc,
                        "rest" => EndpointKind::Rest,
                        _ => EndpointKind::Rpc,
                    },
                    address: e.address,
                    provider: e.provider,
                })
                .collect(),
            tokens: toks
                .into_iter()
                .map(|t| TokenInfo {
                    denom: t.denom,
                    display_denom: t.display_denom,
                    exponent: t.exponent,
                })
                .collect(),
        })
    }

    fn is_fresh(&self, chain_name: &str) -> RegistryResult<bool> {
        let c = self.db.conn()?;
        let updated_at: Option<String> = c
            .query_row(
                "SELECT updated_at FROM chains WHERE chain_name = ?1",
                rusqlite::params![chain_name],
                |r| r.get(0),
            )
            .ok();
        let Some(s) = updated_at else {
            return Ok(false);
        };
        // SQLite `datetime('now')` → "YYYY-MM-DD HH:MM:SS" в UTC.
        let parsed = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| RegistryError::Malformed(format!("updated_at: {e}")))?;
        let age = chrono::Utc::now().naive_utc() - parsed;
        Ok(age < self.cache_ttl)
    }
}
