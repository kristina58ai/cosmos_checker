//! Парсер `chain.json` и `assetlist.json` из cosmos/chain-registry.
//!
//! Pure-функции: принимают уже распарсенный `serde_json::Value` и
//! возвращают наши внутренние структуры. Graceful к пропущенным полям
//! (например, `apis.grpc` может отсутствовать — вернётся пустой вектор).

use serde_json::Value;

use super::{ChainInfo, EndpointInfo, EndpointKind, RegistryError, RegistryResult, TokenInfo};

/// Распарсить `chain.json` в [`ChainInfo`].
///
/// Обязательные поля: `chain_id`, `chain_name`, `bech32_prefix`.
/// `slip44` берётся из `slip44` (дефолт 118 если отсутствует — это и есть
/// cosmos coin-type).
pub fn parse_chain_json(v: &Value) -> RegistryResult<ChainInfo> {
    let obj = v
        .as_object()
        .ok_or_else(|| RegistryError::Malformed("root not an object".into()))?;

    let chain_id = get_str(obj, "chain_id")?.to_owned();
    let chain_name = get_str(obj, "chain_name")?.to_owned();
    let bech32_prefix = get_str(obj, "bech32_prefix")?.to_owned();

    // slip44: спеком задано NOT NULL, но в реальных файлах registry он
    // иногда отсутствует для тест-сетей. Берём 118 (cosmos) как дефолт.
    let slip44: u32 = obj
        .get("slip44")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32)
        .unwrap_or(118);

    let pretty_name = obj
        .get("pretty_name")
        .and_then(|x| x.as_str())
        .map(str::to_owned);

    let logo_url = obj
        .get("logo_URIs")
        .and_then(|x| x.as_object())
        .and_then(|l| l.get("png").or_else(|| l.get("svg")))
        .and_then(|x| x.as_str())
        .map(str::to_owned);

    let endpoints = parse_endpoints(obj.get("apis"));

    // Базовые токены достанем только из staking.staking_tokens / fees.fee_tokens
    // — без `display_denom` / `exponent`. Реальные метаданные приходят из
    // `assetlist.json` и перезатирают этот набор в `Registry::fetch_and_cache`.
    let tokens = collect_base_tokens(obj);

    Ok(ChainInfo {
        chain_id,
        chain_name,
        pretty_name,
        bech32_prefix,
        slip44,
        logo_url,
        endpoints,
        tokens,
    })
}

