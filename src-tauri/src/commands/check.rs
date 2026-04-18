//! IPC: старт / остановка проверки.
//!
//! `start_check` консумирует ранее импортированные wallets по `import_token`,
//! создаёт `check_sessions` row, собирает [`PipelineConfig`] (endpoints из
//! кеша [`crate::chain_registry`]), и спавнит pipeline на `tokio`-таске.
//! Возвращает `session_id` немедленно — дальше фронтенд следит за прогрессом
//! через события `check:progress`.
//!
//! `stop_check` находит `CancellationToken` по `session_id` в [`AppState`]
//! и отменяет задачу. Pipeline кооперативно завершается и пишет финальный
//! статус сессии.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::checker::{run_pipeline, PipelineConfig, ProgressEvent};
use crate::crypto::ChainConfig as CryptoChain;
use crate::db::sessions::{create_session, finish_session, SessionStatus};
use crate::transport::TransportPools;

use super::state::{AppState, CommandError, CommandResult};

// ---------------------------------------------------------------------------
// Requests / responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartCheckRequest {
    pub import_token: i64,
    pub chain_id: String,
    pub name: Option<String>,
    pub max_concurrency: Option<usize>,
    /// Опциональный cosmos.directory fallback base_url
    /// (например `https://rest.cosmos.directory/cosmoshub`).
    pub directory_rest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartCheckResponse {
    pub session_id: i64,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StopCheckResponse {
    pub session_id: i64,
    pub cancelled: bool,
}

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

/// Собирает `TransportPools` + `CryptoChain` из кешированного `ChainInfo`.
fn build_pipeline_config(
    state: &AppState,
    chain_id: &str,
    directory_rest: Option<String>,
    max_concurrency: Option<usize>,
) -> CommandResult<PipelineConfig> {
    let info = state
        .registry
        .list_cached()?
        .into_iter()
        .find(|c| c.chain_id == chain_id)
        .ok_or_else(|| CommandError::NotFound(format!("chain_id={chain_id}")))?;

    let mut grpc = Vec::new();
    let mut rest = Vec::new();
    for ep in &info.endpoints {
        match ep.kind {
            crate::chain_registry::EndpointKind::Grpc => grpc.push(ep.address.clone()),
            crate::chain_registry::EndpointKind::Rest => rest.push(ep.address.clone()),
            crate::chain_registry::EndpointKind::Rpc => {}
        }
    }
    if rest.is_empty() && directory_rest.is_none() {
        return Err(CommandError::Invalid(format!(
            "no REST endpoints for {chain_id} and no directory_rest fallback"
        )));
    }

    let transport = TransportPools::new(grpc, rest, directory_rest);
    let crypto = CryptoChain {
        bech32_prefix: info.bech32_prefix.clone(),
        slip44: info.slip44,
    };

    let mut cfg = PipelineConfig::new(info.chain_id.clone(), crypto, transport);
    if let Some(n) = max_concurrency {
        cfg.max_concurrency = n.max(1);
    }
    Ok(cfg)
}

pub fn start_check_inner(
    state: &AppState,
    req: &StartCheckRequest,
) -> CommandResult<StartCheckResponse> {
    // 1. Сначала проверяем, что такой import_token существует И что сеть есть.
    //    Это важно для UX: при ошибке в chain_id пользователь не теряет импорт.
    {
        let guard = state
            .pending_imports
            .lock()
            .expect("pending_imports mutex poisoned");
        if !guard.contains_key(&req.import_token) {
            return Err(CommandError::NotFound(format!(
                "import_token={}",
                req.import_token
            )));
        }
    }
    let cfg = build_pipeline_config(
        state,
        &req.chain_id,
        req.directory_rest.clone(),
        req.max_concurrency,
    )?;

    // 2. Теперь безопасно извлекаем входы.
    let inputs = {
        let mut guard = state
            .pending_imports
            .lock()
            .expect("pending_imports mutex poisoned");
        guard
            .remove(&req.import_token)
            .expect("token was verified to exist above")
    };
    if inputs.is_empty() {
        return Err(CommandError::Invalid("import has 0 entries".into()));
    }
    let total = inputs.len();

    // 3. Создаём сессию в БД.
    let session_id = create_session(&state.db, req.name.as_deref(), total as i64)?;

    // 4. Регистрируем cancel token.
    let cancel = CancellationToken::new();
    {
        let mut guard = state
            .running_checks
            .lock()
            .expect("running_checks mutex poisoned");
        guard.insert(session_id, cancel.clone());
    }

    // 5. Spawn pipeline. Прогресс сейчас пишем в tracing; слой Tauri
    //    (Stage 10+) перевесит mpsc на `AppHandle::emit("check:progress", …)`.
    let db = state.db.clone();
    let running_checks = Arc::clone(&state.running_checks);
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::channel::<ProgressEvent>(256);
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                tracing::debug!(?ev, "check progress");
            }
        });

        let summary = run_pipeline(inputs, session_id, db.clone(), cfg, cancel, tx).await;
        drop(forwarder);

        let status = if summary.cancelled {
            SessionStatus::Cancelled
        } else if summary.errors > 0 && summary.processed == summary.errors {
            SessionStatus::Error
        } else {
            SessionStatus::Completed
        };
        if let Err(e) = finish_session(&db, session_id, status) {
            tracing::error!(session_id, "finish_session failed: {e}");
        }

        // Убираем токен из реестра активных проверок.
        if let Ok(mut guard) = running_checks.lock() {
            guard.remove(&session_id);
        }
    });

    Ok(StartCheckResponse { session_id, total })
}

