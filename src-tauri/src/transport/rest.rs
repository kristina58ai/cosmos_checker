//! REST-клиент Cosmos SDK LCD.
//!
//! 4 endpoint'а:
//! - `/cosmos/bank/v1beta1/balances/{addr}`
//! - `/cosmos/staking/v1beta1/delegations/{addr}`
//! - `/cosmos/distribution/v1beta1/delegators/{addr}/rewards`
//! - `/cosmos/staking/v1beta1/delegators/{addr}/unbonding_delegations`
//!
//! Ошибки парсинга/сетевые возвращаются как [`TransportError`] — вышестоящий
//! слой (`fallback.rs`) решает, ротировать ли endpoint.

use std::time::Duration;

use serde_json::Value;

use super::types::{
    Coin, DecCoin, Delegation, Rewards, UnbondingDelegation, UnbondingEntry, ValidatorReward,
};
use super::{TransportError, TransportResult};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

const PATH_BALANCES: &str = "/cosmos/bank/v1beta1/balances";
const PATH_DELEGATIONS: &str = "/cosmos/staking/v1beta1/delegations";
const PATH_REWARDS_PREFIX: &str = "/cosmos/distribution/v1beta1/delegators";
const PATH_UNBONDING_PREFIX: &str = "/cosmos/staking/v1beta1/delegators";

/// Тонкий REST-клиент, привязанный к одному base_url endpoint'а.
#[derive(Clone)]
pub struct RestClient {
    client: reqwest::Client,
    base_url: String,
}

impl RestClient {
    pub fn new(base_url: impl Into<String>) -> TransportResult<Self> {
        Self::with_timeout(base_url, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> TransportResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("cosmos-checker/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(TransportError::Http)?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn get_json(&self, path: &str) -> TransportResult<Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(TransportError::HttpStatus {
                url,
                status: status.as_u16(),
            });
        }
        resp.json::<Value>()
            .await
            .map_err(|e| TransportError::Parse(format!("json at {url}: {e}")))
    }

    pub async fn fetch_balances(&self, address: &str) -> TransportResult<Vec<Coin>> {
        let v = self.get_json(&format!("{PATH_BALANCES}/{address}")).await?;
        parse_balances(&v)
    }

    pub async fn fetch_delegations(&self, address: &str) -> TransportResult<Vec<Delegation>> {
        let v = self
            .get_json(&format!("{PATH_DELEGATIONS}/{address}"))
            .await?;
        parse_delegations(&v)
    }

    pub async fn fetch_rewards(&self, address: &str) -> TransportResult<Rewards> {
        let v = self
            .get_json(&format!("{PATH_REWARDS_PREFIX}/{address}/rewards"))
            .await?;
        parse_rewards(&v)
    }

    pub async fn fetch_unbonding(
        &self,
        address: &str,
    ) -> TransportResult<Vec<UnbondingDelegation>> {
        let v = self
            .get_json(&format!(
                "{PATH_UNBONDING_PREFIX}/{address}/unbonding_delegations"
            ))
            .await?;
        parse_unbonding(&v)
    }
}

// ---------------------------------------------------------------------------
// Classify reqwest errors into TransportError::Timeout / Http
// ---------------------------------------------------------------------------

fn classify_reqwest_error(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else if e.is_connect() {
        TransportError::Connect(e.to_string())
    } else {
        TransportError::Http(e)
    }
}

// ---------------------------------------------------------------------------
// Pure parsers (также удобны в тестах)
// ---------------------------------------------------------------------------

pub fn parse_balances(v: &Value) -> TransportResult<Vec<Coin>> {
    let arr = v
        .get("balances")
        .and_then(|x| x.as_array())
        .ok_or_else(|| TransportError::Parse("missing `balances`".into()))?;
    arr.iter().map(coin_from_value).collect()
}

pub fn parse_delegations(v: &Value) -> TransportResult<Vec<Delegation>> {
    let arr = v
        .get("delegation_responses")
        .and_then(|x| x.as_array())
        .ok_or_else(|| TransportError::Parse("missing `delegation_responses`".into()))?;
    arr.iter()
        .map(|dr| {
            let delegation = dr
                .get("delegation")
                .ok_or_else(|| TransportError::Parse("missing `delegation`".into()))?;
            let validator_address = delegation
                .get("validator_address")
                .and_then(|x| x.as_str())
                .ok_or_else(|| TransportError::Parse("missing `validator_address`".into()))?
                .to_owned();
            let balance = dr
                .get("balance")
                .ok_or_else(|| TransportError::Parse("missing `balance`".into()))?;
            let balance = coin_from_value(balance)?;
            Ok(Delegation {
                validator_address,
                balance,
            })
        })
        .collect()
}

pub fn parse_rewards(v: &Value) -> TransportResult<Rewards> {
    // `rewards` может отсутствовать у кошельков без delegations — вернём пусто.
    let per_validator_arr = v
        .get("rewards")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let per_validator: Result<Vec<_>, _> = per_validator_arr
        .iter()
        .map(|r| {
            let validator_address = r
                .get("validator_address")
                .and_then(|x| x.as_str())
                .ok_or_else(|| TransportError::Parse("missing `validator_address`".into()))?
                .to_owned();
            let reward = r
                .get("reward")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(dec_coin_from_value)
                        .collect::<Result<_, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok::<_, TransportError>(ValidatorReward {
                validator_address,
                reward,
            })
        })
        .collect();
    let per_validator = per_validator?;

    let total = v
        .get("total")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(dec_coin_from_value)
                .collect::<Result<_, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Rewards {
        per_validator,
        total,
    })
}