/// Распарсить `assetlist.json` в набор `TokenInfo`.
///
/// Структура:
/// ```json
/// { "assets": [
///     { "base": "uatom", "display": "atom", "symbol": "ATOM",
///       "denom_units": [{"denom":"uatom","exponent":0},{"denom":"atom","exponent":6}] }
/// ] }
/// ```
/// Мы берём `base` как denom, `symbol` как display_denom, exponent из
/// denom_units для `display` (наибольший, как правило).
pub fn parse_assetlist_json(v: &Value) -> RegistryResult<Vec<TokenInfo>> {
    let Some(assets) = v.get("assets").and_then(|x| x.as_array()) else {
        return Ok(vec![]);
    };
    let mut out = Vec::with_capacity(assets.len());
    for a in assets {
        let Some(obj) = a.as_object() else { continue };
        let base = obj.get("base").and_then(|x| x.as_str()).unwrap_or("");
        if base.is_empty() {
            continue;
        }
        let display = obj.get("display").and_then(|x| x.as_str()).unwrap_or(base);
        let symbol = obj
            .get("symbol")
            .and_then(|x| x.as_str())
            .unwrap_or(display);

        let exponent = obj
            .get("denom_units")
            .and_then(|x| x.as_array())
            .and_then(|units| {
                units.iter().find_map(|u| {
                    let denom = u.get("denom").and_then(|d| d.as_str())?;
                    if denom == display {
                        u.get("exponent").and_then(|e| e.as_u64())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0) as u32;

        out.push(TokenInfo {
            denom: base.to_owned(),
            display_denom: symbol.to_owned(),
            exponent,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn get_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> RegistryResult<&'a str> {
    obj.get(key)
        .and_then(|x| x.as_str())
        .ok_or_else(|| RegistryError::Malformed(format!("missing `{key}`")))
}

fn parse_endpoints(apis: Option<&Value>) -> Vec<EndpointInfo> {
    let Some(apis) = apis.and_then(|x| x.as_object()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (k, kind) in [
        ("grpc", EndpointKind::Grpc),
        ("rest", EndpointKind::Rest),
        ("rpc", EndpointKind::Rpc),
    ] {
        let Some(arr) = apis.get(k).and_then(|x| x.as_array()) else {
            continue;
        };
        for ep in arr {
            let Some(eobj) = ep.as_object() else { continue };
            let Some(addr) = eobj.get("address").and_then(|x| x.as_str()) else {
                continue;
            };
            if addr.is_empty() {
                continue;
            }
            let provider = eobj
                .get("provider")
                .and_then(|x| x.as_str())
                .map(str::to_owned);
            out.push(EndpointInfo {
                kind,
                address: addr.to_owned(),
                provider,
            });
        }
    }
    out
}

fn collect_base_tokens(obj: &serde_json::Map<String, Value>) -> Vec<TokenInfo> {
    let mut denoms: Vec<String> = Vec::new();

    if let Some(toks) = obj
        .get("staking")
        .and_then(|x| x.get("staking_tokens"))
        .and_then(|x| x.as_array())
    {
        for t in toks {
            if let Some(d) = t.get("denom").and_then(|x| x.as_str()) {
                denoms.push(d.to_owned());
            }
        }
    }
    if let Some(toks) = obj
        .get("fees")
        .and_then(|x| x.get("fee_tokens"))
        .and_then(|x| x.as_array())
    {
        for t in toks {
            if let Some(d) = t.get("denom").and_then(|x| x.as_str()) {
                denoms.push(d.to_owned());
            }
        }
    }
    denoms.sort();
    denoms.dedup();
    denoms
        .into_iter()
        .map(|d| TokenInfo {
            denom: d.clone(),
            display_denom: d, // без assetlist — display==base
            exponent: 0,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cosmoshub_fixture() -> Value {
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
                "grpc": [
                    {"address": "grpc.cosmos.network:443", "provider": "Cosmos Network"},
                    {"address": "cosmoshub-grpc.polkachu.com:14990", "provider": "Polkachu"}
                ]
            },
            "staking": {"staking_tokens": [{"denom": "uatom"}]},
            "fees":    {"fee_tokens":     [{"denom": "uatom", "fixed_min_gas_price": 0.0}]}
        })
    }

    fn cosmoshub_assetlist_fixture() -> Value {
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

    #[test]
    fn parse_cosmoshub_chain_json() {
        let info = parse_chain_json(&cosmoshub_fixture()).unwrap();
        assert_eq!(info.chain_id, "cosmoshub-4");
        assert_eq!(info.chain_name, "cosmoshub");
        assert_eq!(info.bech32_prefix, "cosmos");
        assert_eq!(info.slip44, 118);
        assert_eq!(info.pretty_name.as_deref(), Some("Cosmos Hub"));
        assert_eq!(
            info.logo_url.as_deref(),
            Some("https://example.com/atom.png")
        );
    }

    #[test]
    fn endpoints_extracted_all_types() {
        let info = parse_chain_json(&cosmoshub_fixture()).unwrap();
        let grpc: Vec<_> = info
            .endpoints
            .iter()
            .filter(|e| e.kind == EndpointKind::Grpc)
            .collect();
        let rest: Vec<_> = info
            .endpoints
            .iter()
            .filter(|e| e.kind == EndpointKind::Rest)
            .collect();
        let rpc: Vec<_> = info
            .endpoints
            .iter()
            .filter(|e| e.kind == EndpointKind::Rpc)
            .collect();
        assert_eq!(grpc.len(), 2);
        assert_eq!(rest.len(), 1);
        assert_eq!(rpc.len(), 1);
        assert_eq!(grpc[0].provider.as_deref(), Some("Cosmos Network"));
    }

    #[test]
    fn handle_missing_fields_gracefully() {
        // Нет apis.grpc → должен быть пустой список grpc, не ошибка.
        let v = json!({
            "chain_name": "foo",
            "chain_id":   "foo-1",
            "bech32_prefix": "foo",
            "slip44": 118,
            "apis": {"rest": [{"address": "https://r"}]}
        });
        let info = parse_chain_json(&v).unwrap();
        assert!(info.endpoints.iter().all(|e| e.kind != EndpointKind::Grpc));
        assert_eq!(info.logo_url, None);
        assert_eq!(info.pretty_name, None);
    }

    #[test]
    fn nonstandard_slip44_handled() {
        // Terra (было 330 до депега, сейчас 118, но в fixture ставим 330).
        let v = json!({
            "chain_name": "terra",
            "chain_id":   "columbus-5",
            "bech32_prefix": "terra",
            "slip44": 330
        });
        let info = parse_chain_json(&v).unwrap();
        assert_eq!(info.slip44, 330);
        assert_eq!(info.bech32_prefix, "terra");
        assert!(info.endpoints.is_empty());
    }

    #[test]
    fn missing_required_field_is_error() {
        let v = json!({"chain_name": "foo", "slip44": 118});
        let err = parse_chain_json(&v).unwrap_err();
        assert!(matches!(err, RegistryError::Malformed(_)));
    }

    #[test]
    fn assetlist_parses_uatom() {
        let toks = parse_assetlist_json(&cosmoshub_assetlist_fixture()).unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].denom, "uatom");
        assert_eq!(toks[0].display_denom, "ATOM");
        assert_eq!(toks[0].exponent, 6);
    }

    #[test]
    fn assetlist_missing_is_empty() {
        let toks = parse_assetlist_json(&json!({})).unwrap();
        assert!(toks.is_empty());
    }

    #[test]
    fn base_tokens_collected_without_assetlist() {
        let info = parse_chain_json(&cosmoshub_fixture()).unwrap();
        assert_eq!(info.tokens.len(), 1);
        assert_eq!(info.tokens[0].denom, "uatom");
        assert_eq!(info.tokens[0].exponent, 0); // без assetlist
    }
}
