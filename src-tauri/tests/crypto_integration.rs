//! Stage 2 integration test: проверяет, что public API `cosmos_checker::crypto`
//! доступен из внешнего crate и работает end-to-end.

use cosmos_checker::crypto::{derive_address, ChainConfig, WalletInput};
use cosmos_checker::security::secret_string;

const MNEMONIC_12: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn public_api_derives_cosmos_address() {
    let input = WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));
    let chain = ChainConfig::new("cosmos", 118);
    let addr = derive_address(&input, &chain).expect("derivation must succeed");
    assert!(addr.starts_with("cosmos1"));
    assert_eq!(addr, "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4");
}

#[test]
fn public_api_derives_osmosis_address() {
    let input = WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));
    let chain = ChainConfig::new("osmo", 118);
    let addr = derive_address(&input, &chain).expect("derivation");
    assert!(addr.starts_with("osmo1"));
}
