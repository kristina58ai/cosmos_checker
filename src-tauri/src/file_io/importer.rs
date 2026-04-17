//! Importer: классификация строк входного файла.
//!
//! Поддерживаемые типы входа:
//! - **Address** — bech32 Cosmos-адрес (произвольный prefix: cosmos/osmo/terra/…).
//! - **Seed12** — 12 слов BIP-39 (пробелы — разделитель; кейс нечувствителен).
//! - **Seed24** — 24 слова BIP-39.
//! - **PrivateKeyHex** — 32 байта как hex (64 hex-символа, опционально `0x…`).
//!
//! Эвристика:
//! 1. Пустые строки / комментарии (`#`, `//`) — пропуск.
//! 2. Если начинается с `0x` или состоит из ровно 64 hex-символов → PrivateKeyHex.
//! 3. Если 12 или 24 слова, каждое слово ∈ BIP-39 wordlist → Seed.
//! 4. Если содержит 1..=2 компонентов вида `<prefix>1<bech32_body>` с валидной
//!    кодировкой bech32 → Address.
//! 5. Иначе — `Invalid`.
//!
//! ВАЖНО: криптографические секреты (seed, privkey) помещаются в
//! [`secrecy::SecretString`] / [`secrecy::SecretBox<[u8]>`] как можно раньше —
//! так же, как это делает crypto-слой.

use std::path::Path;

use bip39::Mnemonic;
use secrecy::{SecretBox, SecretString};

use super::FileIoError;

/// Классифицированная строка входа.
pub enum InputEntry {
    Address(String),
    /// 12 слов (hot path в UI).
    Seed12(SecretString),
    /// 24 слова.
    Seed24(SecretString),
    /// 32 байта секретного ключа.
    PrivateKeyHex(SecretBox<[u8]>),
}

impl InputEntry {
    pub fn kind(&self) -> InputKind {
        match self {
            InputEntry::Address(_) => InputKind::Address,
            InputEntry::Seed12(_) => InputKind::Seed12,
            InputEntry::Seed24(_) => InputKind::Seed24,
            InputEntry::PrivateKeyHex(_) => InputKind::PrivateKey,
        }
    }
}

impl std::fmt::Debug for InputEntry {
    /// Логируем только тип записи — не сам материал.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputEntry::Address(a) => f.debug_tuple("Address").field(a).finish(),
            InputEntry::Seed12(_) => f.write_str("Seed12(***)"),
            InputEntry::Seed24(_) => f.write_str("Seed24(***)"),
            InputEntry::PrivateKeyHex(_) => f.write_str("PrivateKeyHex(***)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Address,
    Seed12,
    Seed24,
    PrivateKey,
}

/// Результат импорта: валидные записи + ошибки построчно.
pub struct ImportReport {
    pub entries: Vec<InputEntry>,
    pub errors: Vec<String>,
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Количество по типам.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut a = 0;
        let mut s12 = 0;
        let mut s24 = 0;
        let mut pk = 0;
        for e in &self.entries {
            match e.kind() {
                InputKind::Address => a += 1,
                InputKind::Seed12 => s12 += 1,
                InputKind::Seed24 => s24 += 1,
                InputKind::PrivateKey => pk += 1,
            }
        }
        (a, s12, s24, pk)
    }
}

/// Классифицирует одну строку. `None` означает "пропустить" (пустая/комментарий).
pub fn classify_line(raw: &str) -> Result<Option<InputEntry>, FileIoError> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return Ok(None);
    }

    // 1. PrivateKey hex — 64 hex-символа, возможно с префиксом 0x.
    let hex_candidate = line.strip_prefix("0x").unwrap_or(line);
    if hex_candidate.len() == 64 && hex_candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes =
            hex::decode(hex_candidate).map_err(|e| FileIoError::Invalid(format!("hex: {e}")))?;
        let boxed: Box<[u8]> = bytes.into_boxed_slice();
        return Ok(Some(InputEntry::PrivateKeyHex(SecretBox::new(boxed))));
    }

    // 2. Seed — 12 или 24 слова.
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.len() == 12 || words.len() == 24 {
        if is_valid_bip39(&words) {
            let phrase = words.join(" ").to_lowercase();
            let secret = SecretString::from(phrase);
            return Ok(Some(if words.len() == 12 {
                InputEntry::Seed12(secret)
            } else {
                InputEntry::Seed24(secret)
            }));
        } else {
            return Err(FileIoError::Invalid(
                "12/24 words present but not all are BIP-39".into(),
            ));
        }
    }

    // 3. Address — bech32 (одно "слово" вида prefix1payload).
    if words.len() == 1 && looks_like_bech32(words[0]) {
        return Ok(Some(InputEntry::Address(words[0].to_owned())));
    }

    Err(FileIoError::Invalid(format!(
        "cannot classify `{}`",
        truncate(line, 32)
    )))
}

