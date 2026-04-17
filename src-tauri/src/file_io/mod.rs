//! File I/O layer — импорт входных файлов (addresses/seeds/privkeys) и
//! экспорт результатов проверки в txt/csv. Stage 7.
//!
//! Importer (`importer.rs`):
//! - классификация строк: Address / Seed12 / Seed24 / PrivateKeyHex
//! - BIP-39 валидация через `Mnemonic::parse_normalized`
//! - 64-hex → private key (в `SecretBox<[u8]>`)
//! - bech32 verification для адресов
//! - пустые/комментарии пропускаются, ошибки собираются построчно
//!
//! Exporter (`exporter.rs`):
//! - TXT (TAB-delimited) / CSV (RFC 4180)
//! - фильтры: `only_with_funds`, `chain_id`, `input_type`
//! - header в первой строке
//! - sanitization: таб/newline внутри полей заменяются на пробел
//!   (TXT) или экранируются (CSV)
//!
//! Секреты (seed/privkey) обёрнуты в `secrecy::SecretString` / `SecretBox`
//! сразу после классификации; `Debug` не печатает содержимое.

use thiserror::Error;

pub mod exporter;
pub mod importer;

pub use exporter::{export_to_file, export_to_writer, ExportFilter, ExportFormat};
pub use importer::{
    classify_line, classify_text, import_file, ImportReport, InputEntry, InputKind,
};

#[derive(Debug, Error)]
pub enum FileIoError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid: {0}")]
    Invalid(String),
}
