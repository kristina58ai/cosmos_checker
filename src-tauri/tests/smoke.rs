//! Stage 1 smoke test.
//!
//! Проверяет, что:
//! - library crate компилируется и линкуется,
//! - команда `ping` возвращает `"pong"` при прямом вызове.
//!
//! Полноценный тест Tauri-runtime'а (через tauri::test::mock_builder) добавим
//! в Этапе 9 вместе с остальными IPC-командами.

use cosmos_checker::commands::ping::ping;

#[test]
fn ping_returns_pong() {
    assert_eq!(ping(), "pong");
}
