//! Checker Pipeline — оркестратор проверки кошельков.
//!
//! Этапы на один input:
//! 1. **derive** — для Seed/PrivateKey вычисляем bech32-адрес (`crypto::derive_address`).
//!    Для готового адреса — passthrough. Секреты остаются внутри `SecretBox`
//!    и обнуляются при Drop.
//! 2. **query** — `transport::query_wallet` со всей fallback-цепочкой.
//! 3. **aggregate** — упаковываем `WalletData` в `NewWalletResult` (JSON поля
//!    в SQLite + human-friendly display).
//! 4. **persist** — батч-вставка в `wallet_results` (commit каждые N результатов).
//! 5. **progress** — после каждого кошелька в `mpsc::Sender<ProgressEvent>`.
//!
//! Concurrency:
//! - `tokio::Semaphore(max_concurrency)` — одновременно ≤ N активных задач.
//! - `CancellationToken` — кооперативная отмена (проверяется перед и внутри
//!   каждой задачи; активные завершают текущий запрос и выходят).
//! - Одна ошибка не валит батч — ошибка сохраняется в `wallet_results.error`.
//!
//! Производительность:
//! - При 100 concurrency и REST-latency ~1с на кошелёк теоретический предел
//!   6000 wallets/min (цель CLAUDE.md — ≥5000/min).
//! - `insert_results_batch` группирует DB-запись → амортизирует fsync.

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{mpsc, Mutex, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::crypto::{derive_address, ChainConfig as CryptoChain, KeyError, WalletInput};
use crate::db::results::{insert_results_batch, InputType, NewWalletResult};
use crate::db::sessions::{finish_session, update_progress, SessionStatus};
use crate::db::Db;
use crate::file_io::InputEntry;
use crate::transport::{query_wallet, TransportError, TransportPools, WalletData};

/// Конфигурация пайплайна.
#[derive(Clone)]
pub struct PipelineConfig {
    pub chain_id: String,
    pub crypto_chain: CryptoChain,
    pub transport: TransportPools,
    /// Лимит параллельных задач. По CLAUDE.md — 100.
    pub max_concurrency: usize,
    /// Размер батча вставки в SQLite.
    pub db_batch_size: usize,
    /// Периодический update_progress в сессии.
    pub progress_interval: usize,
}

impl PipelineConfig {
    pub fn new(
        chain_id: impl Into<String>,
        crypto: CryptoChain,
        transport: TransportPools,
    ) -> Self {
        Self {
            chain_id: chain_id.into(),
            crypto_chain: crypto,
            transport,
            max_concurrency: 100,
            db_batch_size: 50,
            progress_interval: 25,
        }
    }
}

/// События прогресса. Эмитятся в Tauri через `AppHandle::emit` в слое commands
/// (Stage 9); внутри pipeline мы публикуем их в `mpsc::Sender`, что позволяет
/// легко тестировать без Tauri-рантайма.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// Проверка стартовала (N кошельков всего).
    Started { total: usize },
    /// Один кошелёк обработан (успешно или с ошибкой).
    WalletDone {
        index: usize,
        address: String,
        has_funds: bool,
        error: Option<String>,
    },
    /// Весь батч закончен.
    Finished {
        total: usize,
        with_funds: usize,
        errors: usize,
    },
    /// Проверка отменена пользователем (через [`CancellationToken`]).
    Cancelled { processed: usize },
}

/// Итог работы пайплайна.
#[derive(Debug, Clone, Default)]
pub struct PipelineSummary {
    pub total: usize,
    pub processed: usize,
    pub with_funds: usize,
    pub errors: usize,
    pub cancelled: bool,
}

