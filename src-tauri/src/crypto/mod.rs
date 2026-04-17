//! Деривация Cosmos-адресов из seed-фраз и приватных ключей.
//!
//! Public API: [`derive_address`], [`WalletInput`], [`ChainConfig`], [`KeyError`].

pub mod key_deriver;

pub use key_deriver::{derive_address, ChainConfig, KeyError, WalletInput};
