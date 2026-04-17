//! gRPC-транспорт (stub).
//!
//! TODO(stage-5b): полноценная реализация через `tonic` + `prost`.
//!
//! Обоснование отложенной реализации: cosmos-sdk proto-дерево (bank, staking,
//! distribution + gogoproto + googleapis) суммарно ~150 .proto файлов и
//! разворачивается в несколько сотен килобайт сгенерированного кода.
//! Встраивание этого в текущий коммит раздует репозиторий и build time, а
//! gRPC-fallback в нашем fallback-chain'е — опциональный шаг (REST всегда
//! доступен как первый полноценный транспорт).
//!
//! Поэтому пока `GrpcClient` всегда возвращает [`TransportError::GrpcUnavailable`],
//! что корректно протаскивается через [`fallback::query_wallet`]: gRPC-попытки
//! быстро провалятся и мы перейдём к REST. Как только будет готов codegen,
//! здесь появится реальная реализация — сигнатуры публичных методов
//! останутся теми же.

use super::types::{Coin, Delegation, Rewards, UnbondingDelegation};
use super::{TransportError, TransportResult};

/// Stub gRPC-клиент. Реальная имплементация — stage 5b.
#[derive(Clone)]
pub struct GrpcClient {
    endpoint: String,
}

impl GrpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub async fn fetch_balances(&self, _address: &str) -> TransportResult<Vec<Coin>> {
        Err(TransportError::GrpcUnavailable)
    }

    pub async fn fetch_delegations(&self, _address: &str) -> TransportResult<Vec<Delegation>> {
        Err(TransportError::GrpcUnavailable)
    }

    pub async fn fetch_rewards(&self, _address: &str) -> TransportResult<Rewards> {
        Err(TransportError::GrpcUnavailable)
    }

    pub async fn fetch_unbonding(
        &self,
        _address: &str,
    ) -> TransportResult<Vec<UnbondingDelegation>> {
        Err(TransportError::GrpcUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_grpc_unavailable() {
        let c = GrpcClient::new("grpc.cosmos.network:443");
        let err = c.fetch_balances("cosmos1...").await.unwrap_err();
        assert!(matches!(err, TransportError::GrpcUnavailable));
    }
}
