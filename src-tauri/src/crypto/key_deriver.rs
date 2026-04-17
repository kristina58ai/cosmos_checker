//! Деривация Cosmos-адресов.
//!
//! Пайплайн (из `docs/CLAUDE.md`):
//!
//! ```text
//! Seed (BIP39)
//!   → PBKDF2-HMAC-SHA512  → 64-byte seed
//!     → BIP32  m/44'/{slip44}'/0'/0/0
//!       → secp256k1 private key (32B)
//!         → compressed public key (33B)
//!           → SHA256 → RIPEMD160 → 20B address_bytes
//!             → bech32({prefix}, address_bytes)  → "cosmos1..."
//! ```
//!
//! Приватные данные (seed-фраза и raw privkey) приходят в `SecretBox`
//! и не покидают этот модуль в открытом виде.

use bech32::{primitives::hrp::Hrp, Bech32};
use bip32::{DerivationPath, XPrv};
use bip39::Mnemonic;
use k256::ecdsa::SigningKey;
use ripemd::Ripemd160;
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroize;

use crate::security::{SecretBytes, SecretString};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Конфиг сети: bech32-префикс + slip44 (для derivation path).
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub bech32_prefix: String,
    pub slip44: u32,
}

impl ChainConfig {
    pub fn new(bech32_prefix: impl Into<String>, slip44: u32) -> Self {
        Self {
            bech32_prefix: bech32_prefix.into(),
            slip44,
        }
    }
}

/// Вход пользователя. `Seed`/`PrivateKey` живут в SecretBox и обнуляются при Drop.
pub enum WalletInput {
    /// Готовый bech32-адрес — проходит passthrough.
    Address(String),
    /// BIP39 mnemonic (12 / 15 / 18 / 21 / 24 слова).
    Seed(SecretString),
    /// Raw private key (32 байта, secp256k1).
    PrivateKey(SecretBytes),
}

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),

    #[error("invalid derivation path: {0}")]
    InvalidPath(String),

    #[error("key derivation failed: {0}")]
    Derivation(String),

    #[error("bech32 encoding failed: {0}")]
    Encoding(String),
}

// ---------------------------------------------------------------------------
// Main API
// ---------------------------------------------------------------------------

