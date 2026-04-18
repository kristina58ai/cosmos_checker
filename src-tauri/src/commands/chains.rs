//! IPC: работа с реестром сетей.
//!
//! - `get_chains` — список сетей из локального кеша (SQLite).
//! - `get_chain_details` — ChainInfo (endpoints + tokens) по `chain_id`.
//! - `refresh_chain` — принудительный fetch из chain-registry (GitHub).

use serde::{Deserialize, Serialize};

use crate::chain_registry::ChainInfo;

use super::state::{AppState, CommandError, CommandResult};

/// Компактное представление для списка сетей (без endpoints/tokens).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainSummary {
    pub chain_id: String,
    pub chain_name: String,
    pub display_name: Option<String>,
    pub bech32_prefix: String,
    pub slip44: u32,
    pub logo_url: Option<String>,
}

impl From<&ChainInfo> for ChainSummary {
    fn from(c: &ChainInfo) -> Self {
        Self {
            chain_id: c.chain_id.clone(),
            chain_name: c.chain_name.clone(),
            display_name: c.pretty_name.clone(),
            bech32_prefix: c.bech32_prefix.clone(),
            slip44: c.slip44,
            logo_url: c.logo_url.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Inner (unit-testable)
// ---------------------------------------------------------------------------

pub fn get_chains_inner(state: &AppState) -> CommandResult<Vec<ChainSummary>> {
    let all = state.registry.list_cached()?;
    Ok(all.iter().map(ChainSummary::from).collect())
}

pub fn get_chain_details_inner(state: &AppState, chain_id: &str) -> CommandResult<ChainInfo> {
    let all = state.registry.list_cached()?;
    all.into_iter()
        .find(|c| c.chain_id == chain_id)
        .ok_or_else(|| CommandError::NotFound(format!("chain_id={chain_id}")))
}

pub async fn refresh_chain_inner(state: &AppState, chain_name: &str) -> CommandResult<ChainInfo> {
    Ok(state.registry.force_refresh(chain_name).await?)
}

// ---------------------------------------------------------------------------
// Tauri command wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_chains(state: tauri::State<'_, AppState>) -> CommandResult<Vec<ChainSummary>> {
    get_chains_inner(state.inner())
}

#[tauri::command]
pub fn get_chain_details(
    state: tauri::State<'_, AppState>,
    chain_id: String,
) -> CommandResult<ChainInfo> {
    get_chain_details_inner(state.inner(), &chain_id)
}

#[tauri::command]
pub async fn refresh_chain(
    state: tauri::State<'_, AppState>,
    chain_name: String,
) -> CommandResult<ChainInfo> {
    refresh_chain_inner(state.inner(), &chain_name).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chains::{upsert_chain, ChainRow};

    fn seed_cosmos(state: &AppState) {
        upsert_chain(
            &state.db,
            &ChainRow {
                chain_id: "cosmoshub-4".into(),
                chain_name: "cosmoshub".into(),
                bech32_prefix: "cosmos".into(),
                slip44: 118,
                display_name: Some("Cosmos Hub".into()),
                logo_url: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn get_chains_empty_on_fresh_db() {
        let st = AppState::new_in_memory().unwrap();
        assert!(get_chains_inner(&st).unwrap().is_empty());
    }

    #[test]
    fn get_chains_lists_cached() {
        let st = AppState::new_in_memory().unwrap();
        seed_cosmos(&st);
        let chains = get_chains_inner(&st).unwrap();
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].chain_id, "cosmoshub-4");
        assert_eq!(chains[0].display_name.as_deref(), Some("Cosmos Hub"));
    }

    #[test]
    fn get_chain_details_not_found() {
        let st = AppState::new_in_memory().unwrap();
        let err = get_chain_details_inner(&st, "missing").unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }

    #[test]
    fn get_chain_details_returns_info() {
        let st = AppState::new_in_memory().unwrap();
        seed_cosmos(&st);
        let info = get_chain_details_inner(&st, "cosmoshub-4").unwrap();
        assert_eq!(info.bech32_prefix, "cosmos");
        assert_eq!(info.slip44, 118);
    }
}
