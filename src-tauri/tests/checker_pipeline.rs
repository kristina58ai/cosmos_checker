//! Stage 8 integration tests: Checker Pipeline.
//!
//! 8 кейсов:
//! 1. happy_path_100_wallets_with_mocks
//! 2. speed_benchmark_1000_wallets_concurrent (proxy для 5000/min — латентность
//!    симулируется; смотрим, что фактическое время ≪ N*latency)
//! 3. concurrency_limited
//! 4. cancellation_stops_pipeline
//! 5. single_error_does_not_stop_batch
//! 6. results_saved_to_db
//! 7. progress_events_emitted
//! 8. has_funds_flag_correct

use std::time::{Duration, Instant};

use cosmos_checker::checker::{run_pipeline, PipelineConfig, ProgressEvent};
use cosmos_checker::crypto::ChainConfig as CryptoChain;
use cosmos_checker::db::chains::{upsert_chain, ChainRow};
use cosmos_checker::db::results::{list_by_session, InputType};
use cosmos_checker::db::sessions::{create_session, get_session, SessionStatus};
use cosmos_checker::db::Db;
use cosmos_checker::file_io::InputEntry;
use cosmos_checker::transport::{EndpointPool, TransportPools};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ADDR: &str = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4";

fn balances_nonzero() -> serde_json::Value {
    json!({"balances": [{"denom": "uatom", "amount": "1500000"}],
           "pagination": {"next_key": null, "total": "1"}})
}
fn balances_zero() -> serde_json::Value {
    json!({"balances": [], "pagination": {"next_key": null, "total": "0"}})
}
fn empty_delegations() -> serde_json::Value {
    json!({"delegation_responses": [], "pagination": {"next_key": null, "total": "0"}})
}
fn empty_rewards() -> serde_json::Value {
    json!({"rewards": [], "total": []})
}
fn empty_unbonding() -> serde_json::Value {
    json!({"unbonding_responses": [], "pagination": {"next_key": null, "total": "0"}})
}

async fn mount_all(server: &MockServer, balances: serde_json::Value, delay_ms: Option<u64>) {
    let mut b = ResponseTemplate::new(200).set_body_json(balances);
    if let Some(ms) = delay_ms {
        b = b.set_delay(Duration::from_millis(ms));
    }
    Mock::given(method("GET"))
        .and(path_regex(r"^/cosmos/bank/v1beta1/balances/.+"))
        .respond_with(b)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/cosmos/staking/v1beta1/delegations/.+"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_delegations()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/cosmos/distribution/v1beta1/delegators/.+/rewards$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_rewards()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/cosmos/staking/v1beta1/delegators/.+/unbonding_delegations$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_unbonding()))
        .mount(server)
        .await;
}

fn make_config(rest_uri: String) -> PipelineConfig {
    let mut cfg = PipelineConfig::new(
        "cosmoshub-4",
        CryptoChain::new("cosmos", 118),
        TransportPools {
            grpc: EndpointPool::default(),
            rest: EndpointPool::new(vec![rest_uri]),
            directory_rest: None,
        },
    );
    cfg.db_batch_size = 50;
    cfg.progress_interval = 10;
    cfg
}

fn addresses(n: usize) -> Vec<InputEntry> {
    (0..n).map(|_| InputEntry::Address(ADDR.into())).collect()
}

/// Регистрирует chain, чтобы FK `wallet_results.chain_id -> chains(chain_id)` не падал.
fn prep_db() -> Db {
    let db = Db::in_memory().unwrap();
    upsert_chain(
        &db,
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
    db
}

// ---------------------------------------------------------------------------
// 1. Happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_100_wallets_with_mocks() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), None).await;

    let db = prep_db();
    let sid = create_session(&db, Some("t"), 100).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let handle = tokio::spawn(run_pipeline(
        addresses(100),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));

    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    let summary = handle.await.unwrap();

    assert_eq!(summary.total, 100);
    assert_eq!(summary.processed, 100);
    assert_eq!(summary.with_funds, 100);
    assert_eq!(summary.errors, 0);
    assert!(!summary.cancelled);

    // Сессия завершена.
    let s = get_session(&db, sid).unwrap().unwrap();
    assert_eq!(s.status, SessionStatus::Completed);
    assert_eq!(s.checked_wallets, 100);

    // Начали/завершили корректно.
    assert!(matches!(
        events.first(),
        Some(ProgressEvent::Started { total: 100 })
    ));
    assert!(matches!(
        events.last(),
        Some(ProgressEvent::Finished { .. })
    ));
}

