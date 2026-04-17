//! Exporter: выгрузка результатов проверки в .txt / .csv.
//!
//! Формат по умолчанию — TXT с TAB-разделителем (UI-friendly для копипасты
//! в Excel/LibreOffice Calc). Первая строка — заголовок.
//!
//! Фильтры: `only_with_funds`, `chain_id`, `input_type`.
//!
//! ВАЖНО: seed/privkey в результаты НЕ попадают — в `wallet_results` лежит
//! только derived-адрес. Так что экспорт безопасен по определению.

use std::io::Write;
use std::path::Path;

use crate::db::results::{InputType, WalletResultRow};

use super::FileIoError;

/// Настройки экспорта.
#[derive(Debug, Clone, Default)]
pub struct ExportFilter {
    pub only_with_funds: bool,
    pub chain_id: Option<String>,
    pub input_type: Option<InputType>,
}

impl ExportFilter {
    pub fn matches(&self, r: &WalletResultRow) -> bool {
        if self.only_with_funds && !r.has_funds {
            return false;
        }
        if let Some(ref c) = self.chain_id {
            if &r.chain_id != c {
                return false;
            }
        }
        if let Some(it) = self.input_type {
            if r.input_type != it {
                return false;
            }
        }
        true
    }
}

/// Формат выходного файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// TAB-delimited `.txt`.
    Txt,
    /// CSV по RFC 4180.
    Csv,
}

/// Колонки экспорта. Порядок совпадает с заголовком.
const HEADERS: &[&str] = &[
    "chain_id",
    "address",
    "input_type",
    "has_funds",
    "balance",
    "staked",
    "rewards",
    "unbonding",
    "error",
    "checked_at",
];

/// Пишет результаты в writer в указанном формате.
pub fn export_to_writer<W: Write>(
    writer: &mut W,
    rows: &[WalletResultRow],
    filter: &ExportFilter,
    fmt: ExportFormat,
) -> Result<usize, FileIoError> {
    write_header(writer, fmt)?;
    let mut n = 0;
    for r in rows {
        if !filter.matches(r) {
            continue;
        }
        write_row(writer, r, fmt)?;
        n += 1;
    }
    writer.flush()?;
    Ok(n)
}

/// Экспорт в файл.
pub fn export_to_file(
    path: impl AsRef<Path>,
    rows: &[WalletResultRow],
    filter: &ExportFilter,
    fmt: ExportFormat,
) -> Result<usize, FileIoError> {
    let file = std::fs::File::create(path)?;
    let mut w = std::io::BufWriter::new(file);
    export_to_writer(&mut w, rows, filter, fmt)
}

fn write_header<W: Write>(w: &mut W, fmt: ExportFormat) -> Result<(), FileIoError> {
    let line = match fmt {
        ExportFormat::Txt => HEADERS.join("\t"),
        ExportFormat::Csv => HEADERS
            .iter()
            .map(|h| csv_escape(h))
            .collect::<Vec<_>>()
            .join(","),
    };
    writeln!(w, "{line}")?;
    Ok(())
}

fn write_row<W: Write>(
    w: &mut W,
    r: &WalletResultRow,
    fmt: ExportFormat,
) -> Result<(), FileIoError> {
    let input_type = match r.input_type {
        InputType::Address => "address",
        InputType::Seed => "seed",
        InputType::PrivateKey => "private_key",
    };
    let has_funds = if r.has_funds { "1" } else { "0" };
    let fields = [
        r.chain_id.as_str(),
        r.address.as_str(),
        input_type,
        has_funds,
        r.balance_display.as_deref().unwrap_or(""),
        r.staked_display.as_deref().unwrap_or(""),
        r.rewards_display.as_deref().unwrap_or(""),
        r.unbonding_display.as_deref().unwrap_or(""),
        r.error.as_deref().unwrap_or(""),
        r.checked_at.as_str(),
    ];
    let line = match fmt {
        ExportFormat::Txt => fields
            .iter()
            .map(|s| sanitize_txt_field(s))
            .collect::<Vec<_>>()
            .join("\t"),
        ExportFormat::Csv => fields
            .iter()
            .map(|s| csv_escape(s))
            .collect::<Vec<_>>()
            .join(","),
    };
    writeln!(w, "{line}")?;
    Ok(())
}

