//! CRUD для `check_sessions`.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{Db, DbError, DbResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Completed,
    Cancelled,
    Error,
}

impl SessionStatus {
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    pub fn from_sql(s: &str) -> DbResult<Self> {
        Ok(match s {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            "error" => Self::Error,
            other => return Err(DbError::Invalid(format!("status: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRow {
    pub id: i64,
    pub name: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub total_wallets: i64,
    pub checked_wallets: i64,
    pub status: SessionStatus,
}

/// Создать новую сессию со статусом `running`.
pub fn create_session(db: &Db, name: Option<&str>, total_wallets: i64) -> DbResult<i64> {
    let c = db.conn()?;
    c.execute(
        "INSERT INTO check_sessions (name, total_wallets, checked_wallets, status)
         VALUES (?1, ?2, 0, 'running')",
        params![name, total_wallets],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn get_session(db: &Db, id: i64) -> DbResult<Option<SessionRow>> {
    let c = db.conn()?;
    let row = c
        .query_row(
            "SELECT id, name, started_at, finished_at, total_wallets, checked_wallets, status
             FROM check_sessions WHERE id = ?1",
            params![id],
            |r| {
                let status: String = r.get(6)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    status,
                ))
            },
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((id, name, started_at, finished_at, total, checked, status)) => Ok(Some(SessionRow {
            id,
            name,
            started_at,
            finished_at,
            total_wallets: total,
            checked_wallets: checked,
            status: SessionStatus::from_sql(&status)?,
        })),
    }
}

pub fn update_progress(db: &Db, id: i64, checked_wallets: i64) -> DbResult<()> {
    let c = db.conn()?;
    c.execute(
        "UPDATE check_sessions SET checked_wallets = ?1 WHERE id = ?2",
        params![checked_wallets, id],
    )?;
    Ok(())
}

pub fn finish_session(db: &Db, id: i64, status: SessionStatus) -> DbResult<()> {
    let c = db.conn()?;
    let n = c.execute(
        "UPDATE check_sessions
            SET status = ?1,
                finished_at = datetime('now')
          WHERE id = ?2",
        params![status.as_sql(), id],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(format!("session {id}")));
    }
    Ok(())
}

pub fn list_sessions(db: &Db, limit: i64) -> DbResult<Vec<SessionRow>> {
    let c = db.conn()?;
    let mut stmt = c.prepare(
        "SELECT id, name, started_at, finished_at, total_wallets, checked_wallets, status
         FROM check_sessions ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            let status: String = r.get(6)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                status,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, name, started_at, finished_at, total, checked, st)| {
            Ok(SessionRow {
                id,
                name,
                started_at,
                finished_at,
                total_wallets: total,
                checked_wallets: checked,
                status: SessionStatus::from_sql(&st)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_get_session() {
        let db = Db::in_memory().unwrap();
        let id = create_session(&db, Some("batch-1"), 1000).unwrap();
        let s = get_session(&db, id).unwrap().unwrap();
        assert_eq!(s.name.as_deref(), Some("batch-1"));
        assert_eq!(s.total_wallets, 1000);
        assert_eq!(s.checked_wallets, 0);
        assert_eq!(s.status, SessionStatus::Running);
        assert!(s.finished_at.is_none());
    }

    #[test]
    fn progress_and_finish() {
        let db = Db::in_memory().unwrap();
        let id = create_session(&db, None, 10).unwrap();
        update_progress(&db, id, 5).unwrap();
        update_progress(&db, id, 10).unwrap();
        finish_session(&db, id, SessionStatus::Completed).unwrap();
        let s = get_session(&db, id).unwrap().unwrap();
        assert_eq!(s.checked_wallets, 10);
        assert_eq!(s.status, SessionStatus::Completed);
        assert!(s.finished_at.is_some());
    }

    #[test]
    fn finish_missing_session_errors() {
        let db = Db::in_memory().unwrap();
        let err = finish_session(&db, 9999, SessionStatus::Error).unwrap_err();
        assert!(matches!(err, DbError::NotFound(_)));
    }

    #[test]
    fn list_sessions_desc() {
        let db = Db::in_memory().unwrap();
        let a = create_session(&db, Some("a"), 1).unwrap();
        let b = create_session(&db, Some("b"), 1).unwrap();
        let list = list_sessions(&db, 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, b);
        assert_eq!(list[1].id, a);
    }
}
