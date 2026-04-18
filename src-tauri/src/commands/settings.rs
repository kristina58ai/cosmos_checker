//! IPC: чтение и обновление `app_settings` (простое key/value).

use serde::{Deserialize, Serialize};

use crate::db::settings as db_settings;

use super::state::{AppState, CommandResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

pub fn get_settings_inner(state: &AppState) -> CommandResult<Vec<Setting>> {
    let rows = db_settings::list_all(&state.db)?;
    Ok(rows
        .into_iter()
        .map(|(k, v)| Setting { key: k, value: v })
        .collect())
}

pub fn update_settings_inner(state: &AppState, key: &str, value: &str) -> CommandResult<()> {
    db_settings::set(&state.db, key, value)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Setting>> {
    get_settings_inner(state.inner())
}

#[tauri::command]
pub fn update_settings(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> CommandResult<()> {
    update_settings_inner(state.inner(), &key, &value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_then_get_round_trip() {
        let st = AppState::new_in_memory().unwrap();
        update_settings_inner(&st, "max_concurrency", "64").unwrap();
        let all = get_settings_inner(&st).unwrap();
        let found = all
            .iter()
            .find(|s| s.key == "max_concurrency")
            .expect("must find key we just set");
        assert_eq!(found.value, "64");
    }

    #[test]
    fn get_settings_contains_seeded_defaults() {
        let st = AppState::new_in_memory().unwrap();
        let all = get_settings_inner(&st).unwrap();
        // `Db::in_memory` seed'ит ≥5 дефолтных настроек.
        assert!(all.len() >= 5);
    }

    #[test]
    fn setting_serde_round_trip() {
        let s = Setting {
            key: "k".into(),
            value: "v".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: Setting = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }
}