/// Деривирует bech32-адрес для `chain` из `input`.
///
/// Для `WalletInput::Address` — возвращает строку без изменений (passthrough).
/// Для `Seed` и `PrivateKey` — выполняет полный пайплайн BIP39/32 → secp256k1 → bech32.
pub fn derive_address(input: &WalletInput, chain: &ChainConfig) -> Result<String, KeyError> {
    let compressed_pub: [u8; 33] = match input {
        WalletInput::Address(s) => return Ok(s.clone()),
        WalletInput::Seed(mnemonic) => pubkey_from_mnemonic(mnemonic, chain)?,
        WalletInput::PrivateKey(bytes) => pubkey_from_private_key(bytes)?,
    };

    bech32_address(&compressed_pub, &chain.bech32_prefix)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn pubkey_from_mnemonic(
    mnemonic: &SecretString,
    chain: &ChainConfig,
) -> Result<[u8; 33], KeyError> {
    // Parse mnemonic (validates checksum + wordlist).
    let parsed = Mnemonic::parse_normalized(mnemonic.expose_secret())
        .map_err(|e| KeyError::InvalidMnemonic(e.to_string()))?;

    // BIP39 seed (PBKDF2-HMAC-SHA512 с пустым passphrase).
    let mut seed: [u8; 64] = parsed.to_seed("");

    // BIP32 derivation.
    let path_str = format!("m/44'/{}'/0'/0/0", chain.slip44);
    let path: DerivationPath = path_str
        .parse()
        .map_err(|e: bip32::Error| KeyError::InvalidPath(e.to_string()))?;

    let xprv =
        XPrv::derive_from_path(seed, &path).map_err(|e| KeyError::Derivation(e.to_string()))?;

    // Получаем 32-байтовый raw privkey и сразу приводим к k256 SigningKey.
    // bip32::XPrv::private_key() → &bip32::PrivateKey (= &k256::SecretKey).
    let sk_bytes = xprv.private_key().to_bytes();
    // sk_bytes — FieldBytes<Secp256k1> (GenericArray<u8, U32>); берём через Deref как &[u8].
    let signing =
        SigningKey::from_slice(&sk_bytes[..]).map_err(|e| KeyError::Derivation(e.to_string()))?;

    let pubkey = compressed_pub_from_signing(&signing);

    // Очищаем буфер seed. (xprv/signing будут удалены RAII; SigningKey
    // сам зануляется при drop — у k256 zeroize-on-drop включён.)
    seed.zeroize();

    Ok(pubkey)
}

fn pubkey_from_private_key(bytes: &SecretBytes) -> Result<[u8; 33], KeyError> {
    let raw = bytes.expose_secret();
    if raw.len() != 32 {
        return Err(KeyError::InvalidPrivateKey(format!(
            "expected 32 bytes, got {}",
            raw.len()
        )));
    }
    let signing =
        SigningKey::from_slice(raw).map_err(|e| KeyError::InvalidPrivateKey(e.to_string()))?;
    Ok(compressed_pub_from_signing(&signing))
}

fn compressed_pub_from_signing(sk: &SigningKey) -> [u8; 33] {
    let point = sk.verifying_key().to_encoded_point(true);
    let bytes = point.as_bytes();
    debug_assert_eq!(
        bytes.len(),
        33,
        "compressed secp256k1 pubkey must be 33 bytes"
    );
    let mut out = [0u8; 33];
    out.copy_from_slice(bytes);
    out
}

fn bech32_address(compressed_pub: &[u8; 33], prefix: &str) -> Result<String, KeyError> {
    // SHA256 → RIPEMD160 → 20 bytes.
    let sha = Sha256::digest(compressed_pub);
    let ripe = Ripemd160::digest(sha);

    let hrp = Hrp::parse(prefix).map_err(|e| KeyError::Encoding(e.to_string()))?;
    bech32::encode::<Bech32>(hrp, &ripe).map_err(|e| KeyError::Encoding(e.to_string()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{secret_bytes, secret_string};

    // BIP39 test vector: entropy all-zeros → 12 words "abandon...about".
    const MNEMONIC_12: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    // BIP39 test vector: entropy all-zeros 32B → 24 words "abandon × 23 art".
    const MNEMONIC_24: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    // Ожидаемый cosmos1-адрес для MNEMONIC_12 при slip44=118, m/44'/118'/0'/0/0.
    // Сверено с CosmJS (`DirectSecp256k1HdWallet.fromMnemonic`) и подтверждено
    // первым прогоном cargo test на референсной реализации.
    const EXPECTED_COSMOS_12: &str = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4";

    /// Длина стандартного bech32-адреса Cosmos: HRP + "1" + 32 base32-символа
    /// (для 20-байтного payload) + 6 символов контрольной суммы = 6 + 1 + 32 + 6 = 45.
    /// Для других HRP длина меняется: "osmo1..." = 43, "terra1..." = 44 и т.д.
    fn expected_bech32_len(prefix: &str) -> usize {
        prefix.len() + 1 + 32 + 6
    }

    fn cfg(prefix: &str, slip44: u32) -> ChainConfig {
        ChainConfig::new(prefix, slip44)
    }

    // -----------------------------------------------------------------------
    // 1. derive_address_from_seed_12_words
    // -----------------------------------------------------------------------
    #[test]
    fn derive_address_from_seed_12_words() {
        let input = WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));
        let addr = derive_address(&input, &cfg("cosmos", 118)).expect("derivation");
        assert_eq!(
            addr, EXPECTED_COSMOS_12,
            "Cosmos address for abandon×11 about should match known test vector"
        );
    }

    // -----------------------------------------------------------------------
    // 2. derive_address_from_seed_24_words
    // -----------------------------------------------------------------------
    #[test]
    fn derive_address_from_seed_24_words() {
        let input = WalletInput::Seed(secret_string(MNEMONIC_24.to_owned()));
        let addr = derive_address(&input, &cfg("cosmos", 118)).expect("derivation");
        assert!(
            addr.starts_with("cosmos1"),
            "Expected cosmos1-prefixed address, got: {addr}"
        );
        assert_eq!(
            addr.len(),
            expected_bech32_len("cosmos"),
            "Bech32 cosmos address must be {} chars, got: {addr} ({})",
            expected_bech32_len("cosmos"),
            addr.len(),
        );
    }

    // -----------------------------------------------------------------------
    // 3. derive_address_from_private_key
    // -----------------------------------------------------------------------
    #[test]
    fn derive_address_from_private_key() {
        // Валидный secp256k1 privkey — 32 байта. Выбран произвольно.
        let pk: [u8; 32] =
            hex::decode("c87509a1c067bbde78beffc0f6b4c22a26b58e8f4a8ce85ce3d7ad7b7e50bc3b")
                .unwrap()
                .try_into()
                .unwrap();
        let input = WalletInput::PrivateKey(secret_bytes(pk.to_vec()));
        let addr = derive_address(&input, &cfg("cosmos", 118)).expect("derivation");
        assert!(addr.starts_with("cosmos1"), "got: {addr}");
        assert_eq!(addr.len(), expected_bech32_len("cosmos"));

        // Детерминизм: тот же privkey → тот же адрес.
        let input2 = WalletInput::PrivateKey(secret_bytes(pk.to_vec()));
        let addr2 = derive_address(&input2, &cfg("cosmos", 118)).unwrap();
        assert_eq!(addr, addr2, "derivation must be deterministic");
    }

    // -----------------------------------------------------------------------
    // 4. derive_different_chains_from_same_seed
    // -----------------------------------------------------------------------
    #[test]
    fn derive_different_chains_from_same_seed() {
        let mk = || WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));

        let cosmos = derive_address(&mk(), &cfg("cosmos", 118)).unwrap();
        let osmosis = derive_address(&mk(), &cfg("osmo", 118)).unwrap();
        let terra = derive_address(&mk(), &cfg("terra", 330)).unwrap();

        // Разные префиксы → разные адреса.
        assert_ne!(cosmos, osmosis);
        assert_ne!(cosmos, terra);
        assert_ne!(osmosis, terra);

        // Одинаковый slip44 → одинаковые 20 "данных-байт", отличается только HRP.
        // Проверяем через decode: body (data part) должен совпасть у cosmos и osmo.
        let (hrp_cosmos, data_cosmos) = bech32::decode(&cosmos).unwrap();
        let (hrp_osmo, data_osmo) = bech32::decode(&osmosis).unwrap();
        assert_eq!(hrp_cosmos.as_str(), "cosmos");
        assert_eq!(hrp_osmo.as_str(), "osmo");
        assert_eq!(
            data_cosmos, data_osmo,
            "При равном slip44 сами 20 байт адреса обязаны совпадать"
        );

        // Terra — slip44 отличается → данные тоже отличаются.
        let (_, data_terra) = bech32::decode(&terra).unwrap();
        assert_ne!(data_cosmos, data_terra);
    }

    // -----------------------------------------------------------------------
    // 5. derive_with_custom_slip44
    // -----------------------------------------------------------------------
    #[test]
    fn derive_with_custom_slip44() {
        let input = WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));
        let addr = derive_address(&input, &cfg("terra", 330)).unwrap();
        assert!(addr.starts_with("terra1"), "got: {addr}");

        let other = WalletInput::Seed(secret_string(MNEMONIC_12.to_owned()));
        let addr_default = derive_address(&other, &cfg("terra", 118)).unwrap();
        assert_ne!(addr, addr_default, "slip44 must affect derived address");
    }

    // -----------------------------------------------------------------------
    // 6. invalid_seed_phrase_rejected
    // -----------------------------------------------------------------------
    #[test]
    fn invalid_seed_phrase_rejected() {
        let input = WalletInput::Seed(secret_string(
            "invalid words here not a real mnemonic".to_owned(),
        ));
        let err = derive_address(&input, &cfg("cosmos", 118)).unwrap_err();
        matches!(err, KeyError::InvalidMnemonic(_));
    }

    // -----------------------------------------------------------------------
    // 7. invalid_private_key_rejected
    // -----------------------------------------------------------------------
    #[test]
    fn invalid_private_key_rejected() {
        // Неверная длина.
        let short = WalletInput::PrivateKey(secret_bytes(vec![1, 2, 3]));
        let err = derive_address(&short, &cfg("cosmos", 118)).unwrap_err();
        assert!(matches!(err, KeyError::InvalidPrivateKey(_)));

        // Невалидный скаляр для secp256k1 (всё нули — запрещено).
        let zero = WalletInput::PrivateKey(secret_bytes(vec![0u8; 32]));
        let err = derive_address(&zero, &cfg("cosmos", 118)).unwrap_err();
        assert!(matches!(err, KeyError::InvalidPrivateKey(_)));
    }

    // -----------------------------------------------------------------------
    // 8. zeroize_seed_after_derivation  (proxy-тест через Zeroize::zeroize на Vec)
    // -----------------------------------------------------------------------
    #[test]
    fn zeroize_vec_clears_buffer() {
        use zeroize::Zeroize;
        let mut buf = vec![0xAAu8; 64];
        let ptr = buf.as_ptr();
        let len = buf.len();
        buf.zeroize();
        // После zeroize сам Vec не освобождён (ёмкость та же),
        // буфер должен быть весь нулевой.
        unsafe {
            let slice = std::slice::from_raw_parts(ptr, len);
            assert!(slice.iter().all(|&b| b == 0), "buffer must be zeroed");
        }
    }

    // -----------------------------------------------------------------------
    // 9. passthrough_address_input
    // -----------------------------------------------------------------------
    #[test]
    fn passthrough_address_input() {
        let known = EXPECTED_COSMOS_12;
        let input = WalletInput::Address(known.to_owned());
        let addr = derive_address(&input, &cfg("cosmos", 118)).unwrap();
        assert_eq!(addr, known);
    }
}
