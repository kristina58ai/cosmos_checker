//! Stage 4 integration tests: `Registry::get_chain` поверх wiremock.

use cosmos_checker::chain_registry::{fetcher::Fetcher, EndpointKind, Registry};
use cosmos_checker::db::Db;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cosmoshub_chain_json() -> serde_json::Value {
    json!({
        "chain_name": "cosmoshub",
        "chain_id": "cosmoshub-4",
        "pretty_name": "Cosmos Hub",
        "bech32_prefix": "cosmos",
        "slip44": 118,
        "logo_URIs": {"png": "https://example.com/atom.png"},
        "apis": {
            "rpc":  [{"address": "https://rpc.cosmos.network:443",  "provider": "Cosmos Network"}],
            "rest": [{"address": "https://rest.cosmos.network"}],
            "grpc": [{"address": "grpc.cosmos.network:443"}]
        },
        "staking": {"staking_tokens": [{"denom": "uatom"}]}
    })
}

fn cosmoshub_assetlist_json() -> serde_json::Value {
    json!({
        "chain_name": "cosmoshub",
        "assets": [{
            "base": "uatom",
            "display": "atom",
            "symbol": "ATOM",
            "denom_units": [
                {"denom": "uatom", "exponent": 0},
                {"denom": "atom",  "exponent": 6}
            ]
        }]
    })
}

/// Вариант v2 — изменённое логотип-URL, чтобы проверить force_refresh.
fn cosmoshub_chain_json_v2() -> serde_json::Value {
    let mut v = cosmoshub_chain_json();
    v["logo_URIs"]["png"] = json!("https://example.com/atom-v2.png");
    v
}

async fn setup_server_v1() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cosmoshub/chain.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cosmoshub_chain_json()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cosmoshub/assetlist.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cosmoshub_assetlist_json()))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn fetch_and_cache_cosmoshub() {
    let server = setup_server_v1().await;
    let db = Db::in_memory().expect("db");
    let registry = Registry::new(db.clone(), Fetcher::for_tests(server.uri()), 24);

    let info = registry.get_chain("cosmoshub", false).await.expect("fetch");
    assert_eq!(info.chain_id, "cosmoshub-4");
    assert_eq!(info.bech32_prefix, "cosmos");
    assert_eq!(info.slip44, 118);

    // assetlist должен был перезаписать токены.
    assert_eq!(info.tokens.len(), 1);
    assert_eq!(info.tokens[0].display_denom, "ATOM");
    assert_eq!(info.tokens[0].exponent, 6);

    // endpoints всех трёх типов.
    assert!(info.endpoints.iter().any(|e| e.kind == EndpointKind::Grpc));
    assert!(info.endpoints.iter().any(|e| e.kind == EndpointKind::Rest));
    assert!(info.endpoints.iter().any(|e| e.kind == EndpointKind::Rpc));

    // Запись попала в SQLite — list_cached возвращает её.
    let cached = registry.list_cached().expect("list cached");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].chain_id, "cosmoshub-4");
}

#[tokio::test]
async fn cache_read_when_fresh_no_network() {
    // Первый вызов через живой сервер → кладёт в кеш.
    let server = setup_server_v1().await;
    let db = Db::in_memory().unwrap();
    let reg = Registry::new(db.clone(), Fetcher::for_tests(server.uri()), 24);
    reg.get_chain("cosmoshub", false).await.unwrap();

    // Останавливаем mock-сервер — любой реальный fetch упадёт по connect.
    drop(server);

    // Повторный вызов с force=false должен вернуться из кеша.
    let cached = reg.get_chain("cosmoshub", false).await.expect("from cache");
    assert_eq!(cached.chain_id, "cosmoshub-4");
}

#[tokio::test]
async fn force_refresh_hits_network_and_overwrites_cache() {
    let server = MockServer::start().await;
    // V1 отвечает 1 раз.
    Mock::given(method("GET"))
        .and(path("/cosmoshub/chain.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cosmoshub_chain_json()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Потом V2 — остальные запросы.
    Mock::given(method("GET"))
        .and(path("/cosmoshub/chain.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cosmoshub_chain_json_v2()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cosmoshub/assetlist.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cosmoshub_assetlist_json()))
        .mount(&server)
        .await;

    let db = Db::in_memory().unwrap();
    let reg = Registry::new(db.clone(), Fetcher::for_tests(server.uri()), 24);

    let v1 = reg.get_chain("cosmoshub", false).await.unwrap();
    assert_eq!(v1.logo_url.as_deref(), Some("https://example.com/atom.png"));

    let v2 = reg.force_refresh("cosmoshub").await.unwrap();
    assert_eq!(
        v2.logo_url.as_deref(),
        Some("https://example.com/atom-v2.png")
    );

    // Кеш действительно перезаписан.
    let cached = reg.list_cached().unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(
        cached[0].logo_url.as_deref(),
        Some("https://example.com/atom-v2.png")
    );
}

#[tokio::test]
async fn missing_assetlist_is_ok() {
    // chain.json есть, assetlist — нет (404).
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/minichain/chain.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "chain_name": "minichain",
            "chain_id": "mini-1",
            "bech32_prefix": "mini",
            "slip44": 118,
            "staking": {"staking_tokens": [{"denom": "umini"}]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/minichain/assetlist.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let db = Db::in_memory().unwrap();
    let reg = Registry::new(db, Fetcher::for_tests(server.uri()), 24);
    let info = reg.get_chain("minichain", false).await.unwrap();
    assert_eq!(info.chain_id, "mini-1");
    // Токены — fallback к staking_tokens без display/exponent.
    assert_eq!(info.tokens.len(), 1);
    assert_eq!(info.tokens[0].denom, "umini");
    assert_eq!(info.tokens[0].exponent, 0);
}

#[tokio::test]
async fn chain_not_found_returns_notfound() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let db = Db::in_memory().unwrap();
    let reg = Registry::new(db, Fetcher::for_tests(server.uri()), 24);
    let err = reg.get_chain("nosuch", false).await.unwrap_err();
    assert!(matches!(
        err,
        cosmos_checker::chain_registry::RegistryError::NotFound(_)
    ));
}

#[tokio::test]
async fn cache_ttl_zero_forces_refetch() {
    // TTL = 0 часов → кеш сразу "устаревший", второй вызов снова идёт в сеть.
    let server = setup_server_v1().await;
    let db = Db::in_memory().unwrap();
    let reg = Registry::new(db, Fetcher::for_tests(server.uri()), 0);

    let _a = reg.get_chain("cosmoshub", false).await.unwrap();
    let _b = reg.get_chain("cosmoshub", false).await.unwrap();

    // Оба запроса должны были попасть в mock (2 раза /chain.json).
    let requests = server.received_requests().await.expect("requests recorded");
    let chain_hits = requests
        .iter()
        .filter(|r| r.url.path() == "/cosmoshub/chain.json")
        .count();
    assert_eq!(chain_hits, 2, "TTL=0 must force refetch on every call");
}