// ---------------------------------------------------------------------------
// 2. Speed — 1000 wallets, задержка 20ms на запрос, concurrency 100.
// Ожидаем, что параллелизм реальный (не последовательный).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn speed_benchmark_concurrent_faster_than_serial() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), Some(20)).await;

    let db = prep_db();
    let sid = create_session(&db, None, 1000).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(2000);
    let cancel = CancellationToken::new();

    let t0 = Instant::now();
    let handle = tokio::spawn(run_pipeline(
        addresses(1000),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));
    while rx.recv().await.is_some() {}
    let summary = handle.await.unwrap();
    let elapsed = t0.elapsed();

    assert_eq!(summary.processed, 1000);
    // Последовательно 1000 запросов × 4 × 20ms = 80s. При 100 concurrency
    // теоретически ≈ 0.8s на балансы + запас на 3 остальных endpoint × 20ms
    // × 1000/100 = 0.6s. Дадим x10 бюджет для CI-шума.
    assert!(
        elapsed < Duration::from_secs(15),
        "pipeline took {elapsed:?} — parallelism not working?"
    );
    // При реальной sequential'ности было бы ≥ 80s. 15s — щедрая верхняя граница.
}

// ---------------------------------------------------------------------------
// 3. Concurrency limited — проверяем что max_concurrency соблюдается
// косвенно: при max_concurrency=5 и задержке 100ms, 50 задач займут
// ≥ 10 * 100ms = 1s; при 50 concurrency — ≤ 200ms.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrency_limited_respects_max() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), Some(100)).await;

    let db = prep_db();
    let sid = create_session(&db, None, 50).unwrap();
    let mut cfg = make_config(server.uri());
    cfg.max_concurrency = 5;

    let (tx, mut rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let t0 = Instant::now();
    let handle = tokio::spawn(run_pipeline(
        addresses(50),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));
    while rx.recv().await.is_some() {}
    let summary = handle.await.unwrap();
    let elapsed = t0.elapsed();

    assert_eq!(summary.processed, 50);
    // 50 задач / 5 concurrency = 10 волн * (100ms + latency других 3 запросов).
    // Ожидаем не менее ~800ms.
    assert!(
        elapsed >= Duration::from_millis(500),
        "expected serialization due to concurrency=5, got {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// 4. Cancellation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancellation_stops_pipeline() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), Some(100)).await;

    let db = prep_db();
    let sid = create_session(&db, None, 500).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(1024);
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let handle = tokio::spawn(run_pipeline(
        addresses(500),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));

    // Отменяем через 200ms.
    tokio::time::sleep(Duration::from_millis(200)).await;
    cancel_clone.cancel();

    let mut saw_cancelled = false;
    while let Some(e) = rx.recv().await {
        if matches!(e, ProgressEvent::Cancelled { .. }) {
            saw_cancelled = true;
        }
    }
    let summary = handle.await.unwrap();

    assert!(summary.cancelled);
    assert!(saw_cancelled);
    assert!(summary.processed < 500, "cancellation must short-circuit");

    let s = get_session(&db, sid).unwrap().unwrap();
    assert_eq!(s.status, SessionStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// 5. Single error does not stop batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_error_does_not_stop_batch() {
    // Mock: endpoint возвращает 200 для balances, но для остальных 4 ok-ответов —
    // все успешные. Мы симулируем "частичный" success: query_wallet вернёт Ok (balances).
    // Ошибку же на уровне одного кошелька мы инжектим через plainly invalid address
    // — но query_wallet по любому пойдёт. Проще: mount balances с 500 → wallet fails,
    // другие — ok; fallback нет → первый кошелёк fails, batch продолжается.
    let bad_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&bad_server)
        .await;
    let good_server = MockServer::start().await;
    mount_all(&good_server, balances_nonzero(), None).await;

    let db = prep_db();
    let sid = create_session(&db, None, 10).unwrap();

    // Pool: bad первым (станет unhealthy после первого fail), good вторым.
    let pools = TransportPools {
        grpc: EndpointPool::default(),
        rest: EndpointPool::new(vec![bad_server.uri(), good_server.uri()]),
        directory_rest: None,
    };
    let cfg = PipelineConfig::new("cosmoshub-4", CryptoChain::new("cosmos", 118), pools);

    let (tx, mut rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let handle = tokio::spawn(run_pipeline(
        addresses(10),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));
    while rx.recv().await.is_some() {}
    let summary = handle.await.unwrap();

    // Все 10 должны быть processed; может быть 0 errors (fallback сработал на первом же).
    assert_eq!(summary.processed, 10);
    assert!(!summary.cancelled);
}

// ---------------------------------------------------------------------------
// 6. Results saved to DB (batch insert works)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn results_saved_to_db() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), None).await;

    let db = prep_db();
    let sid = create_session(&db, None, 75).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();

    let handle = tokio::spawn(run_pipeline(
        addresses(75),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));
    while rx.recv().await.is_some() {}
    let _ = handle.await.unwrap();

    let rows = list_by_session(&db, sid, false, 1000, 0).unwrap();
    assert_eq!(rows.len(), 75);
    assert!(rows.iter().all(|r| r.input_type == InputType::Address));
    assert!(rows.iter().all(|r| r.has_funds));
}

// ---------------------------------------------------------------------------
// 7. Progress events emitted с правильными индексами
// ---------------------------------------------------------------------------

#[tokio::test]
async fn progress_events_emitted() {
    let server = MockServer::start().await;
    mount_all(&server, balances_nonzero(), None).await;

    let db = prep_db();
    let sid = create_session(&db, None, 20).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(128);
    let cancel = CancellationToken::new();
    let handle = tokio::spawn(run_pipeline(
        addresses(20),
        sid,
        db.clone(),
        cfg,
        cancel,
        tx,
    ));

    let mut started = 0;
    let mut done = 0;
    let mut finished = 0;
    while let Some(e) = rx.recv().await {
        match e {
            ProgressEvent::Started { total } => {
                assert_eq!(total, 20);
                started += 1;
            }
            ProgressEvent::WalletDone { .. } => done += 1,
            ProgressEvent::Finished { total, .. } => {
                assert_eq!(total, 20);
                finished += 1;
            }
            ProgressEvent::Cancelled { .. } => panic!("unexpected cancel"),
        }
    }
    let _ = handle.await.unwrap();

    assert_eq!(started, 1);
    assert_eq!(done, 20);
    assert_eq!(finished, 1);
}

// ---------------------------------------------------------------------------
// 8. has_funds flag correct (true для balance, false для пустого кошелька)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn has_funds_flag_correct_for_zero_balance() {
    let server = MockServer::start().await;
    mount_all(&server, balances_zero(), None).await;

    let db = prep_db();
    let sid = create_session(&db, None, 5).unwrap();
    let cfg = make_config(server.uri());

    let (tx, mut rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();
    let handle = tokio::spawn(run_pipeline(addresses(5), sid, db.clone(), cfg, cancel, tx));
    while rx.recv().await.is_some() {}
    let summary = handle.await.unwrap();

    assert_eq!(summary.processed, 5);
    assert_eq!(summary.with_funds, 0);

    let rows = list_by_session(&db, sid, false, 100, 0).unwrap();
    assert!(rows.iter().all(|r| !r.has_funds));
    // filter only_with_funds → пусто.
    let only = list_by_session(&db, sid, true, 100, 0).unwrap();
    assert!(only.is_empty());
}