fn is_valid_bip39(words: &[&str]) -> bool {
    let phrase = words.join(" ").to_lowercase();
    Mnemonic::parse_normalized(&phrase).is_ok()
}

fn looks_like_bech32(s: &str) -> bool {
    // bech32 separator '1' делит на HRP (1..=83) и data (6+ символов).
    // Характеристики bech32:
    // - только lowercase (мы всё приводим к lower через strict check ниже);
    // - HRP — alpha;
    // - data — из bech32 charset (без "1,b,i,o").
    let s_lower = s.to_ascii_lowercase();
    let Some(idx) = s_lower.rfind('1') else {
        return false;
    };
    let (hrp, data) = s_lower.split_at(idx);
    let data = &data[1..]; // drop '1'
    if hrp.is_empty() || data.len() < 6 {
        return false;
    }
    if !hrp.bytes().all(|b| b.is_ascii_lowercase()) {
        return false;
    }
    let charset = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    if !data.bytes().all(|b| charset.contains(&b)) {
        return false;
    }
    // Финальная верификация контрольной суммы через bech32 decode.
    bech32::decode(&s_lower).is_ok()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Классифицирует всё содержимое файла (`\n`-delimited).
pub fn classify_text(text: &str) -> ImportReport {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (ln, raw) in text.lines().enumerate() {
        match classify_line(raw) {
            Ok(None) => {}
            Ok(Some(e)) => entries.push(e),
            Err(e) => errors.push(format!("line {}: {e}", ln + 1)),
        }
    }
    ImportReport { entries, errors }
}

/// Читает файл и классифицирует его. Использует `BufReader` для больших файлов.
pub fn import_file(path: impl AsRef<Path>) -> Result<ImportReport, FileIoError> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (ln, line) in reader.lines().enumerate() {
        let line = line?;
        match classify_line(&line) {
            Ok(None) => {}
            Ok(Some(e)) => entries.push(e),
            Err(e) => errors.push(format!("line {}: {e}", ln + 1)),
        }
    }
    Ok(ImportReport { entries, errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABANDON_12: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const ABANDON_24: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn classify_address_cosmos() {
        let addr = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4";
        let e = classify_line(addr).unwrap().unwrap();
        assert_eq!(e.kind(), InputKind::Address);
    }

    #[test]
    fn classify_seed_12() {
        let e = classify_line(ABANDON_12).unwrap().unwrap();
        assert_eq!(e.kind(), InputKind::Seed12);
    }

    #[test]
    fn classify_seed_24() {
        let e = classify_line(ABANDON_24).unwrap().unwrap();
        assert_eq!(e.kind(), InputKind::Seed24);
    }

    #[test]
    fn classify_privkey_plain() {
        let hex = "a".repeat(64);
        let e = classify_line(&hex).unwrap().unwrap();
        assert_eq!(e.kind(), InputKind::PrivateKey);
    }

    #[test]
    fn classify_privkey_0x_prefix() {
        let hex = format!("0x{}", "b".repeat(64));
        let e = classify_line(&hex).unwrap().unwrap();
        assert_eq!(e.kind(), InputKind::PrivateKey);
    }

    #[test]
    fn empty_and_comments_skipped() {
        assert!(classify_line("").unwrap().is_none());
        assert!(classify_line("   \t").unwrap().is_none());
        assert!(classify_line("# комментарий").unwrap().is_none());
        assert!(classify_line("// noop").unwrap().is_none());
    }

    #[test]
    fn invalid_rejected() {
        assert!(classify_line("not_a_valid_input").is_err());
        // 12 "слов", но одно не из BIP-39.
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzz";
        assert!(classify_line(bad).is_err());
        // 11 слов — не 12 и не 24, и не bech32 → ошибка.
        let eleven = "abandon ".repeat(11);
        assert!(classify_line(eleven.trim()).is_err());
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        let e = classify_line(ABANDON_12).unwrap().unwrap();
        let dbg = format!("{e:?}");
        assert_eq!(dbg, "Seed12(***)");
        assert!(!dbg.contains("abandon"));

        let pk = classify_line(&"c".repeat(64)).unwrap().unwrap();
        let dbg_pk = format!("{pk:?}");
        assert_eq!(dbg_pk, "PrivateKeyHex(***)");
    }

    #[test]
    fn classify_text_mixed() {
        let text = format!(
            "# header\n{addr}\n\n{seed12}\n{seed24}\n{pk}\ngarbage\n",
            addr = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4",
            seed12 = ABANDON_12,
            seed24 = ABANDON_24,
            pk = "a".repeat(64),
        );
        let r = classify_text(&text);
        assert_eq!(r.entries.len(), 4);
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].contains("line 7"));
        let (a, s12, s24, pk) = r.counts();
        assert_eq!((a, s12, s24, pk), (1, 1, 1, 1));
    }
}
