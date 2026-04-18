//! cosmos-checker library entry-point.
//!
//! Публичные подмодули — скелет под этапы плана. На Этапе 1 они пустые /
//! содержат только типы и доки. Реализации добавляются поэтапно.

pub mod chain_registry;
pub mod checker;
pub mod commands;
pub mod crypto;
pub mod db;
pub mod errors;
pub mod file_io;
pub mod proxy;
pub mod security;
pub mod transport;

use std::path::PathBuf;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use chain_registry::Registry;
use commands::AppState;
use db::Db;

/// Инициализация структурного логирования (tracing).
///
/// Вызывается из `main` и из integration-тестов. Повторный вызов игнорируется.
pub fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_target(false))
        .try_init();
}

/// Открывает БД в рабочем каталоге приложения. Fallback — в текущем каталоге.
fn open_db() -> Db {
    let path: PathBuf = std::env::var("COSMOS_CHECKER_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("cosmos-checker.sqlite"));
    Db::open(&path).unwrap_or_else(|e| {
        eprintln!("DB open failed ({e}) — fallback to in-memory");
        Db::in_memory().expect("in-memory db")
    })
}

/// Основная точка входа Tauri-приложения.
pub fn run() {
    init_tracing();

    let db = open_db();
    let registry = Registry::with_defaults(db.clone()).expect("registry init");
    let state = AppState::new(db, registry);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::ping::ping,
            commands::chains::get_chains,
            commands::chains::get_chain_details,
            commands::chains::refresh_chain,
            commands::import::import_wallets,
            commands::import::import_proxies,
            commands::check::start_check,
            commands::check::stop_check,
            commands::results::get_results,
            commands::results::export_results,
            commands::settings::get_settings,
            commands::settings::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
