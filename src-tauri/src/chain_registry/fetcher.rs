//! HTTP-fetcher для cosmos/chain-registry.
//!
//! По умолчанию ходит на
//! `https://raw.githubusercontent.com/cosmos/chain-registry/master/{chain_name}/...`.
//! `base_url` параметризован — в тестах мы подставляем wiremock URL.

use std::time::Duration;

use serde_json::Value;

use super::{RegistryError, RegistryResult};

const DEFAULT_BASE_URL: &str = "https://raw.githubusercontent.com/cosmos/chain-registry/master";
const DEFAULT_TIMEOUT_SECS: u64 = 10;
const USER_AGENT: &str = concat!("cosmos-checker/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub struct Fetcher {
    client: reqwest::Client,
    base_url: String,
}

impl Fetcher {
    /// Дефолтный fetcher — raw.githubusercontent.com.
    pub fn new_github() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .expect("reqwest::Client build");
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    /// Специализированный конструктор для тестов — короткий timeout.
    #[doc(hidden)]
    pub fn for_tests(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest::Client build");
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Скачать `chain.json` для указанной сети.
    pub async fn fetch_chain_json(&self, chain_name: &str) -> RegistryResult<Value> {
        let url = format!("{}/{}/chain.json", self.base_url, chain_name);
        let resp = self.client.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RegistryError::NotFound(chain_name.to_owned()));
        }
        let resp = resp.error_for_status()?;
        let v = resp.json::<Value>().await?;
        Ok(v)
    }

    /// Скачать `assetlist.json`. Если его нет (404) — возвращается `Ok(None)`.
    pub async fn try_fetch_assetlist_json(
        &self,
        chain_name: &str,
    ) -> RegistryResult<Option<Value>> {
        let url = format!("{}/{}/assetlist.json", self.base_url, chain_name);
        let resp = self.client.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = resp.error_for_status()?;
        Ok(Some(resp.json::<Value>().await?))
    }
}
