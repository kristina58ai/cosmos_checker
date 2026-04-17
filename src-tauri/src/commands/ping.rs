//! `ping` — smoke-команда, используется Этапом 1 для проверки IPC.

/// Возвращает `"pong"`. Используется frontend'ом и integration-тестом.
#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_pong() {
        assert_eq!(ping(), "pong");
    }
}
