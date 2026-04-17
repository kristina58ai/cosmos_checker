//! Stage 7 integration tests: Importer + Exporter end-to-end.

use std::io::Write;

use cosmos_checker::db::results::{InputType, WalletResultRow};
use cosmos_checker::file_io::{export_to_file, import_file, ExportFilter, ExportFormat, InputKind};
use tempfile::NamedTempFile;

const ADDR: &str = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4";
const ABANDON_12: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const ABANDON_24: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

fn write_tmp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

// ---------------------------------------------------------------------------
// Importer (9 кейсов)
// ---------------------------------------------------------------------------

#[test]
fn import_pure_addresses_file() {
    let content = format!("{ADDR}\n{ADDR}\n");
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.entries.len(), 2);
    assert!(r.errors.is_empty());
    assert_eq!(r.counts(), (2, 0, 0, 0));
}

#[test]
fn import_seed_12_file() {
    let content = format!("{ABANDON_12}\n");
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.entries.len(), 1);
    assert_eq!(r.entries[0].kind(), InputKind::Seed12);
}

#[test]
fn import_seed_24_file() {
    let content = format!("{ABANDON_24}\n");
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.entries.len(), 1);
    assert_eq!(r.entries[0].kind(), InputKind::Seed24);
}

#[test]
fn import_privkey_hex_variants() {
    let content = format!("{}\n0x{}\n", "a".repeat(64), "b".repeat(64));
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.entries.len(), 2);
    assert_eq!(r.counts(), (0, 0, 0, 2));
}

#[test]
fn import_empty_file() {
    let f = write_tmp("");
    let r = import_file(f.path()).unwrap();
    assert!(r.is_empty());
    assert!(r.errors.is_empty());
}

#[test]
fn import_blanks_and_comments_skipped() {
    let content = format!("# комментарий\n\n   \n// another\n{ADDR}\n\n");
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.entries.len(), 1);
    assert!(r.errors.is_empty());
}

#[test]
fn import_invalid_lines_collected() {
    let content = "garbage\nnot_a_valid_address\n123\n";
    let f = write_tmp(content);
    let r = import_file(f.path()).unwrap();
    assert!(r.is_empty());
    assert_eq!(r.errors.len(), 3);
    assert!(r.errors.iter().any(|e| e.contains("line 1")));
    assert!(r.errors.iter().any(|e| e.contains("line 3")));
}

#[test]
fn import_mixed_file_all_kinds() {
    let content = format!(
        "# mixed input\n{ADDR}\n\n{ABANDON_12}\n{ABANDON_24}\n{pk}\n",
        pk = "c".repeat(64)
    );
    let f = write_tmp(&content);
    let r = import_file(f.path()).unwrap();
    assert_eq!(r.counts(), (1, 1, 1, 1));
    assert!(r.errors.is_empty());
}

#[test]
fn import_large_file_100k_addresses() {
    // 100k строк — проверяем, что BufReader справляется и производительность разумная.
    let mut content = String::with_capacity(100_000 * (ADDR.len() + 1));
    for _ in 0..100_000 {
        content.push_str(ADDR);
        content.push('\n');
    }
    let f = write_tmp(&content);
    let t0 = std::time::Instant::now();
    let r = import_file(f.path()).unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(r.entries.len(), 100_000);
    assert!(r.errors.is_empty());
    // Sanity: 100k должно укладываться в несколько секунд на любом железе.
    assert!(
        elapsed.as_secs() < 30,
        "import too slow: {elapsed:?} for 100k lines"
    );
}

// ---------------------------------------------------------------------------
// Exporter (4 кейса)
// ---------------------------------------------------------------------------

fn sample_row(chain: &str, addr: &str, has_funds: bool, it: InputType) -> WalletResultRow {
    WalletResultRow {
        id: 0,
        session_id: 1,
        address: addr.into(),
        chain_id: chain.into(),
        input_type: it,
        balance_raw: None,
        balance_display: Some("1.5 ATOM".into()),
        staked_raw: None,
        staked_display: Some("0".into()),
        rewards_raw: None,
        rewards_display: Some("0".into()),
        unbonding_raw: None,
        unbonding_display: Some("0".into()),
        has_funds,
        error: None,
        checked_at: "2026-04-17T10:00:00".into(),
    }
}

#[test]
fn export_txt_full_roundtrip_to_file() {
    let rows = vec![
        sample_row("cosmoshub-4", "cosmos1a", true, InputType::Address),
        sample_row("cosmoshub-4", "cosmos1b", false, InputType::Seed),
    ];
    let f = NamedTempFile::new().unwrap();
    let n = export_to_file(f.path(), &rows, &ExportFilter::default(), ExportFormat::Txt).unwrap();
    assert_eq!(n, 2);

    let content = std::fs::read_to_string(f.path()).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3); // header + 2 rows
    assert!(lines[0].contains("chain_id"));
    assert!(lines[1].contains("cosmos1a"));
    assert!(lines[2].contains("cosmos1b"));
}

#[test]
fn export_filter_only_with_funds() {
    let rows = vec![
        sample_row("c", "a1", true, InputType::Address),
        sample_row("c", "a2", false, InputType::Address),
        sample_row("c", "a3", true, InputType::Address),
    ];
    let f = NamedTempFile::new().unwrap();
    let filter = ExportFilter {
        only_with_funds: true,
        ..Default::default()
    };
    let n = export_to_file(f.path(), &rows, &filter, ExportFormat::Txt).unwrap();
    assert_eq!(n, 2);
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("a1"));
    assert!(!content.contains("a2"));
    assert!(content.contains("a3"));
}

#[test]
fn export_filter_by_chain_and_input_type() {
    let rows = vec![
        sample_row("cosmoshub-4", "a1", true, InputType::Address),
        sample_row("osmosis-1", "a2", true, InputType::Address),
        sample_row("cosmoshub-4", "a3", true, InputType::Seed),
    ];
    let f = NamedTempFile::new().unwrap();
    let filter = ExportFilter {
        chain_id: Some("cosmoshub-4".into()),
        input_type: Some(InputType::Seed),
        ..Default::default()
    };
    let n = export_to_file(f.path(), &rows, &filter, ExportFormat::Txt).unwrap();
    assert_eq!(n, 1);
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("a3"));
    assert!(!content.contains("a1"));
    assert!(!content.contains("a2"));
}

#[test]
fn export_csv_format_parseable() {
    let rows = vec![sample_row("c", "addr", true, InputType::Address)];
    let f = NamedTempFile::new().unwrap();
    export_to_file(f.path(), &rows, &ExportFilter::default(), ExportFormat::Csv).unwrap();
    let content = std::fs::read_to_string(f.path()).unwrap();
    // Все поля экранированы кавычками — простейший sanity.
    assert!(content.starts_with("\"chain_id\","));
    assert!(content.contains("\"addr\""));
    assert!(content.contains("\"1\"")); // has_funds=1
}
