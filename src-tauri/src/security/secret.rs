//! Обёртки над `secrecy::SecretBox<T>` для seed-фраз и приватных ключей.
//!
//! Семантика:
//! - `Debug`/`Display` печатают `[REDACTED]` — нельзя случайно залогировать секрет.
//! - При `Drop` внутренняя память зануляется через `zeroize`.
//! - Доступ к значению — только через `expose_secret()`.
//!
//! Соответствует Threat Model из `docs/CLAUDE.md §5` (T1/T2/T5).

use secrecy::SecretBox;

/// Секретная строка (mnemonic seed-фраза).
pub type SecretString = SecretBox<str>;

/// Секретные байты (raw private key, 32 байта).
pub type SecretBytes = SecretBox<[u8]>;

/// Завернуть `String` в `SecretString` — с явной передачей владения.
///
/// Исходный `String` будет зануляется при drop внутри SecretBox.
pub fn secret_string(s: String) -> SecretString {
    // SecretBox::new требует Box<T>; для str используем Box<str>.
    SecretBox::new(s.into_boxed_str())
}

/// Завернуть `Vec<u8>` в `SecretBytes`.
pub fn secret_bytes(v: Vec<u8>) -> SecretBytes {
    SecretBox::new(v.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn secret_string_debug_is_redacted() {
        let s = secret_string("abandon abandon about".to_owned());
        let dbg = format!("{:?}", s);
        assert!(
            !dbg.contains("abandon"),
            "Debug должен скрывать содержимое, получено: {dbg}"
        );
        assert!(
            dbg.to_lowercase().contains("redact"),
            "Debug должен явно писать REDACTED, получено: {dbg}"
        );
    }

    #[test]
    fn secret_bytes_expose_gives_back_data() {
        let original = vec![1u8, 2, 3, 4];
        let s = secret_bytes(original.clone());
        assert_eq!(s.expose_secret(), original.as_slice());
    }
}
