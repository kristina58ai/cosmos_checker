//! Деривация Cosmos-адресов из seed-фраз и приватных ключей.
//!
//! Будет реализовано в Этапе 2 согласно `docs/plan.md` §1.2.
//!
//! Пайплайн:
//! `Seed (BIP39) → PBKDF2 → Master Key (BIP32) → m/44'/slip44'/0'/0/0
//!  → secp256k1 → SHA256+RIPEMD160 → Bech32(prefix, 20 bytes)`

pub mod key_deriver;
