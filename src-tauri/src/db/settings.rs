//! Get/Set для `app_settings` (простой key→value store).

use rusqlite::{params, OptionalExtension};

use super::{Db, DbError, DbResult};

pub fn get(db: &Db, key: &str) -> DbResult<Option<String>> {
    let c = db.conn()?;
    Ok(c.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .optional()?)
}

/// Вернуть значение или дефолт.
pub fn get_or(db: &Db, key: &str, default: &str) -> DbResult<String> {
    Ok(get(db, key)?.unwrap_or_else(|| default.to_owned()))
}

/// Типизированный хелпер для числовых настроек (i64).
pub fn get_i64(db: &Db, key: &str) -> DbResult<Option<i64>> {
    match get(db, key)? {
        None => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|e| DbError::Invalid(format!("{key}: not i64 ({e})"))),
    }
}

/// Типизированный хелпер для bool (строковые "true"/"false").
pub fn get_bool(db: &Db, key: &str) -> DbResult<Option<bool>> {
    match get(db, key)? {
        None => Ok(None),
        Some(s) => match s.as_str() {
            "true" | "1" => Ok(Some(true)),
            "false" | "0" => Ok(Some(false)),
            other => Err(DbError::Invalid(format!("{key}: not bool ({other})"))),
        },
    }
}

/// Upsert: либо вставить, либо обновить существующий ключ.
pub fn set(db: &Db, key: &str, value: &str) -> DbResult<()> {
    let c = db.conn()?;
    c.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete(db: &Db, key: &str) -> DbResult<bool> {
    let c = db.conn()?;
    let n = c.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
    Ok(n > 0)
}

pub fn list_all(db: &Db) -> DbResult<Vec<(String, String)>> {
    let c = db.conn()?;
    let mut stmt = c.prepare("SELECT key, value FROM app_settings ORDER BY key")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seed_present() {
        let db = Db::in_memory().unwrap();
        assert_eq!(get(&db, "max_concurrency").unwrap().as_deref(), Some("100"));
        assert_eq!(get_i64(&db, "max_concurrency").unwrap(), Some(100));
        assert_eq!(get_bool(&db, "fallback_enabled").unwrap(), Some(true));
    }

    #[test]
    fn settings_upsert() {
        let db = Db::in_memory().unwrap();
        set(&db, "max_concurrency", "50").unwrap();
        assert_eq!(get_i64(&db, "max_concurrency").unwrap(), Some(50));
        // второй раз — обновление
        set(&db, "max_concurrency", "200").unwrap();
        assert_eq!(get_i64(&db, "max_concurrency").unwrap(), Some(200));
    }

    #[test]
    fn set_new_key() {
        let db = Db::in_memory().unwrap();
        assert_eq!(get(&db, "custom_key").unwrap(), None);
        set(&db, "custom_key", "hello").unwrap();
        assert_eq!(get(&db, "custom_key").unwrap().as_deref(), Some("hello"));
    }

    #[test]
    fn get_or_default() {
        let db = Db::in_memory().unwrap();
        assert_eq!(get_or(&db, "no_such_key", "dflt").unwrap(), "dflt");
    }

    #[test]
    fn delete_removes_row() {
        let db = Db::in_memory().unwrap();
        set(&db, "tmp", "x").unwrap();
        assert!(delete(&db, "tmp").unwrap());
        assert_eq!(get(&db, "tmp").unwrap(), None);
        // второй раз — false (уже нет)
        assert!(!delete(&db, "tmp").unwrap());
    }

    #[test]
    fn invalid_i64_value_errors() {
        let db = Db::in_memory().unwrap();
        set(&db, "num", "not-a-number").unwrap();
        let err = get_i64(&db, "num").unwrap_err();
        assert!(matches!(err, DbError::Invalid(_)));
    }

    #[test]
    fn list_all_includes_seeded() {
        let db = Db::in_memory().unwrap();
        let all = list_all(&db).unwrap();
        let keys: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"max_concurrency"));
        assert!(keys.contains(&"request_timeout_ms"));
        assert!(keys.contains(&"fallback_enabled"));
    }
}