pub fn stop_check_inner(state: &AppState, session_id: i64) -> CommandResult<StopCheckResponse> {
    let token_opt = {
        let mut guard = state
            .running_checks
            .lock()
            .expect("running_checks mutex poisoned");
        guard.remove(&session_id)
    };
    match token_opt {
        Some(tok) => {
            tok.cancel();
            Ok(StopCheckResponse {
                session_id,
                cancelled: true,
            })
        }
        None => Err(CommandError::NotFound(format!("session_id={session_id}"))),
    }
}

// ---------------------------------------------------------------------------
// Tauri wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn start_check(
    state: tauri::State<'_, AppState>,
    req: StartCheckRequest,
) -> CommandResult<StartCheckResponse> {
    start_check_inner(state.inner(), &req)
}

#[tauri::command]
pub fn stop_check(
    state: tauri::State<'_, AppState>,
    session_id: i64,
) -> CommandResult<StopCheckResponse> {
    stop_check_inner(state.inner(), session_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chains::{upsert_chain, ChainRow, NewEndpoint};
    use crate::file_io::InputEntry;

    fn seed_chain(state: &AppState, chain_id: &str, rest_endpoints: &[&str]) {
        upsert_chain(
            &state.db,
            &ChainRow {
                chain_id: chain_id.into(),
                chain_name: "cosmoshub".into(),
                bech32_prefix: "cosmos".into(),
                slip44: 118,
                display_name: None,
                logo_url: None,
            },
        )
        .unwrap();
        let eps: Vec<_> = rest_endpoints
            .iter()
            .map(|a| NewEndpoint {
                endpoint_type: "rest".into(),
                address: (*a).into(),
                provider: None,
            })
            .collect();
        crate::db::chains::replace_endpoints(&state.db, chain_id, &eps).unwrap();
    }

    fn stash_address(state: &AppState) -> i64 {
        let token = state.next_import_token();
        let entries = vec![InputEntry::Address(
            "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4".into(),
        )];
        state.pending_imports.lock().unwrap().insert(token, entries);
        token
    }

    #[test]
    fn start_check_errors_on_unknown_token() {
        let st = AppState::new_in_memory().unwrap();
        seed_chain(&st, "cosmoshub-4", &["http://127.0.0.1:1317"]);
        let err = start_check_inner(
            &st,
            &StartCheckRequest {
                import_token: 999,
                chain_id: "cosmoshub-4".into(),
                name: None,
                max_concurrency: None,
                directory_rest: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }

    #[test]
    fn start_check_errors_on_unknown_chain() {
        let st = AppState::new_in_memory().unwrap();
        let token = stash_address(&st);
        let err = start_check_inner(
            &st,
            &StartCheckRequest {
                import_token: token,
                chain_id: "ghost-1".into(),
                name: None,
                max_concurrency: None,
                directory_rest: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
        // token должен остаться на месте — команду не должны считать консуматором
        // при раннем падении.
        assert!(st.pending_imports.lock().unwrap().contains_key(&token));
    }

    #[test]
    fn start_check_errors_when_no_rest_endpoints() {
        let st = AppState::new_in_memory().unwrap();
        seed_chain(&st, "cosmoshub-4", &[]);
        let token = stash_address(&st);
        let err = start_check_inner(
            &st,
            &StartCheckRequest {
                import_token: token,
                chain_id: "cosmoshub-4".into(),
                name: None,
                max_concurrency: None,
                directory_rest: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::Invalid(_)));
    }

    #[test]
    fn stop_check_errors_on_unknown_session() {
        let st = AppState::new_in_memory().unwrap();
        let err = stop_check_inner(&st, 42).unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }

    #[tokio::test]
    async fn start_then_stop_cancels_pipeline() {
        let st = AppState::new_in_memory().unwrap();
        // unreachable URL — запросы будут зависать/падать; Pipeline всё равно
        // стартует, но мы сразу же его отменим.
        seed_chain(&st, "cosmoshub-4", &["http://127.0.0.1:1"]);
        let token = stash_address(&st);
        let started = start_check_inner(
            &st,
            &StartCheckRequest {
                import_token: token,
                chain_id: "cosmoshub-4".into(),
                name: Some("ipc-test".into()),
                max_concurrency: Some(2),
                directory_rest: None,
            },
        )
        .unwrap();
        assert_eq!(started.total, 1);

        // Остановить сразу же — должно пройти без ошибки.
        let stopped = stop_check_inner(&st, started.session_id).unwrap();
        assert!(stopped.cancelled);

        // Подождать, пока spawn'нутая задача завершит finish_session.
        for _ in 0..50 {
            let row = crate::db::sessions::get_session(&st.db, started.session_id)
                .unwrap()
                .unwrap();
            if row.status != SessionStatus::Running {
                return; // готово
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("session never transitioned out of running");
    }

    #[test]
    fn request_serde_round_trip() {
        let r = StartCheckRequest {
            import_token: 1,
            chain_id: "cosmoshub-4".into(),
            name: Some("x".into()),
            max_concurrency: Some(50),
            directory_rest: Some("https://rest.cosmos.directory/cosmoshub".into()),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: StartCheckRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
