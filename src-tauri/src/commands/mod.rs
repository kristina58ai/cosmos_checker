//! Tauri IPC commands (Stage 9).
//!
//! Слой `commands` — тонкая обёртка между frontend'ом и backend-модулями
//! (db, chain_registry, file_io, proxy, checker). Каждый модуль реализует:
//! - `*_inner(&AppState, …)` — unit-testable функции без зависимости от Tauri;
//! - `#[tauri::command]` обёртки, которые только распаковывают
//!   `tauri::State<AppState>` и вызывают `*_inner`.
//!
//! Регистрируются в [`crate::run`] через `tauri::generate_handler![...]`.
//!
//! Все команды возвращают [`state::CommandError`] при ошибках — он
//! serde-сериализуемый `{ code, message }` и удобен на стороне JS.

pub mod chains;
pub mod check;
pub mod import;
pub mod ping;
pub mod results;
pub mod settings;
pub mod state;

pub use state::{AppState, CommandError, CommandResult};
