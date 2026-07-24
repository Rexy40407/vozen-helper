//! SQLite persistence with an intentionally small, auditable surface.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use helper_contracts::{EntitlementSnapshot, SessionClaims};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CaseRecord {
    pub id: i64,
    pub guild_id: String,
    pub kind: String,
    pub target_id: String,
    pub moderator_id: String,
    pub reason: String,
    pub duration_ms: Option<i64>,
    pub created_at: i64,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).context("open helper sqlite")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute_batch("CREATE TABLE IF NOT EXISTS helper_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS helper_sessions (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, guild_id TEXT NOT NULL, issued_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_seen_at TEXT NOT NULL, revoked_at TEXT); CREATE TABLE IF NOT EXISTS helper_entitlements (subject_id TEXT PRIMARY KEY, payload TEXT NOT NULL, fetched_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS helper_usage (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, quota_key TEXT NOT NULL, period TEXT NOT NULL, used INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,user_id,quota_key,period)); CREATE TABLE IF NOT EXISTS cases (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, target_id TEXT NOT NULL, moderator_id TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', duration_ms INTEGER, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_cases_guild_time ON cases(guild_id, created_at DESC); CREATE TABLE IF NOT EXISTS settings (guild_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(guild_id,key)); CREATE TABLE IF NOT EXISTS activity_log (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, user_id TEXT NOT NULL, user_tag TEXT, actor_id TEXT, detail TEXT NOT NULL DEFAULT '{}', created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_activity_guild_time ON activity_log(guild_id, created_at DESC); CREATE TABLE IF NOT EXISTS scheduled_actions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, target_id TEXT NOT NULL, execute_at INTEGER NOT NULL, payload TEXT NOT NULL DEFAULT '', case_id INTEGER); CREATE INDEX IF NOT EXISTS idx_scheduled_due ON scheduled_actions(execute_at); CREATE TABLE IF NOT EXISTS infractions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, target_id TEXT NOT NULL, weight INTEGER NOT NULL DEFAULT 1, source TEXT NOT NULL DEFAULT 'manual', created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS afk (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', since INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS tags (guild_id TEXT NOT NULL, name TEXT NOT NULL, content TEXT NOT NULL, author_id TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(guild_id,name)); CREATE TABLE IF NOT EXISTS levels (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, xp INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS stats (guild_id TEXT NOT NULL, date TEXT NOT NULL, messages INTEGER NOT NULL DEFAULT 0, joins INTEGER NOT NULL DEFAULT 0, leaves INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,date));")?;
        Ok(())
    }

    pub fn save_session(&self, claims: &SessionClaims) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT OR REPLACE INTO helper_sessions (id,user_id,guild_id,issued_at,expires_at,last_seen_at,revoked_at) VALUES (?1,?2,?3,?4,?5,?6,NULL)", params![claims.session_id.to_string(), claims.user_id, claims.guild_id, claims.issued_at.to_rfc3339(), claims.expires_at.to_rfc3339(), claims.last_seen_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn load_session(&self, id: Uuid) -> Result<Option<SessionClaims>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row = conn.query_row("SELECT user_id,guild_id,issued_at,expires_at,last_seen_at,revoked_at FROM helper_sessions WHERE id=?1", [id.to_string()], |r| {
            let revoked: Option<String> = r.get(5)?;
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, revoked))
        }).optional()?;
        let Some((user_id, guild_id, issued, expires, last_seen, revoked)) = row else {
            return Ok(None);
        };
        if revoked.is_some() {
            return Ok(None);
        }
        Ok(Some(SessionClaims {
            session_id: id,
            user_id,
            guild_id,
            issued_at: parse_dt(&issued)?,
            expires_at: parse_dt(&expires)?,
            last_seen_at: parse_dt(&last_seen)?,
        }))
    }

    pub fn revoke_session(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE helper_sessions SET revoked_at=?2 WHERE id=?1",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn save_entitlement(&self, snapshot: &EntitlementSnapshot) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT OR REPLACE INTO helper_entitlements (subject_id,payload,fetched_at) VALUES (?1,?2,?3)", params![snapshot.subject_id, serde_json::to_string(snapshot)?, snapshot.fetched_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn record_case(
        &self,
        guild_id: &str,
        kind: &str,
        target_id: &str,
        moderator_id: &str,
        reason: &str,
        duration_ms: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO cases(guild_id,type,target_id,moderator_id,reason,duration_ms,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![guild_id, kind, target_id, moderator_id, reason, duration_ms, Utc::now().timestamp_millis()])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_cases(&self, guild_id: &str, limit: u32) -> Result<Vec<CaseRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,type,target_id,moderator_id,reason,duration_ms,created_at FROM cases WHERE guild_id=?1 ORDER BY id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(200))], |row| {
            Ok(CaseRecord {
                id: row.get(0)?,
                guild_id: row.get(1)?,
                kind: row.get(2)?,
                target_id: row.get(3)?,
                moderator_id: row.get(4)?,
                reason: row.get(5)?,
                duration_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn consume_quota(
        &self,
        guild_id: &str,
        user_id: &str,
        key: &str,
        limit: u64,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let period = now.format("%Y-%m").to_string();
        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        let used: u64 = tx.query_row("SELECT used FROM helper_usage WHERE guild_id=?1 AND user_id=?2 AND quota_key=?3 AND period=?4", params![guild_id,user_id,key,period], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(0).try_into().unwrap_or(0);
        if used >= limit {
            return Ok(false);
        }
        tx.execute("INSERT INTO helper_usage(guild_id,user_id,quota_key,period,used) VALUES(?1,?2,?3,?4,1) ON CONFLICT(guild_id,user_id,quota_key,period) DO UPDATE SET used=used+1", params![guild_id,user_id,key,period])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn due_scheduled_actions(
        &self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,type,target_id FROM scheduled_actions WHERE execute_at<=?1 ORDER BY execute_at ASC LIMIT ?2")?;
        let rows = stmt.query_map(params![now_ms, i64::from(limit.min(100))], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_scheduled_action(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("DELETE FROM scheduled_actions WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn load_entitlement(&self, subject_id: &str) -> Result<Option<EntitlementSnapshot>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value: Option<String> = conn
            .query_row(
                "SELECT payload FROM helper_entitlements WHERE subject_id=?1",
                [subject_id],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).context("decode entitlement snapshot"))
            .transpose()
    }
}

fn parse_dt(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use helper_contracts::{EntitlementSnapshot, Plan, SessionClaims};

    #[test]
    fn schema_and_case_round_trip() {
        let store = Store::open(":memory:").unwrap();
        let id = store
            .record_case("guild", "warn", "user", "mod", "reason", None)
            .unwrap();
        assert_eq!(id, 1);
        let cases = store.recent_cases("guild", 10).unwrap();
        assert_eq!(cases[0].target_id, "user");
        assert!(
            store
                .consume_quota("guild", "user", "workflow_runs", 1, Utc::now())
                .unwrap()
        );
        assert!(
            !store
                .consume_quota("guild", "user", "workflow_runs", 1, Utc::now())
                .unwrap()
        );
    }

    #[test]
    fn sessions_revoke_and_entitlements_persist() {
        let store = Store::open(":memory:").unwrap();
        let now = Utc::now();
        let claims = SessionClaims {
            session_id: Uuid::new_v4(),
            user_id: "u".into(),
            guild_id: "g".into(),
            issued_at: now,
            expires_at: now + chrono::Duration::hours(1),
            last_seen_at: now,
        };
        store.save_session(&claims).unwrap();
        assert!(store.load_session(claims.session_id).unwrap().is_some());
        store.revoke_session(claims.session_id).unwrap();
        assert!(store.load_session(claims.session_id).unwrap().is_none());
        let mut entitlement = EntitlementSnapshot::free("u");
        entitlement.plan = Plan::Plus;
        store.save_entitlement(&entitlement).unwrap();
        assert_eq!(
            store.load_entitlement("u").unwrap().unwrap().plan,
            Plan::Plus
        );
    }
}