/// Основная точка входа. Блокирует текущую async-задачу до завершения /
/// отмены. Безопасно переживает одиночные ошибки (они пишутся в
/// `wallet_results.error` и в `ProgressEvent::WalletDone::error`).
pub async fn run_pipeline(
    inputs: Vec<InputEntry>,
    session_id: i64,
    db: Db,
    cfg: PipelineConfig,
    cancel: CancellationToken,
    progress: mpsc::Sender<ProgressEvent>,
) -> PipelineSummary {
    let total = inputs.len();
    let _ = progress.send(ProgressEvent::Started { total }).await;

    let sem = Arc::new(Semaphore::new(cfg.max_concurrency.max(1)));
    let buffer: Arc<Mutex<Vec<NewWalletResult>>> = Arc::new(Mutex::new(Vec::new()));
    let summary = Arc::new(Mutex::new(PipelineSummary {
        total,
        ..Default::default()
    }));

    let cfg = Arc::new(cfg);
    let db_h = db.clone();
    let mut join = JoinSet::new();

    for (idx, entry) in inputs.into_iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let permit_sem = Arc::clone(&sem);
        let cfg = Arc::clone(&cfg);
        let buf = Arc::clone(&buffer);
        let summary = Arc::clone(&summary);
        let progress = progress.clone();
        let db = db_h.clone();
        let cancel = cancel.clone();

        join.spawn(async move {
            // Берём permit (если все заняты — ждём).
            let _permit = match permit_sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return, // Semaphore closed — пайплайн глобально завершается.
            };
            if cancel.is_cancelled() {
                return;
            }

            let (address, input_type, derive_err) = classify_and_derive(&entry, &cfg.crypto_chain);
            let (data, query_err) = if derive_err.is_some() {
                (WalletData::default(), None)
            } else {
                match query_wallet(&address, &cfg.transport).await {
                    Ok(d) => (d, None),
                    Err(e) => (WalletData::default(), Some(format_transport_err(&e))),
                }
            };
            let error = derive_err.or(query_err);
            let has_funds = error.is_none() && data.has_funds();

            let row = build_result_row(
                session_id,
                &cfg.chain_id,
                &address,
                input_type,
                &data,
                error.clone(),
            );

            // Буферизуем и, если накопили batch_size — сбрасываем в БД через
            // spawn_blocking (rusqlite/r2d2 — блокирующие).
            let mut b = buf.lock().await;
            b.push(row);
            let flush_now = b.len() >= cfg.db_batch_size;
            if flush_now {
                let to_flush = std::mem::take(&mut *b);
                drop(b);
                let db2 = db.clone();
                let res =
                    tokio::task::spawn_blocking(move || insert_results_batch(&db2, &to_flush))
                        .await;
                match res {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        tracing::error!(session_id, "batch insert failed: {e}");
                    }
                    Err(e) => {
                        tracing::error!(session_id, "spawn_blocking join error: {e}");
                    }
                }
            } else {
                drop(b);
            }

            // Обновляем summary + прогресс.
            {
                let mut s = summary.lock().await;
                s.processed += 1;
                if has_funds {
                    s.with_funds += 1;
                }
                if error.is_some() {
                    s.errors += 1;
                }
                let processed = s.processed;
                drop(s);

                if processed % cfg.progress_interval == 0 || processed == total {
                    let db2 = db.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        update_progress(&db2, session_id, processed as i64)
                    })
                    .await;
                }
            }

            let _ = progress
                .send(ProgressEvent::WalletDone {
                    index: idx,
                    address,
                    has_funds,
                    error,
                })
                .await;
        });
    }

    // Ждём завершения — либо cancel, либо все таски отработали.
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                // Остановим новые spawn'ы; уже стартовавшие сами увидят cancel
                // в начале и/или завершат текущий запрос.
                break;
            }
            res = join.join_next() => {
                match res {
                    Some(Ok(())) => {}
                    Some(Err(e)) => {
                        tracing::warn!("worker join error: {e}");
                    }
                    None => break,
                }
            }
        }
    }

    // Даём активным задачам короткое окно на graceful-выход при cancel.
    if cancel.is_cancelled() {
        let _ = tokio::time::timeout(Duration::from_millis(500), async {
            while join.join_next().await.is_some() {}
        })
        .await;
        join.shutdown().await;
    }

    // Flush оставшегося буфера.
    let tail = {
        let mut b = buffer.lock().await;
        std::mem::take(&mut *b)
    };
    if !tail.is_empty() {
        let db2 = db.clone();
        let res = tokio::task::spawn_blocking(move || insert_results_batch(&db2, &tail)).await;
        match res {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::error!(session_id, "final batch insert failed: {e}"),
            Err(e) => tracing::error!(session_id, "final spawn_blocking join error: {e}"),
        }
    }

    let final_summary = {
        let mut s = summary.lock().await;
        s.cancelled = cancel.is_cancelled();
        s.clone()
    };

    // Финальный апдейт сессии.
    let status = if final_summary.cancelled {
        SessionStatus::Cancelled
    } else {
        SessionStatus::Completed
    };
    let processed = final_summary.processed as i64;
    let db2 = db.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _ = update_progress(&db2, session_id, processed);
        finish_session(&db2, session_id, status)
    })
    .await;

    if final_summary.cancelled {
        let _ = progress
            .send(ProgressEvent::Cancelled {
                processed: final_summary.processed,
            })
            .await;
    } else {
        let _ = progress
            .send(ProgressEvent::Finished {
                total: final_summary.total,
                with_funds: final_summary.with_funds,
                errors: final_summary.errors,
            })
            .await;
    }

    final_summary
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn classify_and_derive(
    entry: &InputEntry,
    chain: &CryptoChain,
) -> (String, InputType, Option<String>) {
    match entry {
        InputEntry::Address(a) => (a.clone(), InputType::Address, None),
        InputEntry::Seed12(s) | InputEntry::Seed24(s) => {
            // Клонируем SecretString — он Clone-safe через Arc внутри.
            let wi = WalletInput::Seed(SecretString::from(
                secrecy::ExposeSecret::expose_secret(s).to_string(),
            ));
            match derive_address(&wi, chain) {
                Ok(a) => (a, InputType::Seed, None),
                Err(e) => (String::new(), InputType::Seed, Some(format_key_err(&e))),
            }
        }
        InputEntry::PrivateKeyHex(pk) => {
            let bytes: Vec<u8> = secrecy::ExposeSecret::expose_secret(pk).to_vec();
            let sb = crate::security::secret_bytes(bytes);
            let wi = WalletInput::PrivateKey(sb);
            match derive_address(&wi, chain) {
                Ok(a) => (a, InputType::PrivateKey, None),
                Err(e) => (
                    String::new(),
                    InputType::PrivateKey,
                    Some(format_key_err(&e)),
                ),
            }
        }
    }
}

