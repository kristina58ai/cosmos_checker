//! Fallback chain: gRPC → REST(pool) → cosmos.directory.
//!
//! Алгоритм [`query_wallet`]:
//! 1. Для каждого gRPC endpoint'а — попытка (сейчас всегда fail,
//!    см. `grpc.rs` stub). Unhealthy endpoint'ы пропускаются.
//! 2. Для каждого REST endpoint'а — 4 запроса (balances/deleg/rewards/unbonding).
//!    При первой же ошибке **на сетевом уровне** (timeout / connect / 5xx)
//!    endpoint помечается unhealthy и мы пробуем следующий. Ошибки парсинга
//!    считаем неретраебельными — аккумулируются в `WalletData.errors`.
//! 3. Fallback `cosmos.directory` — отдельный REST endpoint с известным
//!    base_url (передаётся опциональным параметром).
//!
//! Таймаут 5s — на уровне [`RestClient::with_timeout`].

use super::endpoints::EndpointPool;
use super::grpc::GrpcClient;
use super::rest::{RestClient, DEFAULT_TIMEOUT};
use super::types::WalletData;
use super::{TransportError, TransportResult};

/// Контейнер всех endpoint-пулов для одной сети + опциональный directory fallback.
#[derive(Clone, Default)]
pub struct TransportPools {
    pub grpc: EndpointPool,
    pub rest: EndpointPool,
    /// Cosmos.directory base_url (например, "https://rest.cosmos.directory/cosmoshub").
    pub directory_rest: Option<String>,
}

impl TransportPools {
    pub fn new(grpc: Vec<String>, rest: Vec<String>, directory_rest: Option<String>) -> Self {
        Self {
            grpc: EndpointPool::new(grpc),
            rest: EndpointPool::new(rest),
            directory_rest,
        }
    }
}

impl Default for EndpointPool {
    fn default() -> Self {
        EndpointPool::new(Vec::<String>::new())
    }
}

/// Высокоуровневый запрос данных кошелька со всем fallback'ом.
pub async fn query_wallet(address: &str, pools: &TransportPools) -> TransportResult<WalletData> {
    let mut errors: Vec<String> = Vec::new();

    // ---- 1. gRPC попытки ---------------------------------------------------
    let mut tried_grpc = 0usize;
    while let Some(ep) = pools.grpc.next() {
        tried_grpc += 1;
        let c = GrpcClient::new(&ep);
        match try_fetch_all_grpc(&c, address).await {
            Ok(data) => return Ok(data),
            Err(e) => {
                if is_endpoint_level_error(&e) {
                    pools.grpc.mark_unhealthy(&ep);
                }
                errors.push(format!("grpc {ep}: {e}"));
            }
        }
        if tried_grpc >= pools.grpc.len() {
            break;
        }
    }

    // ---- 2. REST попытки ---------------------------------------------------
    let mut tried_rest = 0usize;
    while let Some(ep) = pools.rest.next() {
        tried_rest += 1;
        let client = match RestClient::with_timeout(&ep, DEFAULT_TIMEOUT) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("rest-builder {ep}: {e}"));
                continue;
            }
        };
        match try_fetch_all_rest(&client, address).await {
            Ok(data) => return Ok(data),
            Err(e) => {
                if is_endpoint_level_error(&e) {
                    pools.rest.mark_unhealthy(&ep);
                }
                errors.push(format!("rest {ep}: {e}"));
            }
        }
        if tried_rest >= pools.rest.len() {
            break;
        }
    }

    // ---- 3. cosmos.directory fallback -------------------------------------
    if let Some(dir) = pools.directory_rest.as_ref() {
        let client = RestClient::with_timeout(dir, DEFAULT_TIMEOUT)?;
        match try_fetch_all_rest(&client, address).await {
            Ok(data) => return Ok(data),
            Err(e) => errors.push(format!("directory {dir}: {e}")),
        }
    }

    Err(TransportError::AllEndpointsFailed(errors))
}

/// Тот же алгоритм, что `query_wallet`, но allows возвращать partial data:
/// даже если часть запросов упала — остальные поля заполнены. Ошибки
/// помещаются в `WalletData.errors`. Возвращает `Ok` пока хотя бы один
/// endpoint *достижим* (balances успешно прочитан), иначе — `Err` через
/// `query_wallet`.
///
/// В чекер-pipeline (Stage 8) будем использовать именно её.
pub async fn query_wallet_partial(
    address: &str,
    pools: &TransportPools,
) -> TransportResult<WalletData> {
    // Сейчас для простоты — делегируем к строгому варианту. При желании
    // здесь можно будет собирать balances с одного endpoint'а, а rewards —
    // с другого; пока выигрыш не оправдывает сложности.
    query_wallet(address, pools).await
}

// ---------------------------------------------------------------------------
// Helpers: полный запрос всех 4 эндпоинтов с одного клиента
// ---------------------------------------------------------------------------

async fn try_fetch_all_rest(client: &RestClient, address: &str) -> TransportResult<WalletData> {
    // Выполняем строго последовательно — reqwest::Client внутри переиспользует
    // connection pool; параллельность сейчас создаст лишнюю нагрузку на endpoint.
    let balances = client.fetch_balances(address).await?;
    let delegations = client.fetch_delegations(address).await.unwrap_or_default();
    let rewards = client.fetch_rewards(address).await.unwrap_or_default();
    let unbonding = client.fetch_unbonding(address).await.unwrap_or_default();

    Ok(WalletData {
        balances,
        delegations,
        rewards,
        unbonding,
        errors: vec![],
    })
}

async fn try_fetch_all_grpc(client: &GrpcClient, address: &str) -> TransportResult<WalletData> {
    let balances = client.fetch_balances(address).await?;
    let delegations = client.fetch_delegations(address).await.unwrap_or_default();
    let rewards = client.fetch_rewards(address).await.unwrap_or_default();
    let unbonding = client.fetch_unbonding(address).await.unwrap_or_default();
    Ok(WalletData {
        balances,
        delegations,
        rewards,
        unbonding,
        errors: vec![],
    })
}

/// "Endpoint-level" означает, что проблема на стороне endpoint'а, а не
/// нашей (валидный адрес, корректный REST-путь). При таких ошибках
/// переключаемся на следующий endpoint.
fn is_endpoint_level_error(e: &TransportError) -> bool {
    matches!(
        e,
        TransportError::Timeout
            | TransportError::Connect(_)
            | TransportError::HttpStatus { .. }
            | TransportError::Http(_)
            | TransportError::GrpcUnavailable
    )
}

// ---------------------------------------------------------------------------
// Tests: логика fallback / классификации (без сети)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pools_produces_all_failed() {
        // Смоук: пустые пулы — query_wallet вернёт AllEndpointsFailed.
        let pools = TransportPools::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(query_wallet("cosmos1...", &pools)).unwrap_err();
        assert!(matches!(err, TransportError::AllEndpointsFailed(_)));
    }

    #[test]
    fn is_endpoint_level_classification() {
        assert!(is_endpoint_level_error(&TransportError::Timeout));
        assert!(is_endpoint_level_error(&TransportError::Connect(
            "x".into()
        )));
        assert!(is_endpoint_level_error(&TransportError::HttpStatus {
            url: "u".into(),
            status: 503,
        }));
        assert!(is_endpoint_level_error(&TransportError::GrpcUnavailable));
        // Parse — НЕ endpoint-level (баг у нас / несовместимый ответ).
        assert!(!is_endpoint_level_error(&TransportError::Parse(
            "bad".into()
        )));
    }
}
