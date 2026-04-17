//! Security primitives.
//!
//! Публичные типы: `SecretString`, `SecretBytes`.
//! Используются всеми модулями, работающими с seed-фразами и приватными ключами.

pub mod secret;

pub use secret::{secret_bytes, secret_string, SecretBytes, SecretString};
