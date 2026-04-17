//! CRUD для таблиц `chains`, `chain_endpoints`, `chain_tokens`.
//!
//! Все запросы — prepared statements (`params![]`). См. `docs/CLAUDE.md §5 T6`.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{Db, DbError, DbResult};

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainRow {
    pub chain_id: String,
    pub chain_name: String,
    pub bech32_prefix: String,
    pub slip44: u32,
    pub display_name: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointRow {
    pub id: i64,
    pub chain_id: String,
    pub endpoint_type: String, // 'grpc' | 'rest' | 'rpc'
    pub address: String,
    pub provider: Option<String>,
    pub is_healthy: bool,
    pub avg_latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenRow {
    pub id: i64,
    pub chain_id: String,
    pub denom: String,
    pub display_denom: String,
    pub exponent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEndpoint {
    pub endpoint_type: String,
    pub address: String,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewToken {
    pub denom: String,
    pub display_denom: String,
    pub exponent: u32,
}

// ---------------------------------------------------------------------------
// Chains CRUD
// ---------------------------------------------------------------------------

/// Upsert chain-строки. Обновляет все поля при конфликте по PK.
pub fn upsert_chain(db: &Db, row: &ChainRow) -> DbResult<()> {
    let c = db.conn()?;
    c.execute(
        "INSERT INTO chains (chain_id, chain_name, bech32_prefix, slip44, display_name, logo_url, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
         ON CONFLICT(chain_id) DO UPDATE SET
             chain_name = excluded.chain_name,
             bech32_prefix = excluded.bech32_prefix,
             slip44 = excluded.slip44,
             display_name = excluded.display_name,
             logo_url = excluded.logo_url,
             updated_at = datetime('now')",
        params![
            row.chain_id,
            row.chain_name,
            row.bech32_prefix,
            row.slip44,
            row.display_name,
            row.logo_url,
        ],
    )?;
    Ok(())
}

pub fn get_chain(db: &Db, chain_id: &str) -> DbResult<Option<ChainRow>> {
    let c = db.conn()?;
    let row = c
        .query_row(
            "SELECT chain_id, chain_name, bech32_prefix, slip44, display_name, logo_url
             FROM chains WHERE chain_id = ?1",
            params![chain_id],
            |r| {
                Ok(ChainRow {
                    chain_id: r.get(0)?,
                    chain_name: r.get(1)?,
                    bech32_prefix: r.get(2)?,
                    slip44: r.get::<_, i64>(3)? as u32,
                    display_name: r.get(4)?,
                    logo_url: r.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn list_chains(db: &Db) -> DbResult<Vec<ChainRow>> {
    let c = db.conn()?;
    let mut stmt = c.prepare(
        "SELECT chain_id, chain_name, bech32_prefix, slip44, display_name, logo_url
         FROM chains ORDER BY chain_id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ChainRow {
                chain_id: r.get(0)?,
                chain_name: r.get(1)?,
                bech32_prefix: r.get(2)?,
                slip44: r.get::<_, i64>(3)? as u32,
                display_name: r.get(4)?,
                logo_url: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_chain(db: &Db, chain_id: &str) -> DbResult<()> {
    let c = db.conn()?;
    let n = c.execute("DELETE FROM chains WHERE chain_id = ?1", params![chain_id])?;
    if n == 0 {
        return Err(DbError::NotFound(format!("chain {chain_id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// Заменить все endpoints для chain_id (транзакционно).
pub fn replace_endpoints(db: &Db, chain_id: &str, endpoints: &[NewEndpoint]) -> DbResult<()> {
    db.with_tx(|tx| {
        tx.execute(
            "DELETE FROM chain_endpoints WHERE chain_id = ?1",
            params![chain_id],
        )?;
        for ep in endpoints {
            if !matches!(ep.endpoint_type.as_str(), "grpc" | "rest" | "rpc") {
                return Err(DbError::Invalid(format!(
                    "endpoint_type: {}",
                    ep.endpoint_type
                )));
            }
            tx.execute(
                "INSERT OR IGNORE INTO chain_endpoints
                    (chain_id, endpoint_type, address, provider, is_healthy)
                 VALUES (?1, ?2, ?3, ?4, 1)",
                params![chain_id, ep.endpoint_type, ep.address, ep.provider],
            )?;
        }
        Ok(())
    })
}

pub fn list_endpoints(db: &Db, chain_id: &str) -> DbResult<Vec<EndpointRow>> {
    let c = db.conn()?;
    let mut stmt = c.prepare(
        "SELECT id, chain_id, endpoint_type, address, provider, is_healthy, avg_latency_ms
         FROM chain_endpoints WHERE chain_id = ?1 ORDER BY endpoint_type, id",
    )?;
    let rows = stmt
        .query_map(params![chain_id], |r| {
            Ok(EndpointRow {
                id: r.get(0)?,
                chain_id: r.get(1)?,
                endpoint_type: r.get(2)?,
                address: r.get(3)?,
                provider: r.get(4)?,
                is_healthy: r.get::<_, i64>(5)? != 0,
                avg_latency_ms: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_endpoint_health(
    db: &Db,
    endpoint_id: i64,
    is_healthy: bool,
    avg_latency_ms: Option<i64>,
) -> DbResult<()> {
    let c = db.conn()?;
    c.execute(
        "UPDATE chain_endpoints
            SET is_healthy = ?1,
                avg_latency_ms = COALESCE(?2, avg_latency_ms),
                last_check_at = datetime('now')
          WHERE id = ?3",
        params![is_healthy as i64, avg_latency_ms, endpoint_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

pub fn replace_tokens(db: &Db, chain_id: &str, tokens: &[NewToken]) -> DbResult<()> {
    db.with_tx(|tx| {
        tx.execute(
            "DELETE FROM chain_tokens WHERE chain_id = ?1",
            params![chain_id],
        )?;
        for t in tokens {
            tx.execute(
                "INSERT OR IGNORE INTO chain_tokens (chain_id, denom, display_denom, exponent)
                 VALUES (?1, ?2, ?3, ?4)",
                params![chain_id, t.denom, t.display_denom, t.exponent],
            )?;
        }
        Ok(())
    })
}

pub fn list_tokens(db: &Db, chain_id: &str) -> DbResult<Vec<TokenRow>> {
    let c = db.conn()?;
    let mut stmt = c.prepare(
        "SELECT id, chain_id, denom, display_denom, exponent
         FROM chain_tokens WHERE chain_id = ?1 ORDER BY denom",
    )?;
    let rows = stmt
        .query_map(params![chain_id], |r| {
            Ok(TokenRow {
                id: r.get(0)?,
                chain_id: r.get(1)?,
                denom: r.get(2)?,
                display_denom: r.get(3)?,
                exponent: r.get::<_, i64>(4)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chain() -> ChainRow {
        ChainRow {
            chain_id: "cosmoshub-4".into(),
            chain_name: "cosmoshub".into(),
            bech32_prefix: "cosmos".into(),
            slip44: 118,
            display_name: Some("Cosmos Hub".into()),
            logo_url: None,
        }
    }

    #[test]
    fn insert_and_query_chain() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        let got = get_chain(&db, "cosmoshub-4").unwrap().unwrap();
        assert_eq!(got, sample_chain());
    }

    #[test]
    fn upsert_chain_is_idempotent_and_updates() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();

        let mut updated = sample_chain();
        updated.display_name = Some("Cosmos Hub Mainnet".into());
        upsert_chain(&db, &updated).unwrap();

        let got = get_chain(&db, "cosmoshub-4").unwrap().unwrap();
        assert_eq!(got.display_name.as_deref(), Some("Cosmos Hub Mainnet"));
        assert_eq!(list_chains(&db).unwrap().len(), 1);
    }

    #[test]
    fn replace_endpoints_and_list() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        let eps = vec![
            NewEndpoint {
                endpoint_type: "grpc".into(),
                address: "https://grpc.cosmos.network:443".into(),
                provider: Some("Cosmos Network".into()),
            },
            NewEndpoint {
                endpoint_type: "rest".into(),
                address: "https://rest.cosmos.network".into(),
                provider: None,
            },
        ];
        replace_endpoints(&db, "cosmoshub-4", &eps).unwrap();
        let got = list_endpoints(&db, "cosmoshub-4").unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|e| e.is_healthy));
    }

    #[test]
    fn invalid_endpoint_type_rejected() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        let eps = vec![NewEndpoint {
            endpoint_type: "wss".into(), // <-- не входит в CHECK
            address: "wss://foo".into(),
            provider: None,
        }];
        let err = replace_endpoints(&db, "cosmoshub-4", &eps).unwrap_err();
        assert!(matches!(err, DbError::Invalid(_)));
    }

    #[test]
    fn mark_endpoint_health_updates_row() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        replace_endpoints(
            &db,
            "cosmoshub-4",
            &[NewEndpoint {
                endpoint_type: "grpc".into(),
                address: "https://g".into(),
                provider: None,
            }],
        )
        .unwrap();
        let ep = list_endpoints(&db, "cosmoshub-4").unwrap().remove(0);
        mark_endpoint_health(&db, ep.id, false, Some(999)).unwrap();
        let ep2 = list_endpoints(&db, "cosmoshub-4").unwrap().remove(0);
        assert!(!ep2.is_healthy);
        assert_eq!(ep2.avg_latency_ms, Some(999));
    }

    #[test]
    fn replace_tokens_and_list() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        replace_tokens(
            &db,
            "cosmoshub-4",
            &[NewToken {
                denom: "uatom".into(),
                display_denom: "ATOM".into(),
                exponent: 6,
            }],
        )
        .unwrap();
        let toks = list_tokens(&db, "cosmoshub-4").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].display_denom, "ATOM");
    }

    #[test]
    fn delete_chain_cascades_endpoints_and_tokens() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        replace_endpoints(
            &db,
            "cosmoshub-4",
            &[NewEndpoint {
                endpoint_type: "rest".into(),
                address: "https://r".into(),
                provider: None,
            }],
        )
        .unwrap();
        replace_tokens(
            &db,
            "cosmoshub-4",
            &[NewToken {
                denom: "uatom".into(),
                display_denom: "ATOM".into(),
                exponent: 6,
            }],
        )
        .unwrap();

        delete_chain(&db, "cosmoshub-4").unwrap();

        assert!(get_chain(&db, "cosmoshub-4").unwrap().is_none());
        assert!(list_endpoints(&db, "cosmoshub-4").unwrap().is_empty());
        assert!(list_tokens(&db, "cosmoshub-4").unwrap().is_empty());
    }

    #[test]
    fn sql_injection_prevented_prepared_statements() {
        let db = Db::in_memory().unwrap();
        upsert_chain(&db, &sample_chain()).unwrap();
        // Злой chain_id — попытка injection. Prepared statement должен
        // трактовать это как литерал, никакой другой строки не появится.
        let evil = "cosmoshub-4'; DROP TABLE chains; --";
        let got = get_chain(&db, evil).unwrap();
        assert!(got.is_none());
        // Убеждаемся, что таблица жива:
        assert!(get_chain(&db, "cosmoshub-4").unwrap().is_some());
    }
}
