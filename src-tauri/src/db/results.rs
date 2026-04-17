//! CRUD для `wallet_results`.
//!
//! JSON-поля (`balance_raw`, `staked_raw`, `rewards_raw`, `unbonding_raw`)
//! передаются как `serde_json::Value` и сериализуются в TEXT.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{Db, DbError, DbResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    Address,
    Seed,
    PrivateKey,
}

impl InputType {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Seed => "seed",
            Self::PrivateKey => "private_key",
        }
    }

    fn from_sql(s: &str) -> DbResult<Self> {
        Ok(match s {
            "address" => Self::Address,
            "seed" => Self::Seed,
            "private_key" => Self::PrivateKey,
            other => return Err(DbError::Invalid(format!("input_type: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWalletResult {
    pub session_id: i64,
    pub address: String,
    pub chain_id: String,
    pub input_type: InputType,
    pub balance_raw: Option<JsonValue>,
    pub balance_display: Option<String>,
    pub staked_raw: Option<JsonValue>,
    pub staked_display: Option<String>,
    pub rewards_raw: Option<JsonValue>,
    pub rewards_display: Option<String>,
    pub unbonding_raw: Option<JsonValue>,
    pub unbonding_display: Option<String>,
    pub has_funds: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletResultRow {
    pub id: i64,
    pub session_id: i64,
    pub address: String,
    pub chain_id: String,
    pub input_type: InputType,
    pub balance_raw: Option<JsonValue>,
    pub balance_display: Option<String>,
    pub staked_raw: Option<JsonValue>,
    pub staked_display: Option<String>,
    pub rewards_raw: Option<JsonValue>,
    pub rewards_display: Option<String>,
    pub unbonding_raw: Option<JsonValue>,
    pub unbonding_display: Option<String>,
    pub has_funds: bool,
    pub error: Option<String>,
    pub checked_at: String,
}

fn json_to_text(v: &Option<JsonValue>) -> Option<String> {
    v.as_ref().map(|x| x.to_string())
}

fn text_to_json(s: Option<String>) -> DbResult<Option<JsonValue>> {
    match s {
        None => Ok(None),
        Some(s) => serde_json::from_str::<JsonValue>(&s)
            .map(Some)
            .map_err(|e| DbError::Invalid(format!("json decode: {e}"))),
    }
}

pub fn insert_result(db: &Db, r: &NewWalletResult) -> DbResult<i64> {
    let c = db.conn()?;
    c.execute(
        "INSERT INTO wallet_results (
            session_id, address, chain_id, input_type,
            balance_raw, balance_display,
            staked_raw, staked_display,
            rewards_raw, rewards_display,
            unbonding_raw, unbonding_display,
            has_funds, error
         ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            ?13, ?14
         )",
        params![
            r.session_id,
            r.address,
            r.chain_id,
            r.input_type.as_sql(),
            json_to_text(&r.balance_raw),
            r.balance_display,
            json_to_text(&r.staked_raw),
            r.staked_display,
            json_to_text(&r.rewards_raw),
            r.rewards_display,
            json_to_text(&r.unbonding_raw),
            r.unbonding_display,
            r.has_funds as i64,
            r.error,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// Батч-вставка — один коммит, ускоряет в разы.
pub fn insert_results_batch(db: &Db, rows: &[NewWalletResult]) -> DbResult<usize> {
    db.with_tx(|tx| {
        let mut stmt = tx.prepare(
            "INSERT INTO wallet_results (
                session_id, address, chain_id, input_type,
                balance_raw, balance_display,
                staked_raw, staked_display,
                rewards_raw, rewards_display,
                unbonding_raw, unbonding_display,
                has_funds, error
             ) VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14
             )",
        )?;
        let mut n = 0usize;
        for r in rows {
            stmt.execute(params![
                r.session_id,
                r.address,
                r.chain_id,
                r.input_type.as_sql(),
                json_to_text(&r.balance_raw),
                r.balance_display,
                json_to_text(&r.staked_raw),
                r.staked_display,
                json_to_text(&r.rewards_raw),
                r.rewards_display,
                json_to_text(&r.unbonding_raw),
                r.unbonding_display,
                r.has_funds as i64,
                r.error,
            ])?;
            n += 1;
        }
        Ok(n)
    })
}

/// Сырое представление строки `wallet_results` в том порядке, что `SELECT_COLS`.
/// Вынесено в `type`, чтобы удовлетворить `clippy::type_complexity`.
type WalletRowTuple = (
    i64,            // id
    i64,            // session_id
    String,         // address
    String,         // chain_id
    String,         // input_type
    Option<String>, // balance_raw
    Option<String>, // balance_display
    Option<String>, // staked_raw
    Option<String>, // staked_display
    Option<String>, // rewards_raw
    Option<String>, // rewards_display
    Option<String>, // unbonding_raw
    Option<String>, // unbonding_display
    i64,            // has_funds
    Option<String>, // error
    String,         // checked_at
);

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WalletRowTuple> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
        r.get(15)?,
    ))
}

fn build_row(t: WalletRowTuple) -> DbResult<WalletResultRow> {
    Ok(WalletResultRow {
        id: t.0,
        session_id: t.1,
        address: t.2,
        chain_id: t.3,
        input_type: InputType::from_sql(&t.4)?,
        balance_raw: text_to_json(t.5)?,
        balance_display: t.6,
        staked_raw: text_to_json(t.7)?,
        staked_display: t.8,
        rewards_raw: text_to_json(t.9)?,
        rewards_display: t.10,
        unbonding_raw: text_to_json(t.11)?,
        unbonding_display: t.12,
        has_funds: t.13 != 0,
        error: t.14,
        checked_at: t.15,
    })
}

const SELECT_COLS: &str = "id, session_id, address, chain_id, input_type,
    balance_raw, balance_display,
    staked_raw, staked_display,
    rewards_raw, rewards_display,
    unbonding_raw, unbonding_display,
    has_funds, error, checked_at";

pub fn get_result(db: &Db, id: i64) -> DbResult<Option<WalletResultRow>> {
    let c = db.conn()?;
    let tup = c
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM wallet_results WHERE id = ?1"),
            params![id],
            map_row,
        )
        .optional()?;
    match tup {
        None => Ok(None),
        Some(t) => Ok(Some(build_row(t)?)),
    }
}

pub fn list_by_session(
    db: &Db,
    session_id: i64,
    only_with_funds: bool,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<WalletResultRow>> {
    let c = db.conn()?;
    let sql = if only_with_funds {
        format!(
            "SELECT {SELECT_COLS} FROM wallet_results
             WHERE session_id = ?1 AND has_funds = 1
             ORDER BY id ASC LIMIT ?2 OFFSET ?3"
        )
    } else {
        format!(
            "SELECT {SELECT_COLS} FROM wallet_results
             WHERE session_id = ?1
             ORDER BY id ASC LIMIT ?2 OFFSET ?3"
        )
    };
    let mut stmt = c.prepare(&sql)?;
    let tups = stmt
        .query_map(params![session_id, limit, offset], map_row)?
        .collect::<Result<Vec<_>, _>>()?;
    tups.into_iter().map(build_row).collect()
}

pub fn count_by_session(db: &Db, session_id: i64) -> DbResult<i64> {
    let c = db.conn()?;
    Ok(c.query_row(
        "SELECT COUNT(*) FROM wallet_results WHERE session_id = ?1",
        params![session_id],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chains::{upsert_chain, ChainRow};
    use crate::db::sessions::create_session;
    use serde_json::json;

    fn prep(db: &Db) -> i64 {
        upsert_chain(
            db,
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
        create_session(db, None, 1).unwrap()
    }

    fn sample(session_id: i64) -> NewWalletResult {
        NewWalletResult {
            session_id,
            address: "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4".into(),
            chain_id: "cosmoshub-4".into(),
            input_type: InputType::Seed,
            balance_raw: Some(json!([{"denom":"uatom","amount":"1500000"}])),
            balance_display: Some("1.5 ATOM".into()),
            staked_raw: None,
            staked_display: None,
            rewards_raw: Some(json!([])),
            rewards_display: Some("0".into()),
            unbonding_raw: None,
            unbonding_display: None,
            has_funds: true,
            error: None,
        }
    }

    #[test]
    fn insert_wallet_result_with_json_fields() {
        let db = Db::in_memory().unwrap();
        let sid = prep(&db);
        let id = insert_result(&db, &sample(sid)).unwrap();
        let got = get_result(&db, id).unwrap().unwrap();
        assert_eq!(got.address, "cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4");
        assert_eq!(
            got.balance_raw.unwrap()[0]["amount"].as_str(),
            Some("1500000")
        );
        assert!(got.has_funds);
        assert_eq!(got.input_type, InputType::Seed);
    }

    #[test]
    fn batch_insert_and_count() {
        let db = Db::in_memory().unwrap();
        let sid = prep(&db);
        let rows: Vec<_> = (0..50).map(|_| sample(sid)).collect();
        let n = insert_results_batch(&db, &rows).unwrap();
        assert_eq!(n, 50);
        assert_eq!(count_by_session(&db, sid).unwrap(), 50);
    }

    #[test]
    fn list_with_funds_filter() {
        let db = Db::in_memory().unwrap();
        let sid = prep(&db);
        let mut a = sample(sid);
        a.has_funds = true;
        let mut b = sample(sid);
        b.has_funds = false;
        insert_result(&db, &a).unwrap();
        insert_result(&db, &b).unwrap();

        let all = list_by_session(&db, sid, false, 100, 0).unwrap();
        assert_eq!(all.len(), 2);
        let only = list_by_session(&db, sid, true, 100, 0).unwrap();
        assert_eq!(only.len(), 1);
        assert!(only[0].has_funds);
    }

    #[test]
    fn foreign_key_enforced_on_bad_session() {
        let db = Db::in_memory().unwrap();
        // chain есть, но session_id несуществующий
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
        let mut bad = sample(9999);
        bad.session_id = 9999;
        let err = insert_result(&db, &bad).unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)));
    }
}