/// TAB-separated txt плохо дружит с TAB/newline внутри значений —
/// заменяем их на пробел.
fn sanitize_txt_field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// CSV escape по RFC 4180: экранируем всегда (упрощает reader'ы).
fn csv_escape(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(
        session_id: i64,
        chain: &str,
        addr: &str,
        has_funds: bool,
        it: InputType,
    ) -> WalletResultRow {
        WalletResultRow {
            id: 1,
            session_id,
            address: addr.into(),
            chain_id: chain.into(),
            input_type: it,
            balance_raw: Some(json!([{"denom":"uatom","amount":"1500000"}])),
            balance_display: Some("1.5 ATOM".into()),
            staked_raw: None,
            staked_display: None,
            rewards_raw: None,
            rewards_display: None,
            unbonding_raw: None,
            unbonding_display: None,
            has_funds,
            error: None,
            checked_at: "2026-04-17T10:00:00".into(),
        }
    }

    #[test]
    fn export_txt_has_header() {
        let rows = vec![row(
            1,
            "cosmoshub-4",
            "cosmos1xyz",
            true,
            InputType::Address,
        )];
        let mut buf = Vec::new();
        let n =
            export_to_writer(&mut buf, &rows, &ExportFilter::default(), ExportFormat::Txt).unwrap();
        assert_eq!(n, 1);
        let s = String::from_utf8(buf).unwrap();
        let mut lines = s.lines();
        let header = lines.next().unwrap();
        assert_eq!(header.split('\t').count(), HEADERS.len());
        assert!(header.starts_with("chain_id\t"));
        let data = lines.next().unwrap();
        assert!(data.contains("cosmos1xyz"));
        assert!(data.contains("1.5 ATOM"));
    }

    #[test]
    fn only_with_funds_filters_out() {
        let rows = vec![
            row(1, "c", "a1", true, InputType::Address),
            row(1, "c", "a2", false, InputType::Address),
        ];
        let mut buf = Vec::new();
        let filter = ExportFilter {
            only_with_funds: true,
            ..Default::default()
        };
        let n = export_to_writer(&mut buf, &rows, &filter, ExportFormat::Txt).unwrap();
        assert_eq!(n, 1);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("a1"));
        assert!(!s.contains("a2"));
    }

    #[test]
    fn chain_filter_exact_match() {
        let rows = vec![
            row(1, "cosmoshub-4", "a1", true, InputType::Address),
            row(1, "osmosis-1", "a2", true, InputType::Address),
        ];
        let filter = ExportFilter {
            chain_id: Some("osmosis-1".into()),
            ..Default::default()
        };
        let mut buf = Vec::new();
        let n = export_to_writer(&mut buf, &rows, &filter, ExportFormat::Txt).unwrap();
        assert_eq!(n, 1);
        assert!(String::from_utf8(buf).unwrap().contains("a2"));
    }

    #[test]
    fn csv_escapes_embedded_quotes_and_commas() {
        let mut r = row(1, "c", "a", true, InputType::Address);
        r.error = Some(r#"weird "value", with comma"#.into());
        let mut buf = Vec::new();
        export_to_writer(
            &mut buf,
            std::slice::from_ref(&r),
            &ExportFilter::default(),
            ExportFormat::Csv,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        // CSV-строка должна содержать экранированные кавычки (двойные "").
        // Оригинал: weird "value", with comma
        // Ожидаем: "weird ""value"", with comma"
        let expected = "\"weird \"\"value\"\", with comma\"";
        assert!(
            s.contains(expected),
            "csv output does not contain escaped field; got:\n{s}"
        );
    }

    #[test]
    fn txt_replaces_tabs_in_fields() {
        let mut r = row(1, "c", "addr\twith\ttabs", true, InputType::Address);
        r.checked_at = "line1\nline2".into();
        let mut buf = Vec::new();
        export_to_writer(
            &mut buf,
            std::slice::from_ref(&r),
            &ExportFilter::default(),
            ExportFormat::Txt,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        let data = s.lines().nth(1).unwrap();
        // Ни таб внутри поля, ни перенос строки не ломают структуру.
        assert_eq!(data.split('\t').count(), HEADERS.len());
        assert!(!data.contains('\n'));
    }
}
