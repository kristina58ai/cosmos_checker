//! Stage 5 integration tests: REST-клиент, fallback и round-robin поверх wiremock.

use std::time::{Duration, Instant};

use cosmos_checker::transport::{
    query_wallet, EndpointPool, RestClient, TransportError, TransportPools,
};
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ADDR: &str = "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4";

fn balances_body() -> serde_json::Value {
    json!({"balances": [{"denom": "uatom", "amount": "1500000"}],
           "pagination": {"next_key": null, "total": "1"}})
}

fn delegations_body() -> serde_json::Value {
    json!({"delegation_responses": [{
        "delegation": {
            "delegator_address": ADDR,
            "validator_address": "cosmosvaloper1abc",
            "shares": "10000000.000000000000000000"
        },
        "balance": {"denom": "uatom", "amount": "10000000"}
    }], "pagination": {"next_key": null, "total": "1"}})
}

fn rewards_body() -> serde_json::Value {
    json!({
        "rewards": [{
            "validator_address": "cosmosvaloper1abc",
            "reward": [{"denom": "uatom", "amount": "300.500000000000000000"}]
        }],
        "total": [{"denom": "uatom", "amount": "300.500000000000000000"}]
    })
}

fn unbonding_body() -> serde_json::Value {
    json!({"unbonding_responses": [], "pagination": {"next_key": null, "total": "0"}})
}

/// Развешивает все 4 endpoint'а на mock-сервере (ok-ответы).
async fn mount_happy_path(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex(r"^/cosmos/bank/v1beta1/balances/.+"))
        .respond_with(ResponseTemplate::new(200).set_body_json(balances_body()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/cosmos/staking/v1beta1/delegations/.+"))
        .respond_with(ResponseTemplate::new(200).set_body_json(delegations_body()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/cosmos/distribution/v1beta1/delegators/.+/rewards$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(rewards_body()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/cosmos/staking/v1beta1/delegators/.+/unbonding_delegations$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(unbonding_body()))
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// RestClient end-to-end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rest_client_fetches_all_four_endpoints() {
    let server = MockServer::start().await;
    mount_happy_path(&server).await;

    let client = RestClient::with_timeout(server.uri(), Duration::from_secs(3)).unwrap();
    let bals = client.fetch_balances(ADDR).await.unwrap();
    let dels = client.fetch_delegations(ADDR).await.unwrap();
    let rews = client.fetch_rewards(ADDR).await.unwrap();
    let unb = client.fetch_unbonding(ADDR).await.unwrap();

    assert_eq!(bals.len(), 1);
    assert_eq!(bals[0].amount, "1500000");
    assert_eq!(dels[0].balance.amount, "10000000");
    assert_eq!(rews.total[0].amount, "300.500000000000000000");
    assert!(unb.is_empty());
}

#[tokio::test]
async fn rest_5xx_classified_as_http_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/cosmos/bank/v1beta1/balances/{ADDR}")))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = RestClient::with_timeout(server.uri(), Duration::from_secs(2)).unwrap();
    let err = client.fetch_balances(ADDR).await.unwrap_err();
    match err {
        TransportError::HttpStatus { status, .. } => assert_eq!(status, 503),
        other => panic!("expected HttpStatus, got {other:?}"),
    }
}

#[tokio::test]
async fn rest_timeout_classified_as_timeout() {
    let server = MockServer::start().await;
    // Задержка ответа 2 сек, а клиент с таймаутом 500 мс.
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(balances_body())
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&server)
        .await;
    let client = RestClient::with_timeout(server.uri(), Duration::from_millis(500)).unwrap();

    let t0 = Instant::now();
    let err = client.fetch_balances(ADDR).await.unwrap_err();
    let elapsed = t0.elapsed();

    assert!(
        matches!(err, TransportError::Timeout),
        "expected Timeout, got {err:?}"
    );
    // 500ms клиент-side timeout ≤ прошедшее ≤ 1.5s (грубый sanity-check).
    assert!(
        elapsed < Duration::from_millis(1500),
        "timeout must fire early, got {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// query_wallet: REST pool fallback + round-robin
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fallback_rotates_to_next_rest_on_5xx() {
    // bad — всегда 503; good — happy path.
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&bad)
        .await;

    let good = MockServer::start().await;
    mount_happy_path(&good).await;

    // Pool: bad первым, good вторым. Первый запрос упадёт, fallback перейдёт на good.
    let pool = EndpointPool::new(vec![bad.uri(), good.uri()]);
    let pools = TransportPools {
        grpc: EndpointPool::default(),
        rest: pool,
        directory_rest: None,
    };

    let data = query_wallet(ADDR, &pools).await.expect("fallback to good");
    assert_eq!(data.balances.len(), 1);
    assert_eq!(data.balances[0].amount, "1500000");
    assert!(data.has_funds());
}

#[tokio::test]
async fn all_rest_endpoints_fail_returns_all_failed() {
    let bad1 = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&bad1)
        .await;
    let bad2 = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&bad2)
        .await;

    let pools = TransportPools::new(vec![], vec![bad1.uri(), bad2.uri()], None);
    let err = query_wallet(ADDR, &pools).await.unwrap_err();
    match err {
        TransportError::AllEndpointsFailed(v) => {
            assert!(v.len() >= 2, "should record failures for both endpoints");
            assert!(v.iter().any(|s| s.contains("500") || s.contains("502")));
        }
        other => panic!("expected AllEndpointsFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn fallback_uses_cosmos_directory_after_all_rest_fail() {
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&bad)
        .await;
    let directory = MockServer::start().await;
    mount_happy_path(&directory).await;

    let pools = TransportPools {
        grpc: EndpointPool::default(),
        rest: EndpointPool::new(vec![bad.uri()]),
        directory_rest: Some(directory.uri()),
    };
    let data = query_wallet(ADDR, &pools)
        .await
        .expect("directory fallback");
    assert_eq!(data.balances[0].amount, "1500000");
}

#[tokio::test]
async fn grpc_stub_skipped_and_rest_used() {
    // gRPC pool непустой, но stub всегда возвращает GrpcUnavailable, поэтому
    // должны перейти к REST.
    let good = MockServer::start().await;
    mount_happy_path(&good).await;

    let pools = TransportPools::new(
        vec!["grpc.nowhere:443".into(), "grpc.other:443".into()],
        vec![good.uri()],
        None,
    );
    let data = query_wallet(ADDR, &pools).await.expect("rest wins");
    assert_eq!(data.balances[0].amount, "1500000");
    // Все gRPC endpoint'ы помечены unhealthy.
    assert_eq!(pools.grpc.healthy_count(), 0);
}

#[tokio::test]
async fn unhealthy_rest_endpoint_skipped_on_second_call() {
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&bad)
        .await;
    let good = MockServer::start().await;
    mount_happy_path(&good).await;

    let pool = EndpointPool::new(vec![bad.uri(), good.uri()]);
    let pools = TransportPools {
        grpc: EndpointPool::default(),
        rest: pool.clone(),
        directory_rest: None,
    };

    // Первый вызов: bad → unhealthy, good → OK.
    let _ = query_wallet(ADDR, &pools).await.unwrap();
    assert_eq!(pool.healthy_count(), 1);

    // Второй вызов — bad должен быть пропущен пулом сразу.
    let bad_hits_before = bad.received_requests().await.unwrap().len();
    let _ = query_wallet(ADDR, &pools).await.unwrap();
    let bad_hits_after = bad.received_requests().await.unwrap().len();
    assert_eq!(
        bad_hits_before, bad_hits_after,
        "unhealthy endpoint must be skipped"
    );
}
