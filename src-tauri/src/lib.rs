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

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Инициализация структурного логирования (tracing).
///
/// Вызывается из `main` и из integration-тестов. Повторный вызов игнорируется.
pub fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_target(false))
        .try_init();
}

/// Основная точка входа Tauri-приложения.
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![commands::ping::ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
