//! IPC: чтение результатов проверки + экспорт.
//!
//! Seed/privkey в `wallet_results` не пишутся, поэтому export безопасен
//! по построению — см. CLAUDE.md §5 T1.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::db::results::{count_by_session, list_by_session, InputType, WalletResultRow};
use crate::file_io::exporter::{export_to_file, ExportFilter, ExportFormat};

use super::state::{AppState, CommandResult};

// ---------------------------------------------------------------------------
// Requests / responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetResultsRequest {
    pub session_id: i64,
    #[serde(default)]
    pub only_with_funds: bool,
    /// LIMIT для SQL. 0/None → 1000 (разумный максимум за вызов).
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetResultsResponse {
    pub rows: Vec<WalletResultRow>,
    pub total: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormatDto {
    Txt,
    Csv,
}

impl From<ExportFormatDto> for ExportFormat {
    fn from(f: ExportFormatDto) -> Self {
        match f {
            ExportFormatDto::Txt => ExportFormat::Txt,
            ExportFormatDto::Csv => ExportFormat::Csv,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportRequest {
    pub session_id: i64,
    pub path: String,
    pub format: ExportFormatDto,
    #[serde(default)]
    pub only_with_funds: bool,
    pub chain_id: Option<String>,
    pub input_type: Option<InputType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportResponse {
    pub written: usize,
    pub path: String,
}

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

const MAX_PAGE: i64 = 1000;

pub fn get_results_inner(
    state: &AppState,
    req: &GetResultsRequest,
) -> CommandResult<GetResultsResponse> {
    let limit = req.limit.unwrap_or(MAX_PAGE).clamp(1, MAX_PAGE);
    let offset = req.offset.unwrap_or(0).max(0);
    let rows = list_by_session(
        &state.db,
        req.session_id,
        req.only_with_funds,
        limit,
        offset,
    )?;
    let total = count_by_session(&state.db, req.session_id)?;
    Ok(GetResultsResponse { rows, total })
}

pub fn export_results_inner(
    state: &AppState,
    req: &ExportRequest,
) -> CommandResult<ExportResponse> {
    // Собираем все строки сессии — экспортируем без пагинации (фильтруется
    // на уровне `ExportFilter`).
    let total = count_by_session(&state.db, req.session_id)?;
    let rows = list_by_session(&state.db, req.session_id, false, total.max(1), 0)?;

    let filter = ExportFilter {
        only_with_funds: req.only_with_funds,
        chain_id: req.chain_id.clone(),
        input_type: req.input_type,
    };

    let path = PathBuf::from(&req.path);
    let written = export_to_file(&path, &rows, &filter, ExportFormat::from(req.format))?;
    Ok(ExportResponse {
        written,
        path: req.path.clone(),
    })
}

// ---------------------------------------------------------------------------
// Tauri wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_results(
    state: tauri::State<'_, AppState>,
    req: GetResultsRequest,
) -> CommandResult<GetResultsResponse> {
    get_results_inner(state.inner(), &req)
}

#[tauri::command]
pub fn export_results(
    state: tauri::State<'_, AppState>,
    req: ExportRequest,
) -> CommandResult<ExportResponse> {
    export_results_inner(state.inner(), &req)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chains::{upsert_chain, ChainRow};
    use crate::db::results::{insert_results_batch, NewWalletResult};
    use crate::db::sessions::create_session;
    use serde_json::json;
    use tempfile::tempdir;

    fn prep(state: &AppState) -> i64 {
        upsert_chain(
            &state.db,
            &ChainRow {
                chain_id: "cosmoshub-4".into(),
                chain_name: "cosmoshub".into(),
                bech32_prefix: "cosmos".into(),
                slip44: 118,
                display_name: None,
                logo_url: None,
            },
        )
        .unwrap();
        create_session(&state.db, Some("t"), 3).unwrap()
    }

    fn row(session_id: i64, addr: &str, has_funds: bool) -> NewWalletResult {
        NewWalletResult {
            session_id,
            address: addr.into(),
            chain_id: "cosmoshub-4".into(),
            input_type: InputType::Address,
            balance_raw: Some(json!([{"denom":"uatom","amount":"1000000"}])),
            balance_display: Some("1 ATOM".into()),
            staked_raw: None,
            staked_display: None,
            rewards_raw: None,
            rewards_display: None,
            unbonding_raw: None,
            unbonding_display: None,
            has_funds,
            error: None,
        }
    }

    #[test]
    fn get_results_paginates_and_filters() {
        let st = AppState::new_in_memory().unwrap();
        let sid = prep(&st);
        insert_results_batch(
            &st.db,
            &[
                row(sid, "a1", true),
                row(sid, "a2", false),
                row(sid, "a3", true),
            ],
        )
        .unwrap();

        let all = get_results_inner(
            &st,
            &GetResultsRequest {
                session_id: sid,
                only_with_funds: false,
                limit: Some(10),
                offset: Some(0),
            },
        )
        .unwrap();
        assert_eq!(all.total, 3);
        assert_eq!(all.rows.len(), 3);

        let funded = get_results_inner(
            &st,
            &GetResultsRequest {
                session_id: sid,
                only_with_funds: true,
                limit: None,
                offset: None,
            },
        )
        .unwrap();
        assert_eq!(funded.rows.len(), 2);

        let paged = get_results_inner(
            &st,
            &GetResultsRequest {
                session_id: sid,
                only_with_funds: false,
                limit: Some(2),
                offset: Some(2),
            },
        )
        .unwrap();
        assert_eq!(paged.rows.len(), 1);
    }

    #[test]
    fn export_writes_file_with_filter() {
        let st = AppState::new_in_memory().unwrap();
        let sid = prep(&st);
        insert_results_batch(&st.db, &[row(sid, "a1", true), row(sid, "a2", false)]).unwrap();

        let dir = tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let resp = export_results_inner(
            &st,
            &ExportRequest {
                session_id: sid,
                path: path.to_string_lossy().into_owned(),
                format: ExportFormatDto::Txt,
                only_with_funds: true,
                chain_id: None,
                input_type: None,
            },
        )
        .unwrap();
        assert_eq!(resp.written, 1);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("a1"));
        assert!(!content.contains("a2"));
    }

    #[test]
    fn export_request_serde_round_trip() {
        let r = ExportRequest {
            session_id: 1,
            path: "/tmp/x.csv".into(),
            format: ExportFormatDto::Csv,
            only_with_funds: true,
            chain_id: Some("cosmoshub-4".into()),
            input_type: Some(InputType::Address),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ExportRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