fn build_result_row(
    session_id: i64,
    chain_id: &str,
    address: &str,
    input_type: InputType,
    data: &WalletData,
    error: Option<String>,
) -> NewWalletResult {
    let balance_raw = if data.balances.is_empty() {
        None
    } else {
        serde_json::to_value(&data.balances).ok()
    };
    let staked_raw = if data.delegations.is_empty() {
        None
    } else {
        serde_json::to_value(&data.delegations).ok()
    };
    let rewards_raw = if data.rewards.total.is_empty() && data.rewards.per_validator.is_empty() {
        None
    } else {
        serde_json::to_value(&data.rewards).ok()
    };
    let unbonding_raw = if data.unbonding.is_empty() {
        None
    } else {
        serde_json::to_value(&data.unbonding).ok()
    };

    NewWalletResult {
        session_id,
        address: address.to_owned(),
        chain_id: chain_id.to_owned(),
        input_type,
        balance_raw,
        balance_display: display_coins_int(&data.balances),
        staked_raw,
        staked_display: display_delegations(&data.delegations),
        rewards_raw,
        rewards_display: display_rewards(&data.rewards),
        unbonding_raw,
        unbonding_display: display_unbonding(&data.unbonding),
        has_funds: error.is_none() && data.has_funds(),
        error,
    }
}

fn display_coins_int(coins: &[crate::transport::Coin]) -> Option<String> {
    if coins.is_empty() {
        None
    } else {
        Some(
            coins
                .iter()
                .map(|c| format!("{} {}", c.amount, c.denom))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn display_delegations(d: &[crate::transport::Delegation]) -> Option<String> {
    if d.is_empty() {
        None
    } else {
        Some(
            d.iter()
                .map(|x| format!("{} {}", x.balance.amount, x.balance.denom))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn display_rewards(r: &crate::transport::Rewards) -> Option<String> {
    if r.total.is_empty() {
        None
    } else {
        Some(
            r.total
                .iter()
                .map(|c| format!("{} {}", c.amount, c.denom))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn display_unbonding(u: &[crate::transport::UnbondingDelegation]) -> Option<String> {
    if u.is_empty() {
        None
    } else {
        let total_entries: usize = u.iter().map(|x| x.entries.len()).sum();
        Some(format!("{total_entries} entries"))
    }
}

fn format_key_err(e: &KeyError) -> String {
    format!("derive: {e}")
}

fn format_transport_err(e: &TransportError) -> String {
    format!("query: {e}")
}

// ---------------------------------------------------------------------------
// Unit tests — чистые helpers (без сети, без БД).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Coin, Delegation};

    #[test]
    fn build_row_has_funds_when_balance_nonzero() {
        let data = WalletData {
            balances: vec![Coin {
                denom: "uatom".into(),
                amount: "1500000".into(),
            }],
            ..Default::default()
        };
        let row = build_result_row(1, "c", "cosmos1x", InputType::Address, &data, None);
        assert!(row.has_funds);
        assert!(row.balance_display.as_ref().unwrap().contains("1500000"));
    }

    #[test]
    fn build_row_no_funds_when_error_even_if_data_present() {
        let data = WalletData {
            balances: vec![Coin {
                denom: "uatom".into(),
                amount: "100".into(),
            }],
            ..Default::default()
        };
        let row = build_result_row(
            1,
            "c",
            "cosmos1x",
            InputType::Address,
            &data,
            Some("boom".into()),
        );
        assert!(!row.has_funds);
        assert_eq!(row.error.as_deref(), Some("boom"));
    }

    #[test]
    fn build_row_delegations_displayed() {
        let data = WalletData {
            delegations: vec![Delegation {
                validator_address: "cosmosvaloper1".into(),
                balance: Coin {
                    denom: "uatom".into(),
                    amount: "42".into(),
                },
            }],
            ..Default::default()
        };
        let row = build_result_row(1, "c", "cosmos1x", InputType::Address, &data, None);
        assert!(row.staked_display.as_ref().unwrap().contains("42"));
    }
}