pub fn parse_unbonding(v: &Value) -> TransportResult<Vec<UnbondingDelegation>> {
    let arr = v
        .get("unbonding_responses")
        .and_then(|x| x.as_array())
        .ok_or_else(|| TransportError::Parse("missing `unbonding_responses`".into()))?;
    arr.iter()
        .map(|u| {
            let validator_address = u
                .get("validator_address")
                .and_then(|x| x.as_str())
                .ok_or_else(|| TransportError::Parse("missing `validator_address`".into()))?
                .to_owned();
            let entries_arr = u
                .get("entries")
                .and_then(|x| x.as_array())
                .ok_or_else(|| TransportError::Parse("missing `entries`".into()))?;
            let entries = entries_arr
                .iter()
                .map(|e| {
                    Ok::<_, TransportError>(UnbondingEntry {
                        creation_height: e
                            .get("creation_height")
                            .and_then(|x| x.as_str())
                            .unwrap_or("0")
                            .to_owned(),
                        completion_time: e
                            .get("completion_time")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_owned(),
                        initial_balance: e
                            .get("initial_balance")
                            .and_then(|x| x.as_str())
                            .unwrap_or("0")
                            .to_owned(),
                        balance: e
                            .get("balance")
                            .and_then(|x| x.as_str())
                            .unwrap_or("0")
                            .to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(UnbondingDelegation {
                validator_address,
                entries,
            })
        })
        .collect()
}

fn coin_from_value(v: &Value) -> TransportResult<Coin> {
    let denom = v
        .get("denom")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TransportError::Parse("coin: missing `denom`".into()))?
        .to_owned();
    let amount = v
        .get("amount")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TransportError::Parse("coin: missing `amount`".into()))?
        .to_owned();
    Ok(Coin { denom, amount })
}

fn dec_coin_from_value(v: &Value) -> TransportResult<DecCoin> {
    let denom = v
        .get("denom")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TransportError::Parse("dec_coin: missing `denom`".into()))?
        .to_owned();
    let amount = v
        .get("amount")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TransportError::Parse("dec_coin: missing `amount`".into()))?
        .to_owned();
    Ok(DecCoin { denom, amount })
}

// ---------------------------------------------------------------------------
// Tests (parsers only — сетевые в tests/transport_integration.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_balances_response() {
        let v = json!({
            "balances": [
                {"denom": "uatom", "amount": "1500000"},
                {"denom": "ibc/27394FB092...", "amount": "42"}
            ],
            "pagination": {"next_key": null, "total": "2"}
        });
        let coins = parse_balances(&v).unwrap();
        assert_eq!(coins.len(), 2);
        assert_eq!(coins[0].denom, "uatom");
        assert_eq!(coins[0].amount, "1500000");
    }

    #[test]
    fn parse_balances_empty() {
        let v = json!({"balances": [], "pagination": {"next_key": null, "total": "0"}});
        let coins = parse_balances(&v).unwrap();
        assert!(coins.is_empty());
    }

    #[test]
    fn parse_balances_missing_field_errors() {
        let v = json!({"pagination": {}});
        let err = parse_balances(&v).unwrap_err();
        assert!(matches!(err, TransportError::Parse(_)));
    }

    #[test]
    fn parse_delegations_response() {
        let v = json!({
            "delegation_responses": [
                {
                    "delegation": {
                        "delegator_address": "cosmos1d...",
                        "validator_address": "cosmosvaloper1abc",
                        "shares": "10000000.000000000000000000"
                    },
                    "balance": {"denom": "uatom", "amount": "10000000"}
                }
            ],
            "pagination": {"next_key": null, "total": "1"}
        });
        let d = parse_delegations(&v).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].validator_address, "cosmosvaloper1abc");
        assert_eq!(d[0].balance.amount, "10000000");
    }

    #[test]
    fn parse_rewards_response() {
        let v = json!({
            "rewards": [{
                "validator_address": "cosmosvaloper1abc",
                "reward": [
                    {"denom": "uatom", "amount": "123456.789000000000000000"}
                ]
            }],
            "total": [
                {"denom": "uatom", "amount": "123456.789000000000000000"}
            ]
        });
        let r = parse_rewards(&v).unwrap();
        assert_eq!(r.per_validator.len(), 1);
        assert_eq!(
            r.per_validator[0].reward[0].amount,
            "123456.789000000000000000"
        );
        assert_eq!(r.total.len(), 1);
        assert_eq!(r.total[0].denom, "uatom");
    }

    #[test]
    fn parse_rewards_empty() {
        // У кошельков без delegations нет `rewards` вообще.
        let v = json!({"rewards": [], "total": []});
        let r = parse_rewards(&v).unwrap();
        assert!(r.per_validator.is_empty());
        assert!(r.total.is_empty());
    }

    #[test]
    fn parse_unbonding_response() {
        let v = json!({
            "unbonding_responses": [{
                "delegator_address": "cosmos1d...",
                "validator_address": "cosmosvaloper1abc",
                "entries": [{
                    "creation_height": "12345",
                    "completion_time": "2026-05-01T00:00:00Z",
                    "initial_balance": "5000000",
                    "balance": "5000000"
                }]
            }],
            "pagination": {"next_key": null, "total": "1"}
        });
        let u = parse_unbonding(&v).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].entries.len(), 1);
        assert_eq!(u[0].entries[0].completion_time, "2026-05-01T00:00:00Z");
        assert_eq!(u[0].entries[0].balance, "5000000");
    }

    #[test]
    fn parse_unbonding_empty() {
        let v = json!({"unbonding_responses": [], "pagination": {}});
        assert!(parse_unbonding(&v).unwrap().is_empty());
    }
}
