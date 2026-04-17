//! Общие типы данных Cosmos REST/gRPC.
//!
//! Все суммы хранятся как [`String`] — у Cosmos SDK суммы это `sdk.Int` /
//! `sdk.Dec` и могут превышать диапазон `u64` (особенно rewards, где
//! используются десятичные доли). Конвертация в удобочитаемый формат
//! (с учётом exponent из `TokenInfo`) — ответственность вышестоящего слоя.

use serde::{Deserialize, Serialize};

/// `sdk.Coin` — целочисленная сумма в минимальных единицах.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

/// `sdk.DecCoin` — десятичная сумма (rewards).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecCoin {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Delegation {
    pub validator_address: String,
    /// Токены делегации в базовом denom (после конвертации shares→coin).
    pub balance: Coin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorReward {
    pub validator_address: String,
    pub reward: Vec<DecCoin>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rewards {
    /// Награды в разбивке по валидаторам.
    pub per_validator: Vec<ValidatorReward>,
    /// Суммарные награды (`total` из REST-ответа).
    pub total: Vec<DecCoin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnbondingEntry {
    pub creation_height: String,
    pub completion_time: String,
    pub initial_balance: String,
    pub balance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnbondingDelegation {
    pub validator_address: String,
    pub entries: Vec<UnbondingEntry>,
}

/// Агрегированные данные по одному кошельку на одной сети.
///
/// Заполняется по мере успешных запросов. Если какая-то часть упала —
/// соответствующее поле остаётся пустым / None, а ошибка кладётся в
/// [`WalletData::errors`] и чекер делает пометку в `wallet_results.error`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletData {
    pub balances: Vec<Coin>,
    pub delegations: Vec<Delegation>,
    pub rewards: Rewards,
    pub unbonding: Vec<UnbondingDelegation>,
    /// Ошибки по каждому из 4 запросов (если были).
    pub errors: Vec<String>,
}

impl WalletData {
    /// Есть ли хоть какие-то средства (на счёте, в стейкинге, unbonding или rewards).
    pub fn has_funds(&self) -> bool {
        fn any_nonzero_int(coins: &[Coin]) -> bool {
            coins.iter().any(|c| !is_zero_int(&c.amount))
        }
        fn any_nonzero_dec(coins: &[DecCoin]) -> bool {
            coins.iter().any(|c| !is_zero_dec(&c.amount))
        }
        if any_nonzero_int(&self.balances) {
            return true;
        }
        if self
            .delegations
            .iter()
            .any(|d| !is_zero_int(&d.balance.amount))
        {
            return true;
        }
        if any_nonzero_dec(&self.rewards.total) {
            return true;
        }
        if self
            .unbonding
            .iter()
            .flat_map(|u| &u.entries)
            .any(|e| !is_zero_int(&e.balance))
        {
            return true;
        }
        false
    }
}

fn is_zero_int(s: &str) -> bool {
    s.is_empty() || s.chars().all(|c| c == '0')
}

fn is_zero_dec(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // "0" / "0.0" / "0.000000000000000000"
    s.chars().all(|c| c == '0' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_funds_empty_is_false() {
        let d = WalletData::default();
        assert!(!d.has_funds());
    }

    #[test]
    fn has_funds_detects_balance() {
        let d = WalletData {
            balances: vec![Coin {
                denom: "uatom".into(),
                amount: "1500000".into(),
            }],
            ..Default::default()
        };
        assert!(d.has_funds());
    }

    #[test]
    fn has_funds_ignores_zero_amounts() {
        let d = WalletData {
            balances: vec![Coin {
                denom: "uatom".into(),
                amount: "0".into(),
            }],
            rewards: Rewards {
                total: vec![DecCoin {
                    denom: "uatom".into(),
                    amount: "0.000000000000000000".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!d.has_funds());
    }

    #[test]
    fn has_funds_detects_rewards_dec() {
        let d = WalletData {
            rewards: Rewards {
                total: vec![DecCoin {
                    denom: "uatom".into(),
                    amount: "0.5".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(d.has_funds());
    }
}
