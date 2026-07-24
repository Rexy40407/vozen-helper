//! SQLite persistence with an intentionally small, auditable surface.

use anyhow::{Context, Result, anyhow, bail};
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

/// Durable scheduler row kept as a tuple to preserve the runtime's small
/// dispatch surface. The fields are `(id, guild_id, action_type, target_id,
/// payload)`.
pub type ScheduledAction = (i64, String, String, String, String);

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct TagRecord {
    pub guild_id: String,
    pub name: String,
    pub content: String,
    pub author_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LevelRecord {
    pub guild_id: String,
    pub user_id: String,
    pub xp: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AfkRecord {
    pub guild_id: String,
    pub user_id: String,
    pub reason: String,
    pub since: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TicketRecord {
    pub guild_id: String,
    pub user_id: String,
    pub channel_id: String,
    pub status: String,
    pub claimed_by: Option<String>,
    pub category: String,
    pub priority: String,
    pub notes: String,
    pub csat: Option<i64>,
    pub created_at: i64,
    pub closed_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventRegistrationRecord {
    pub guild_id: String,
    pub event_id: String,
    pub user_id: String,
    pub status: String,
    pub created_at: i64,
    pub checked_in_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionGuildRecord {
    pub guild_id: String,
    pub name: String,
    pub permissions: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestionRecord {
    pub id: i64,
    pub guild_id: String,
    pub author_id: String,
    pub content: String,
    pub message_id: Option<String>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GiveawayRecord {
    pub id: i64,
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: Option<String>,
    pub prize: String,
    pub winners: i64,
    pub end_at: i64,
    pub ended: bool,
    pub required_role_id: Option<String>,
    pub host_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StarEntry {
    pub starboard_message_id: String,
    pub star_count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PollRecord {
    pub id: i64,
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: Option<String>,
    pub question: String,
    pub options: Vec<String>,
    pub end_at: i64,
    pub closed: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkflowRecord {
    pub id: i64,
    pub guild_id: String,
    pub name: String,
    pub trigger: String,
    pub condition: String,
    pub action: String,
    pub payload: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ConfigImportSummary {
    pub settings: usize,
    pub tags: usize,
    pub workflows: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuarantineRecord {
    pub guild_id: String,
    pub user_id: String,
    pub role_ids: Vec<String>,
    pub reason: String,
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
        conn.execute_batch("CREATE TABLE IF NOT EXISTS helper_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS helper_sessions (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, guild_id TEXT NOT NULL, issued_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_seen_at TEXT NOT NULL, revoked_at TEXT); CREATE TABLE IF NOT EXISTS helper_session_guilds (session_id TEXT NOT NULL, guild_id TEXT NOT NULL, name TEXT NOT NULL, permissions TEXT, PRIMARY KEY(session_id,guild_id)); CREATE TABLE IF NOT EXISTS helper_oauth_states (state_hash TEXT PRIMARY KEY, expires_at INTEGER NOT NULL, used_at INTEGER); CREATE TABLE IF NOT EXISTS helper_entitlements (subject_id TEXT PRIMARY KEY, payload TEXT NOT NULL, fetched_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS helper_usage (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, quota_key TEXT NOT NULL, period TEXT NOT NULL, used INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,user_id,quota_key,period)); CREATE TABLE IF NOT EXISTS cases (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, target_id TEXT NOT NULL, moderator_id TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', duration_ms INTEGER, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_cases_guild_time ON cases(guild_id, created_at DESC); CREATE TABLE IF NOT EXISTS settings (guild_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(guild_id,key)); CREATE TABLE IF NOT EXISTS activity_log (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, user_id TEXT NOT NULL, user_tag TEXT, actor_id TEXT, detail TEXT NOT NULL DEFAULT '{}', created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_activity_guild_time ON activity_log(guild_id, created_at DESC); CREATE TABLE IF NOT EXISTS scheduled_actions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, type TEXT NOT NULL, target_id TEXT NOT NULL, execute_at INTEGER NOT NULL, payload TEXT NOT NULL DEFAULT '', case_id INTEGER); CREATE INDEX IF NOT EXISTS idx_scheduled_due ON scheduled_actions(execute_at); CREATE TABLE IF NOT EXISTS infractions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, target_id TEXT NOT NULL, weight INTEGER NOT NULL DEFAULT 1, source TEXT NOT NULL DEFAULT 'manual', created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS afk (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', since INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS tags (guild_id TEXT NOT NULL, name TEXT NOT NULL, content TEXT NOT NULL, author_id TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(guild_id,name)); CREATE TABLE IF NOT EXISTS levels (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, xp INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS stats (guild_id TEXT NOT NULL, date TEXT NOT NULL, messages INTEGER NOT NULL DEFAULT 0, joins INTEGER NOT NULL DEFAULT 0, leaves INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,date));")?;
        // Keep the ticket schema compatible with the existing Node Helper DB.
        // The owner column is named `opener_id` there; changing it to `user_id`
        // would make an in-place Rust cutover fail on the live database.
        conn.execute_batch("CREATE TABLE IF NOT EXISTS tickets (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, opener_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'open', claimed_by TEXT, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_tickets_owner ON tickets(guild_id,opener_id,status); CREATE TABLE IF NOT EXISTS suggestions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, author_id TEXT NOT NULL, content TEXT NOT NULL, message_id TEXT, status TEXT NOT NULL DEFAULT 'pending', created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS suggestion_votes (suggestion_id INTEGER NOT NULL, user_id TEXT NOT NULL, vote INTEGER NOT NULL, PRIMARY KEY(suggestion_id,user_id)); CREATE TABLE IF NOT EXISTS giveaways (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, message_id TEXT, prize TEXT NOT NULL, winners INTEGER NOT NULL DEFAULT 1, end_at INTEGER NOT NULL, ended INTEGER NOT NULL DEFAULT 0, required_role_id TEXT, host_id TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS giveaway_entries (giveaway_id INTEGER NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY(giveaway_id,user_id)); CREATE TABLE IF NOT EXISTS polls (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, message_id TEXT, question TEXT NOT NULL, options TEXT NOT NULL, end_at INTEGER NOT NULL, closed INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS poll_votes (poll_id INTEGER NOT NULL, user_id TEXT NOT NULL, choice INTEGER NOT NULL, PRIMARY KEY(poll_id,user_id)); CREATE TABLE IF NOT EXISTS workflows (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, name TEXT NOT NULL, trigger TEXT NOT NULL, condition TEXT NOT NULL DEFAULT '', action TEXT NOT NULL, payload TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS workflow_runs (id INTEGER PRIMARY KEY AUTOINCREMENT, workflow_id INTEGER NOT NULL, guild_id TEXT NOT NULL, source_id TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_workflows_trigger ON workflows(guild_id,trigger,enabled); CREATE TABLE IF NOT EXISTS quarantine (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, role_ids TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS starboard (guild_id TEXT NOT NULL, original_message_id TEXT NOT NULL, starboard_message_id TEXT NOT NULL, star_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,original_message_id));")?;
        for (column, definition) in [
            ("category", "TEXT NOT NULL DEFAULT 'general'"),
            ("priority", "TEXT NOT NULL DEFAULT 'normal'"),
            ("notes", "TEXT NOT NULL DEFAULT ''"),
            ("csat", "INTEGER"),
            ("closed_at", "INTEGER"),
        ] {
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tickets') WHERE name=?1",
                [column],
                |row| row.get(0),
            )?;
            if exists == 0 {
                conn.execute(
                    &format!("ALTER TABLE tickets ADD COLUMN {column} {definition}"),
                    [],
                )?;
            }
        }
        conn.execute_batch("CREATE TABLE IF NOT EXISTS event_registrations (guild_id TEXT NOT NULL, event_id TEXT NOT NULL, user_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'registered', created_at INTEGER NOT NULL, checked_in_at INTEGER, PRIMARY KEY(guild_id,event_id,user_id)); CREATE INDEX IF NOT EXISTS idx_event_registrations_event ON event_registrations(guild_id,event_id,status);")?;
        Ok(())
    }

    pub fn save_session(&self, claims: &SessionClaims) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT OR REPLACE INTO helper_sessions (id,user_id,guild_id,issued_at,expires_at,last_seen_at,revoked_at) VALUES (?1,?2,?3,?4,?5,?6,NULL)", params![claims.session_id.to_string(), claims.user_id, claims.guild_id, claims.issued_at.to_rfc3339(), claims.expires_at.to_rfc3339(), claims.last_seen_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn replace_session_guilds(
        &self,
        session_id: Uuid,
        guilds: &[(String, String, Option<String>)],
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM helper_session_guilds WHERE session_id=?1",
            [session_id.to_string()],
        )?;
        for (guild_id, name, permissions) in guilds {
            tx.execute(
                "INSERT INTO helper_session_guilds(session_id,guild_id,name,permissions) VALUES(?1,?2,?3,?4)",
                params![session_id.to_string(), guild_id, name, permissions],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn session_guilds(&self, session_id: Uuid) -> Result<Vec<SessionGuildRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT guild_id,name,permissions FROM helper_session_guilds WHERE session_id=?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([session_id.to_string()], |row| {
            Ok(SessionGuildRecord {
                guild_id: row.get(0)?,
                name: row.get(1)?,
                permissions: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn register_oauth_state(&self, state_hash: &str, expires_at: i64) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "DELETE FROM helper_oauth_states WHERE expires_at < ?1 OR used_at < ?1 - 3600",
            [Utc::now().timestamp()],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO helper_oauth_states(state_hash,expires_at,used_at) VALUES(?1,?2,NULL)",
            params![state_hash, expires_at],
        )?;
        Ok(())
    }

    pub fn consume_oauth_state(&self, state_hash: &str, now: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE helper_oauth_states SET used_at=?2 WHERE state_hash=?1 AND used_at IS NULL AND expires_at>=?2",
            params![state_hash, now],
        )? > 0)
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

    pub fn cases_for_target(
        &self,
        guild_id: &str,
        target_id: &str,
        limit: u32,
    ) -> Result<Vec<CaseRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,type,target_id,moderator_id,reason,duration_ms,created_at FROM cases WHERE guild_id=?1 AND target_id=?2 ORDER BY id DESC LIMIT ?3")?;
        let rows = stmt.query_map(
            params![guild_id, target_id, i64::from(limit.min(200))],
            |row| {
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
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_case_reason(&self, guild_id: &str, case_id: i64, reason: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE cases SET reason=?3 WHERE id=?1 AND guild_id=?2",
            params![case_id, guild_id, reason],
        )? > 0)
    }

    pub fn consume_quota(
        &self,
        guild_id: &str,
        user_id: &str,
        key: &str,
        limit: u64,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        if limit == 0 {
            return Ok(false);
        }
        let period = now.format("%Y-%m").to_string();
        let conn = self.conn.lock().expect("store mutex poisoned");
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = conn.prepare_cached(
            "INSERT INTO helper_usage(guild_id,user_id,quota_key,period,used) VALUES(?1,?2,?3,?4,1) ON CONFLICT(guild_id,user_id,quota_key,period) DO UPDATE SET used=used+1 WHERE used < ?5",
        )?;
        let changed = statement.execute(params![guild_id, user_id, key, period, limit])?;
        Ok(changed > 0)
    }

    pub fn due_scheduled_actions(&self, now_ms: i64, limit: u32) -> Result<Vec<ScheduledAction>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,type,target_id,payload FROM scheduled_actions WHERE execute_at<=?1 ORDER BY execute_at ASC LIMIT ?2")?;
        let rows = stmt.query_map(params![now_ms, i64::from(limit.min(100))], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
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

    pub fn set_afk(&self, guild_id: &str, user_id: &str, reason: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO afk(guild_id,user_id,reason,since) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,user_id) DO UPDATE SET reason=excluded.reason,since=excluded.since",
            params![guild_id, user_id, reason, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn get_afk(&self, guild_id: &str, user_id: &str) -> Result<Option<AfkRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT guild_id,user_id,reason,since FROM afk WHERE guild_id=?1 AND user_id=?2",
                params![guild_id, user_id],
                |row| {
                    Ok(AfkRecord {
                        guild_id: row.get(0)?,
                        user_id: row.get(1)?,
                        reason: row.get(2)?,
                        since: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn clear_afk(&self, guild_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM afk WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
        )? > 0)
    }

    pub fn upsert_tag(
        &self,
        guild_id: &str,
        name: &str,
        content: &str,
        author_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO tags(guild_id,name,content,author_id,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(guild_id,name) DO UPDATE SET content=excluded.content,author_id=excluded.author_id,created_at=excluded.created_at",
            params![guild_id, name, content, author_id, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn get_tag(&self, guild_id: &str, name: &str) -> Result<Option<TagRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT guild_id,name,content,author_id,created_at FROM tags WHERE guild_id=?1 AND name=?2",
                params![guild_id, name],
                |row| {
                    Ok(TagRecord {
                        guild_id: row.get(0)?,
                        name: row.get(1)?,
                        content: row.get(2)?,
                        author_id: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn list_tags(&self, guild_id: &str, limit: u32) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT name FROM tags WHERE guild_id=?1 ORDER BY name LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(100))], |row| {
            row.get(0)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    pub fn delete_tag(&self, guild_id: &str, name: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM tags WHERE guild_id=?1 AND name=?2",
            params![guild_id, name],
        )? > 0)
    }

    pub fn get_setting(&self, guild_id: &str, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT value FROM settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, guild_id: &str, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at", params![guild_id, key, value, Utc::now().timestamp_millis()])?;
        Ok(())
    }

    /// Insert a new namespaced setting while enforcing its quota in the same
    /// SQLite transaction. Updates to an existing key are deliberately not
    /// counted as new entries.
    pub fn insert_setting_bounded(
        &self,
        guild_id: &str,
        key: &str,
        value: &str,
        prefix: &str,
        limit: u64,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT value FROM settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(false);
        }
        let pattern = format!("{prefix}%");
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM settings WHERE guild_id=?1 AND key LIKE ?2",
            params![guild_id, pattern],
            |row| row.get(0),
        )?;
        if count < 0 || count as u64 >= limit {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4)",
            params![guild_id, key, value, Utc::now().timestamp_millis()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn settings_with_prefix(
        &self,
        guild_id: &str,
        prefix: &str,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let pattern = format!("{prefix}%");
        let mut stmt = conn.prepare(
            "SELECT key,value FROM settings WHERE guild_id=?1 AND key LIKE ?2 ORDER BY updated_at DESC, key",
        )?;
        let rows = stmt.query_map(params![guild_id, pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_setting(&self, guild_id: &str, key: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM settings WHERE guild_id=?1 AND key=?2",
            params![guild_id, key],
        )? > 0)
    }

    pub fn count_settings_prefix(&self, guild_id: &str, prefix: &str) -> Result<u64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let pattern = format!("{prefix}%");
        let count = conn.query_row(
            "SELECT COUNT(*) FROM settings WHERE guild_id=?1 AND key LIKE ?2",
            params![guild_id, pattern],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count.try_into().unwrap_or(u64::MAX))
    }

    /// Export the guild-scoped configuration and durable module state without
    /// including Discord message bodies or session/entitlement secrets.
    pub fn export_guild(&self, guild_id: &str) -> Result<serde_json::Value> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut settings = Vec::new();
        let mut stmt = conn
            .prepare("SELECT key,value,updated_at FROM settings WHERE guild_id=?1 ORDER BY key")?;
        for row in stmt.query_map([guild_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })? {
            let (key, value, updated_at) = row?;
            let lower_key = key.to_ascii_lowercase();
            if ["secret", "token", "webhook", "credential"]
                .iter()
                .any(|needle| lower_key.contains(needle))
            {
                continue;
            }
            settings.push(serde_json::json!({
                "key": key,
                "value": value,
                "updatedAt": updated_at,
            }));
        }

        let mut tags = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT name,content,author_id,created_at FROM tags WHERE guild_id=?1 ORDER BY name",
        )?;
        for row in stmt.query_map([guild_id], |row| {
            Ok(serde_json::json!({
                "name": row.get::<_, String>(0)?,
                "content": row.get::<_, String>(1)?,
                "authorId": row.get::<_, String>(2)?,
                "createdAt": row.get::<_, i64>(3)?,
            }))
        })? {
            tags.push(row?);
        }

        let mut workflows = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id,name,trigger,condition,action,payload,enabled,created_at FROM workflows WHERE guild_id=?1 ORDER BY id",
        )?;
        for row in stmt.query_map([guild_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "trigger": row.get::<_, String>(2)?,
                "condition": row.get::<_, String>(3)?,
                "action": row.get::<_, String>(4)?,
                "payload": row.get::<_, String>(5)?,
                "enabled": row.get::<_, i64>(6)? != 0,
                "createdAt": row.get::<_, i64>(7)?,
            }))
        })? {
            workflows.push(row?);
        }

        Ok(serde_json::json!({
            "version": 1,
            "guildId": guild_id,
            "exportedAt": Utc::now().to_rfc3339(),
            "settings": settings,
            "tags": tags,
            "workflows": workflows,
        }))
    }

    /// Export user-associated data for the Discord privacy command. The query is
    /// always bounded to the authenticated guild and omits moderator identities
    /// from moderation cases.
    pub fn export_user(&self, guild_id: &str, user_id: &str) -> Result<serde_json::Value> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut cases = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id,type,target_id,reason,duration_ms,created_at FROM cases WHERE guild_id=?1 AND target_id=?2 ORDER BY id",
        )?;
        for row in stmt.query_map(params![guild_id, user_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "type": row.get::<_, String>(1)?,
                "targetId": row.get::<_, String>(2)?,
                "reason": row.get::<_, String>(3)?,
                "durationMs": row.get::<_, Option<i64>>(4)?,
                "createdAt": row.get::<_, i64>(5)?,
            }))
        })? {
            cases.push(row?);
        }
        let mut voluntary = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT 'afk' AS kind, reason, since FROM afk WHERE guild_id=?1 AND user_id=?2
             UNION ALL SELECT 'level' AS kind, CAST(xp AS TEXT), 0 FROM levels WHERE guild_id=?1 AND user_id=?2
             UNION ALL SELECT 'tag' AS kind, name, created_at FROM tags WHERE guild_id=?1 AND author_id=?2
             ORDER BY kind",
        )?;
        for row in stmt.query_map(params![guild_id, user_id], |row| {
            Ok(serde_json::json!({
                "kind": row.get::<_, String>(0)?,
                "value": row.get::<_, String>(1)?,
                "createdAt": row.get::<_, i64>(2)?,
            }))
        })? {
            voluntary.push(row?);
        }
        let mut suggestions = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id,content,status,created_at FROM suggestions WHERE guild_id=?1 AND author_id=?2 ORDER BY id",
        )?;
        for row in stmt.query_map(params![guild_id, user_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "content": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "createdAt": row.get::<_, i64>(3)?,
            }))
        })? {
            suggestions.push(row?);
        }
        let mut tickets = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id,channel_id,status,created_at FROM tickets WHERE guild_id=?1 AND opener_id=?2 ORDER BY id",
        )?;
        for row in stmt.query_map(params![guild_id, user_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "channelId": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "createdAt": row.get::<_, i64>(3)?,
            }))
        })? {
            tickets.push(row?);
        }
        let mut events = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT event_id,status,created_at,checked_in_at FROM event_registrations WHERE guild_id=?1 AND user_id=?2 ORDER BY event_id",
        )?;
        for row in stmt.query_map(params![guild_id, user_id], |row| {
            Ok(serde_json::json!({
                "eventId": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "createdAt": row.get::<_, i64>(2)?,
                "checkedInAt": row.get::<_, Option<i64>>(3)?,
            }))
        })? {
            events.push(row?);
        }
        Ok(serde_json::json!({
            "version": 1,
            "guildId": guild_id,
            "userId": user_id,
            "exportedAt": Utc::now().to_rfc3339(),
            "moderationCases": cases,
            "voluntary": voluntary,
            "suggestions": suggestions,
            "tickets": tickets,
            "eventRegistrations": events,
        }))
    }

    /// Erase voluntary user data while retaining moderation evidence.
    pub fn purge_user(&self, guild_id: &str, user_id: &str) -> Result<serde_json::Value> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let mut deleted = serde_json::Map::new();
        for (name, statement) in [
            (
                "suggestion_votes",
                "DELETE FROM suggestion_votes WHERE user_id=?1 AND suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?2)",
            ),
            (
                "giveaway_entries",
                "DELETE FROM giveaway_entries WHERE user_id=?1 AND giveaway_id IN (SELECT id FROM giveaways WHERE guild_id=?2)",
            ),
            (
                "poll_votes",
                "DELETE FROM poll_votes WHERE user_id=?1 AND poll_id IN (SELECT id FROM polls WHERE guild_id=?2)",
            ),
            (
                "scheduled_reminders",
                "DELETE FROM scheduled_actions WHERE guild_id=?2 AND type='reminder' AND target_id=?1",
            ),
            (
                "event_registrations",
                "DELETE FROM event_registrations WHERE guild_id=?2 AND user_id=?1",
            ),
            ("afk", "DELETE FROM afk WHERE guild_id=?2 AND user_id=?1"),
            (
                "levels",
                "DELETE FROM levels WHERE guild_id=?2 AND user_id=?1",
            ),
            (
                "tags",
                "DELETE FROM tags WHERE guild_id=?2 AND author_id=?1",
            ),
            (
                "suggestions",
                "DELETE FROM suggestions WHERE guild_id=?2 AND author_id=?1",
            ),
        ] {
            let count = tx.execute(statement, params![user_id, guild_id])?;
            if count > 0 {
                deleted.insert(name.to_string(), serde_json::json!(count));
            }
        }
        tx.commit()?;
        Ok(serde_json::json!({
            "deleted": deleted,
            "retained": ["moderationCases", "infractions", "quarantine"],
        }))
    }

    /// Import only the versioned, guild-scoped configuration produced by
    /// `export_guild`. The target guild always comes from the authenticated
    /// caller; the source `guildId` in the document is informational only.
    /// Secrets and unsupported automation shapes are rejected before any write.
    pub fn import_guild_config(
        &self,
        guild_id: &str,
        document: &serde_json::Value,
    ) -> Result<ConfigImportSummary> {
        if guild_id.trim().is_empty() || guild_id.len() > 64 {
            bail!("invalid_target_guild");
        }
        if document.to_string().len() > 1_000_000 {
            bail!("config_too_large");
        }
        let object = document
            .as_object()
            .ok_or_else(|| anyhow!("invalid_config_document"))?;
        if object.get("version").and_then(serde_json::Value::as_i64) != Some(1) {
            bail!("unsupported_config_version");
        }
        let settings = object
            .get("settings")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("invalid_settings"))?;
        let tags = object
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("invalid_tags"))?;
        let workflows = object
            .get("workflows")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("invalid_workflows"))?;
        if settings.len() > 200 || tags.len() > 100 || workflows.len() > 100 {
            bail!("config_limits_exceeded");
        }

        let secret_needles = ["secret", "token", "webhook", "credential"];
        let mut parsed_settings = Vec::with_capacity(settings.len());
        for item in settings {
            let key = item
                .get("key")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_setting_key"))?
                .trim()
                .to_string();
            let value = item
                .get("value")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_setting_value"))?
                .to_string();
            let lower_key = key.to_ascii_lowercase();
            if !(1..=100).contains(&key.len()) || value.len() > 4_000 {
                bail!("invalid_setting_bounds");
            }
            if secret_needles
                .iter()
                .any(|needle| lower_key.contains(needle))
            {
                bail!("secret_setting_rejected");
            }
            parsed_settings.push((key, value));
        }

        let mut parsed_tags = Vec::with_capacity(tags.len());
        for item in tags {
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_tag_name"))?
                .trim()
                .to_string();
            let content = item
                .get("content")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_tag_content"))?
                .to_string();
            let author_id = item
                .get("authorId")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_tag_author"))?
                .trim()
                .to_string();
            if !(1..=32).contains(&name.len())
                || !(1..=2_000).contains(&content.len())
                || !(1..=64).contains(&author_id.len())
            {
                bail!("invalid_tag_bounds");
            }
            parsed_tags.push((name, content, author_id));
        }

        let mut parsed_workflows = Vec::with_capacity(workflows.len());
        for item in workflows {
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_workflow_name"))?
                .trim()
                .to_string();
            let trigger = item
                .get("trigger")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_workflow_trigger"))?
                .to_string();
            let condition = item
                .get("condition")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let action = item
                .get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_workflow_action"))?
                .to_string();
            let payload = item
                .get("payload")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_workflow_payload"))?
                .trim()
                .to_string();
            let enabled = item
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            if !(1..=50).contains(&name.len())
                || trigger != "message"
                || action != "reply"
                || condition.len() > 200
                || !(1..=1_000).contains(&payload.len())
            {
                bail!("unsupported_workflow");
            }
            parsed_workflows.push((name, condition, payload, enabled));
        }

        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        for (key, value) in &parsed_settings {
            tx.execute(
                "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                params![guild_id, key, value, Utc::now().timestamp_millis()],
            )?;
        }
        for (name, content, author_id) in &parsed_tags {
            tx.execute(
                "INSERT INTO tags(guild_id,name,content,author_id,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(guild_id,name) DO UPDATE SET content=excluded.content,author_id=excluded.author_id,created_at=excluded.created_at",
                params![guild_id, name, content, author_id, Utc::now().timestamp_millis()],
            )?;
        }
        for (name, condition, payload, enabled) in &parsed_workflows {
            tx.execute(
                "DELETE FROM workflows WHERE guild_id=?1 AND name=?2 AND trigger='message'",
                params![guild_id, name],
            )?;
            tx.execute(
                "INSERT INTO workflows(guild_id,name,trigger,condition,action,payload,enabled,created_at) VALUES(?1,?2,'message',?3,'reply',?4,?5,?6)",
                params![guild_id, name, condition, payload, i64::from(*enabled), Utc::now().timestamp_millis()],
            )?;
        }
        tx.commit()?;
        Ok(ConfigImportSummary {
            settings: parsed_settings.len(),
            tags: parsed_tags.len(),
            workflows: parsed_workflows.len(),
        })
    }

    /// Erase all guild-scoped operational data. User entitlements and login
    /// sessions are intentionally outside this operation's scope.
    pub fn purge_guild(&self, guild_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        for statement in [
            "DELETE FROM suggestion_votes WHERE suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?1)",
            "DELETE FROM suggestions WHERE guild_id=?1",
            "DELETE FROM giveaway_entries WHERE giveaway_id IN (SELECT id FROM giveaways WHERE guild_id=?1)",
            "DELETE FROM giveaways WHERE guild_id=?1",
            "DELETE FROM poll_votes WHERE poll_id IN (SELECT id FROM polls WHERE guild_id=?1)",
            "DELETE FROM polls WHERE guild_id=?1",
            "DELETE FROM workflow_runs WHERE guild_id=?1",
            "DELETE FROM workflows WHERE guild_id=?1",
            "DELETE FROM cases WHERE guild_id=?1",
            "DELETE FROM activity_log WHERE guild_id=?1",
            "DELETE FROM scheduled_actions WHERE guild_id=?1",
            "DELETE FROM infractions WHERE guild_id=?1",
            "DELETE FROM afk WHERE guild_id=?1",
            "DELETE FROM tags WHERE guild_id=?1",
            "DELETE FROM levels WHERE guild_id=?1",
            "DELETE FROM stats WHERE guild_id=?1",
            "DELETE FROM tickets WHERE guild_id=?1",
            "DELETE FROM quarantine WHERE guild_id=?1",
            "DELETE FROM starboard WHERE guild_id=?1",
            "DELETE FROM helper_usage WHERE guild_id=?1",
            "DELETE FROM settings WHERE guild_id=?1",
        ] {
            tx.execute(statement, [guild_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_xp(&self, guild_id: &str, user_id: &str, amount: i64) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO levels(guild_id,user_id,xp) VALUES(?1,?2,?3) ON CONFLICT(guild_id,user_id) DO UPDATE SET xp=xp+excluded.xp",
            params![guild_id, user_id, amount.max(0)],
        )?;
        Ok(conn.query_row(
            "SELECT xp FROM levels WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
            |row| row.get(0),
        )?)
    }

    pub fn level_for(&self, guild_id: &str, user_id: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT xp FROM levels WHERE guild_id=?1 AND user_id=?2",
                params![guild_id, user_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn top_levels(&self, guild_id: &str, limit: u32) -> Result<Vec<LevelRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT guild_id,user_id,xp FROM levels WHERE guild_id=?1 ORDER BY xp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(25))], |row| {
            Ok(LevelRecord {
                guild_id: row.get(0)?,
                user_id: row.get(1)?,
                xp: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn record_message(&self, guild_id: &str, day: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO stats(guild_id,date,messages,joins,leaves) VALUES(?1,?2,1,0,0) ON CONFLICT(guild_id,date) DO UPDATE SET messages=messages+1", params![guild_id, day])?;
        Ok(())
    }

    pub fn record_join(&self, guild_id: &str, day: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO stats(guild_id,date,messages,joins,leaves) VALUES(?1,?2,0,1,0) ON CONFLICT(guild_id,date) DO UPDATE SET joins=joins+1", params![guild_id, day])?;
        Ok(())
    }

    pub fn record_leave(&self, guild_id: &str, day: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO stats(guild_id,date,messages,joins,leaves) VALUES(?1,?2,0,0,1) ON CONFLICT(guild_id,date) DO UPDATE SET leaves=leaves+1", params![guild_id, day])?;
        Ok(())
    }

    pub fn stats_for(&self, guild_id: &str, limit: u32) -> Result<Vec<(String, i64, i64, i64)>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT date,messages,joins,leaves FROM stats WHERE guild_id=?1 ORDER BY date DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(365))], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn schedule(
        &self,
        guild_id: &str,
        target_id: &str,
        execute_at: i64,
        payload: &str,
    ) -> Result<i64> {
        self.schedule_typed(guild_id, "reminder", target_id, execute_at, payload)
    }

    pub fn schedule_typed(
        &self,
        guild_id: &str,
        action_type: &str,
        target_id: &str,
        execute_at: i64,
        payload: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO scheduled_actions(guild_id,type,target_id,execute_at,payload,case_id) VALUES(?1,?2,?3,?4,?5,NULL)", params![guild_id, action_type, target_id, execute_at, payload])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn open_ticket(&self, guild_id: &str, user_id: &str, channel_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO tickets(guild_id,channel_id,opener_id,status,claimed_by,created_at) VALUES(?1,?3,?2,'open',NULL,?4)",
            params![guild_id, user_id, channel_id, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn active_ticket_for_user(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<TicketRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row(
            "SELECT guild_id,opener_id,channel_id,status,claimed_by,category,priority,notes,csat,created_at,closed_at FROM tickets WHERE guild_id=?1 AND opener_id=?2 AND status='open' ORDER BY created_at DESC LIMIT 1",
            params![guild_id, user_id],
            |row| Ok(TicketRecord { guild_id: row.get(0)?, user_id: row.get(1)?, channel_id: row.get(2)?, status: row.get(3)?, claimed_by: row.get(4)?, category: row.get(5)?, priority: row.get(6)?, notes: row.get(7)?, csat: row.get(8)?, created_at: row.get(9)?, closed_at: row.get(10)? }),
        ).optional()?)
    }

    pub fn ticket_by_channel(&self, channel_id: &str) -> Result<Option<TicketRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row(
            "SELECT guild_id,opener_id,channel_id,status,claimed_by,category,priority,notes,csat,created_at,closed_at FROM tickets WHERE channel_id=?1",
            [channel_id],
            |row| Ok(TicketRecord { guild_id: row.get(0)?, user_id: row.get(1)?, channel_id: row.get(2)?, status: row.get(3)?, claimed_by: row.get(4)?, category: row.get(5)?, priority: row.get(6)?, notes: row.get(7)?, csat: row.get(8)?, created_at: row.get(9)?, closed_at: row.get(10)? }),
        ).optional()?)
    }

    pub fn claim_ticket(&self, channel_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET claimed_by=?2 WHERE channel_id=?1 AND status='open'",
            params![channel_id, user_id],
        )? > 0)
    }

    pub fn close_ticket(&self, channel_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET status='closed',closed_at=?2 WHERE channel_id=?1 AND status='open'",
            params![channel_id, Utc::now().timestamp_millis()],
        )? > 0)
    }

    pub fn reopen_ticket(&self, channel_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET status='open',closed_at=NULL WHERE channel_id=?1 AND status='closed'",
            [channel_id],
        )? > 0)
    }

    pub fn set_ticket_priority(&self, channel_id: &str, priority: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET priority=?2 WHERE channel_id=?1",
            params![channel_id, priority],
        )? > 0)
    }

    pub fn set_ticket_category(&self, channel_id: &str, category: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET category=?2 WHERE channel_id=?1",
            params![channel_id, category],
        )? > 0)
    }

    pub fn set_ticket_notes(&self, channel_id: &str, notes: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET notes=?2 WHERE channel_id=?1",
            params![channel_id, notes],
        )? > 0)
    }

    pub fn set_ticket_csat(&self, channel_id: &str, score: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE tickets SET csat=?2 WHERE channel_id=?1 AND status='closed'",
            params![channel_id, score],
        )? > 0)
    }

    pub fn register_event(&self, guild_id: &str, event_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "INSERT OR IGNORE INTO event_registrations(guild_id,event_id,user_id,status,created_at,checked_in_at) VALUES(?1,?2,?3,'registered',?4,NULL)",
            params![guild_id, event_id, user_id, Utc::now().timestamp_millis()],
        )? > 0)
    }

    pub fn register_event_with_capacity(
        &self,
        guild_id: &str,
        event_id: &str,
        user_id: &str,
        capacity: Option<u64>,
    ) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        let exists: Option<String> = tx
            .query_row(
                "SELECT status FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND user_id=?3",
                params![guild_id, event_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_some() {
            return Ok(None);
        }
        let registered: u64 = tx
            .query_row(
                "SELECT COUNT(*) FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND status IN ('registered','checked_in')",
                params![guild_id, event_id],
                |row| row.get::<_, i64>(0),
            )?
            .try_into()
            .unwrap_or(u64::MAX);
        let status = if capacity.is_some_and(|limit| limit > 0 && registered >= limit) {
            "waitlisted"
        } else {
            "registered"
        };
        tx.execute(
            "INSERT INTO event_registrations(guild_id,event_id,user_id,status,created_at,checked_in_at) VALUES(?1,?2,?3,?4,?5,NULL)",
            params![guild_id, event_id, user_id, status, Utc::now().timestamp_millis()],
        )?;
        tx.commit()?;
        Ok(Some(status.to_string()))
    }

    pub fn remove_event_registration(
        &self,
        guild_id: &str,
        event_id: &str,
        user_id: &str,
    ) -> Result<(bool, Option<String>)> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND user_id=?3",
                params![guild_id, event_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(status) = status else {
            return Ok((false, None));
        };
        tx.execute(
            "DELETE FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND user_id=?3",
            params![guild_id, event_id, user_id],
        )?;
        let promoted = if status == "registered" || status == "checked_in" {
            let next: Option<(i64, String)> = tx
                .query_row(
                    "SELECT rowid,user_id FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND status='waitlisted' ORDER BY created_at ASC,rowid ASC LIMIT 1",
                    params![guild_id, event_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((rowid, user_id)) = next {
                tx.execute(
                    "UPDATE event_registrations SET status='registered' WHERE rowid=?1",
                    [rowid],
                )?;
                Some(user_id)
            } else {
                None
            }
        } else {
            None
        };
        tx.commit()?;
        Ok((true, promoted))
    }

    pub fn event_registration(
        &self,
        guild_id: &str,
        event_id: &str,
        user_id: &str,
    ) -> Result<Option<EventRegistrationRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT guild_id,event_id,user_id,status,created_at,checked_in_at FROM event_registrations WHERE guild_id=?1 AND event_id=?2 AND user_id=?3",
                params![guild_id, event_id, user_id],
                |row| {
                    Ok(EventRegistrationRecord {
                        guild_id: row.get(0)?,
                        event_id: row.get(1)?,
                        user_id: row.get(2)?,
                        status: row.get(3)?,
                        created_at: row.get(4)?,
                        checked_in_at: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn tickets_for_guild(&self, guild_id: &str, limit: u32) -> Result<Vec<TicketRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT guild_id,opener_id,channel_id,status,claimed_by,category,priority,notes,csat,created_at,closed_at FROM tickets WHERE guild_id=?1 ORDER BY created_at DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(200))], |row| {
            Ok(TicketRecord {
                guild_id: row.get(0)?,
                user_id: row.get(1)?,
                channel_id: row.get(2)?,
                status: row.get(3)?,
                claimed_by: row.get(4)?,
                category: row.get(5)?,
                priority: row.get(6)?,
                notes: row.get(7)?,
                csat: row.get(8)?,
                created_at: row.get(9)?,
                closed_at: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn check_in_event(&self, guild_id: &str, event_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE event_registrations SET status='checked_in',checked_in_at=?4 WHERE guild_id=?1 AND event_id=?2 AND user_id=?3 AND status='registered'",
            params![guild_id, event_id, user_id, Utc::now().timestamp_millis()],
        )? > 0)
    }

    pub fn event_registrations(
        &self,
        guild_id: &str,
        event_id: &str,
        limit: u32,
    ) -> Result<Vec<EventRegistrationRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT guild_id,event_id,user_id,status,created_at,checked_in_at FROM event_registrations WHERE guild_id=?1 AND event_id=?2 ORDER BY created_at ASC LIMIT ?3")?;
        let rows = stmt.query_map(
            params![guild_id, event_id, i64::from(limit.min(100))],
            |row| {
                Ok(EventRegistrationRecord {
                    guild_id: row.get(0)?,
                    event_id: row.get(1)?,
                    user_id: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    checked_in_at: row.get(5)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_suggestion(&self, guild_id: &str, author_id: &str, content: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO suggestions(guild_id,author_id,content,status,created_at) VALUES(?1,?2,?3,'pending',?4)",
            params![guild_id, author_id, content, Utc::now().timestamp_millis()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_suggestion_message(&self, id: i64, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE suggestions SET message_id=?2 WHERE id=?1",
            params![id, message_id],
        )?;
        Ok(())
    }

    pub fn suggestion(&self, id: i64) -> Result<Option<SuggestionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row(
            "SELECT id,guild_id,author_id,content,message_id,status,created_at FROM suggestions WHERE id=?1",
            [id],
            |row| Ok(SuggestionRecord {
                id: row.get(0)?, guild_id: row.get(1)?, author_id: row.get(2)?,
                content: row.get(3)?, message_id: row.get(4)?, status: row.get(5)?, created_at: row.get(6)?,
            }),
        ).optional()?)
    }

    pub fn set_suggestion_status(&self, guild_id: &str, id: i64, status: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE suggestions SET status=?3 WHERE id=?1 AND guild_id=?2",
            params![id, guild_id, status],
        )? > 0)
    }

    pub fn vote_suggestion(&self, id: i64, user_id: &str, vote: i32) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO suggestion_votes(suggestion_id,user_id,vote) VALUES(?1,?2,?3) ON CONFLICT(suggestion_id,user_id) DO UPDATE SET vote=excluded.vote",
            params![id, user_id, vote.clamp(-1, 1)],
        )?;
        Ok(())
    }

    pub fn suggestion_votes(&self, id: i64) -> Result<(i64, i64)> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT COALESCE(SUM(vote=1),0),COALESCE(SUM(vote=-1),0) FROM suggestion_votes WHERE suggestion_id=?1")?;
        Ok(stmt.query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_giveaway(
        &self,
        guild_id: &str,
        channel_id: &str,
        prize: &str,
        winners: i64,
        end_at: i64,
        required_role_id: Option<&str>,
        host_id: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO giveaways(guild_id,channel_id,prize,winners,end_at,required_role_id,host_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![guild_id, channel_id, prize, winners.clamp(1, 20), end_at, required_role_id, host_id, Utc::now().timestamp_millis()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_giveaway_message(&self, id: i64, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE giveaways SET message_id=?2 WHERE id=?1",
            params![id, message_id],
        )?;
        Ok(())
    }

    pub fn giveaway(&self, id: i64) -> Result<Option<GiveawayRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row(
            "SELECT id,guild_id,channel_id,message_id,prize,winners,end_at,ended,required_role_id,host_id,created_at FROM giveaways WHERE id=?1",
            [id],
            |row| Ok(GiveawayRecord {
                id: row.get(0)?, guild_id: row.get(1)?, channel_id: row.get(2)?, message_id: row.get(3)?,
                prize: row.get(4)?, winners: row.get(5)?, end_at: row.get(6)?, ended: row.get::<_, i64>(7)? != 0,
                required_role_id: row.get(8)?, host_id: row.get(9)?, created_at: row.get(10)?,
            }),
        ).optional()?)
    }

    pub fn active_giveaways(&self, guild_id: &str, limit: u32) -> Result<Vec<GiveawayRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,channel_id,message_id,prize,winners,end_at,ended,required_role_id,host_id,created_at FROM giveaways WHERE guild_id=?1 AND ended=0 ORDER BY end_at LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(50))], |row| {
            Ok(GiveawayRecord {
                id: row.get(0)?,
                guild_id: row.get(1)?,
                channel_id: row.get(2)?,
                message_id: row.get(3)?,
                prize: row.get(4)?,
                winners: row.get(5)?,
                end_at: row.get(6)?,
                ended: row.get::<_, i64>(7)? != 0,
                required_role_id: row.get(8)?,
                host_id: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn end_giveaway(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("UPDATE giveaways SET ended=1 WHERE id=?1 AND ended=0", [id])? > 0)
    }

    pub fn add_giveaway_entry(&self, id: i64, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "INSERT OR IGNORE INTO giveaway_entries(giveaway_id,user_id) VALUES(?1,?2)",
            params![id, user_id],
        )? > 0)
    }

    pub fn remove_giveaway_entry(&self, id: i64, user_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "DELETE FROM giveaway_entries WHERE giveaway_id=?1 AND user_id=?2",
            params![id, user_id],
        )?;
        Ok(())
    }

    pub fn giveaway_entries(&self, id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT user_id FROM giveaway_entries WHERE giveaway_id=?1 ORDER BY user_id",
        )?;
        let rows = stmt.query_map([id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn star_entry(&self, guild_id: &str, original_id: &str) -> Result<Option<StarEntry>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row("SELECT starboard_message_id,star_count FROM starboard WHERE guild_id=?1 AND original_message_id=?2", params![guild_id, original_id], |row| Ok(StarEntry { starboard_message_id: row.get(0)?, star_count: row.get(1)? })).optional()?)
    }

    pub fn upsert_star_entry(
        &self,
        guild_id: &str,
        original_id: &str,
        board_message_id: &str,
        count: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO starboard(guild_id,original_message_id,starboard_message_id,star_count) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,original_message_id) DO UPDATE SET starboard_message_id=excluded.starboard_message_id,star_count=excluded.star_count", params![guild_id, original_id, board_message_id, count])?;
        Ok(())
    }

    pub fn delete_star_entry(&self, guild_id: &str, original_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "DELETE FROM starboard WHERE guild_id=?1 AND original_message_id=?2",
            params![guild_id, original_id],
        )?;
        Ok(())
    }

    pub fn create_poll(
        &self,
        guild_id: &str,
        channel_id: &str,
        question: &str,
        options: &[String],
        end_at: i64,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO polls(guild_id,channel_id,question,options,end_at,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![guild_id, channel_id, question, serde_json::to_string(options)?, end_at, Utc::now().timestamp_millis()])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_poll_message(&self, id: i64, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE polls SET message_id=?2 WHERE id=?1",
            params![id, message_id],
        )?;
        Ok(())
    }

    pub fn poll(&self, id: i64) -> Result<Option<PollRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row("SELECT id,guild_id,channel_id,message_id,question,options,end_at,closed,created_at FROM polls WHERE id=?1", [id], |row| {
            let options: String = row.get(5)?;
            Ok(PollRecord { id: row.get(0)?, guild_id: row.get(1)?, channel_id: row.get(2)?, message_id: row.get(3)?, question: row.get(4)?, options: serde_json::from_str(&options).unwrap_or_default(), end_at: row.get(6)?, closed: row.get::<_, i64>(7)? != 0, created_at: row.get(8)? })
        }).optional()?)
    }

    pub fn vote_poll(&self, id: i64, user_id: &str, choice: usize) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO poll_votes(poll_id,user_id,choice) VALUES(?1,?2,?3) ON CONFLICT(poll_id,user_id) DO UPDATE SET choice=excluded.choice", params![id, user_id, choice as i64])?;
        Ok(())
    }

    pub fn poll_counts(&self, id: i64, choices: usize) -> Result<Vec<i64>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut counts = vec![0_i64; choices];
        let mut stmt = conn
            .prepare("SELECT choice,COUNT(*) FROM poll_votes WHERE poll_id=?1 GROUP BY choice")?;
        let rows = stmt.query_map([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (choice, count) = row?;
            if let Some(slot) = counts.get_mut(choice.max(0) as usize) {
                *slot = count;
            }
        }
        Ok(counts)
    }

    pub fn close_poll(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("UPDATE polls SET closed=1 WHERE id=?1 AND closed=0", [id])? > 0)
    }

    pub fn create_workflow(
        &self,
        guild_id: &str,
        name: &str,
        trigger: &str,
        condition: &str,
        action: &str,
        payload: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO workflows(guild_id,name,trigger,condition,action,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![guild_id, name, trigger, condition, action, payload, Utc::now().timestamp_millis()])?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_workflow_bounded(
        &self,
        guild_id: &str,
        name: &str,
        trigger: &str,
        condition: &str,
        action: &str,
        payload: &str,
        limit: u64,
    ) -> Result<Option<i64>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        let count: u64 = tx
            .query_row(
                "SELECT COUNT(*) FROM workflows WHERE guild_id=?1",
                [guild_id],
                |row| row.get::<_, i64>(0),
            )?
            .try_into()
            .unwrap_or(u64::MAX);
        if count >= limit {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO workflows(guild_id,name,trigger,condition,action,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![guild_id, name, trigger, condition, action, payload, Utc::now().timestamp_millis()],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn workflows(&self, guild_id: &str, limit: u32) -> Result<Vec<WorkflowRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,name,trigger,condition,action,payload,enabled,created_at FROM workflows WHERE guild_id=?1 ORDER BY id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(100))], |row| {
            Ok(WorkflowRecord {
                id: row.get(0)?,
                guild_id: row.get(1)?,
                name: row.get(2)?,
                trigger: row.get(3)?,
                condition: row.get(4)?,
                action: row.get(5)?,
                payload: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
                created_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn active_workflows(&self, guild_id: &str, trigger: &str) -> Result<Vec<WorkflowRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,name,trigger,condition,action,payload,enabled,created_at FROM workflows WHERE guild_id=?1 AND trigger=?2 AND enabled=1 ORDER BY id LIMIT 25")?;
        let rows = stmt.query_map(params![guild_id, trigger], |row| {
            Ok(WorkflowRecord {
                id: row.get(0)?,
                guild_id: row.get(1)?,
                name: row.get(2)?,
                trigger: row.get(3)?,
                condition: row.get(4)?,
                action: row.get(5)?,
                payload: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
                created_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn workflow(&self, guild_id: &str, id: i64) -> Result<Option<WorkflowRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT id,guild_id,name,trigger,condition,action,payload,enabled,created_at FROM workflows WHERE guild_id=?1 AND id=?2",
                params![guild_id, id],
                |row| {
                    Ok(WorkflowRecord {
                        id: row.get(0)?,
                        guild_id: row.get(1)?,
                        name: row.get(2)?,
                        trigger: row.get(3)?,
                        condition: row.get(4)?,
                        action: row.get(5)?,
                        payload: row.get(6)?,
                        enabled: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_workflow_enabled(&self, guild_id: &str, id: i64, enabled: bool) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "UPDATE workflows SET enabled=?3 WHERE guild_id=?1 AND id=?2",
            params![guild_id, id, i64::from(enabled)],
        )? > 0)
    }

    pub fn delete_workflow(&self, guild_id: &str, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM workflows WHERE guild_id=?1 AND id=?2",
            params![guild_id, id],
        )? > 0)
    }

    pub fn record_workflow_run(
        &self,
        workflow_id: i64,
        guild_id: &str,
        source_id: &str,
    ) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("INSERT INTO workflow_runs(workflow_id,guild_id,source_id,created_at) SELECT ?1,?2,?3,?4 WHERE NOT EXISTS (SELECT 1 FROM workflow_runs WHERE workflow_id=?1 AND guild_id=?2 AND source_id=?3)", params![workflow_id, guild_id, source_id, Utc::now().timestamp_millis()])? > 0)
    }

    pub fn save_quarantine(
        &self,
        guild_id: &str,
        user_id: &str,
        role_ids: &[String],
        reason: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO quarantine(guild_id,user_id,role_ids,reason,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(guild_id,user_id) DO UPDATE SET role_ids=excluded.role_ids,reason=excluded.reason,created_at=excluded.created_at", params![guild_id, user_id, serde_json::to_string(role_ids)?, reason, Utc::now().timestamp_millis()])?;
        Ok(())
    }

    pub fn get_quarantine(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<QuarantineRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.query_row("SELECT guild_id,user_id,role_ids,reason,created_at FROM quarantine WHERE guild_id=?1 AND user_id=?2", params![guild_id, user_id], |row| {
            let raw: String = row.get(2)?;
            Ok(QuarantineRecord { guild_id: row.get(0)?, user_id: row.get(1)?, role_ids: serde_json::from_str(&raw).unwrap_or_default(), reason: row.get(3)?, created_at: row.get(4)? })
        }).optional()?)
    }

    pub fn clear_quarantine(&self, guild_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM quarantine WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
        )? > 0)
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
        assert_eq!(
            store.cases_for_target("guild", "user", 10).unwrap().len(),
            1
        );
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
        store.register_oauth_state("state", 10).unwrap();
        assert!(store.consume_oauth_state("state", 10).unwrap());
        assert!(!store.consume_oauth_state("state", 10).unwrap());
        store.register_oauth_state("expired", 10).unwrap();
        assert!(!store.consume_oauth_state("expired", 11).unwrap());
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
        store
            .replace_session_guilds(
                claims.session_id,
                &[("g".into(), "Guild".into(), Some("8".into()))],
            )
            .unwrap();
        assert_eq!(
            store.session_guilds(claims.session_id).unwrap()[0].name,
            "Guild"
        );
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

    #[test]
    fn community_state_and_due_jobs_round_trip() {
        let store = Store::open(":memory:").unwrap();
        store.set_afk("g", "u", "away").unwrap();
        assert_eq!(store.get_afk("g", "u").unwrap().unwrap().reason, "away");
        assert!(store.clear_afk("g", "u").unwrap());
        store.upsert_tag("g", "rules", "be kind", "u").unwrap();
        assert_eq!(
            store.get_tag("g", "rules").unwrap().unwrap().content,
            "be kind"
        );
        assert_eq!(store.add_xp("g", "u", 5).unwrap(), 5);
        assert_eq!(store.level_for("g", "u").unwrap(), 5);
        let id = store
            .schedule("g", "u", 1, r#"{"channel_id":"2","text":"hello"}"#)
            .unwrap();
        let jobs = store.due_scheduled_actions(1, 10).unwrap();
        assert_eq!(jobs[0].0, id);
        assert_eq!(jobs[0].4, r#"{"channel_id":"2","text":"hello"}"#);
        store.open_ticket("g", "u", "20").unwrap();
        assert!(store.close_ticket("20").unwrap());
        assert_eq!(
            store.ticket_by_channel("20").unwrap().unwrap().status,
            "closed"
        );
        assert!(store.reopen_ticket("20").unwrap());
        assert_eq!(
            store.ticket_by_channel("20").unwrap().unwrap().status,
            "open"
        );
    }

    #[test]
    fn bounded_settings_are_atomic_and_guild_scoped() {
        let store = Store::open(":memory:").unwrap();
        assert!(
            store
                .insert_setting_bounded(
                    "guild-a",
                    "studio.template.one",
                    "{}",
                    "studio.template.",
                    1
                )
                .unwrap()
        );
        assert!(
            !store
                .insert_setting_bounded(
                    "guild-a",
                    "studio.template.two",
                    "{}",
                    "studio.template.",
                    1
                )
                .unwrap()
        );
        assert!(
            store
                .insert_setting_bounded(
                    "guild-b",
                    "studio.template.two",
                    "{}",
                    "studio.template.",
                    1
                )
                .unwrap()
        );
        assert_eq!(
            store
                .settings_with_prefix("guild-a", "studio.template.")
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .delete_setting("guild-a", "studio.template.one")
                .unwrap()
        );
        assert!(
            !store
                .delete_setting("guild-a", "studio.template.one")
                .unwrap()
        );
    }

    #[test]
    fn settings_prefix_counts_are_guild_scoped() {
        let store = Store::open(":memory:").unwrap();
        store.set_setting("g1", "support.panel.1", "{}").unwrap();
        store.set_setting("g1", "support.panel.2", "{}").unwrap();
        store
            .set_setting("g1", "community.role_panel.1", "{}")
            .unwrap();
        store.set_setting("g2", "support.panel.3", "{}").unwrap();
        assert_eq!(
            store.count_settings_prefix("g1", "support.panel.").unwrap(),
            2
        );
        assert_eq!(
            store
                .count_settings_prefix("g1", "community.role_panel.")
                .unwrap(),
            1
        );
        assert_eq!(
            store.count_settings_prefix("g2", "support.panel.").unwrap(),
            1
        );
    }

    #[test]
    fn events_and_suggestions_round_trip() {
        let store = Store::open(":memory:").unwrap();
        assert!(store.register_event("g", "event-1", "u").unwrap());
        assert!(!store.register_event("g", "event-1", "u").unwrap());
        assert!(store.check_in_event("g", "event-1", "u").unwrap());
        assert_eq!(
            store
                .event_registration("g", "event-1", "u")
                .unwrap()
                .unwrap()
                .status,
            "checked_in"
        );
        assert_eq!(
            store
                .register_event_with_capacity("g", "limited", "first", Some(1))
                .unwrap()
                .as_deref(),
            Some("registered")
        );
        assert_eq!(
            store
                .register_event_with_capacity("g", "limited", "second", Some(1))
                .unwrap()
                .as_deref(),
            Some("waitlisted")
        );
        assert_eq!(
            store
                .remove_event_registration("g", "limited", "first")
                .unwrap(),
            (true, Some("second".to_string()))
        );
        assert_eq!(
            store
                .event_registration("g", "limited", "second")
                .unwrap()
                .unwrap()
                .status,
            "registered"
        );
        let suggestion = store
            .create_suggestion("g", "u", "Add a weekly event")
            .unwrap();
        store.vote_suggestion(suggestion, "voter", 1).unwrap();
        assert_eq!(store.suggestion_votes(suggestion).unwrap(), (1, 0));
        assert!(
            store
                .set_suggestion_status("g", suggestion, "approved")
                .unwrap()
        );

        let giveaway = store
            .create_giveaway("g", "10", "Prize", 2, 100, None, "u")
            .unwrap();
        assert!(store.add_giveaway_entry(giveaway, "winner").unwrap());
        assert!(!store.add_giveaway_entry(giveaway, "winner").unwrap());
        assert_eq!(store.giveaway_entries(giveaway).unwrap(), vec!["winner"]);
        assert!(store.end_giveaway(giveaway).unwrap());

        let poll = store
            .create_poll("g", "10", "Choose", &["A".into(), "B".into()], 100)
            .unwrap();
        store.vote_poll(poll, "voter", 1).unwrap();
        assert_eq!(store.poll_counts(poll, 2).unwrap(), vec![0, 1]);
        assert!(store.close_poll(poll).unwrap());

        let workflow = store
            .create_workflow("g", "welcome", "message", "hello", "reply", "Hi {user}")
            .unwrap();
        assert_eq!(
            store.active_workflows("g", "message").unwrap()[0].id,
            workflow
        );
        assert!(
            store
                .record_workflow_run(workflow, "g", "message-1")
                .unwrap()
        );
        assert!(
            !store
                .record_workflow_run(workflow, "g", "message-1")
                .unwrap()
        );
        assert!(store.delete_workflow("g", workflow).unwrap());
        store
            .save_quarantine("g", "u", &["10".into(), "20".into()], "raid")
            .unwrap();
        assert_eq!(
            store.get_quarantine("g", "u").unwrap().unwrap().role_ids,
            vec!["10", "20"]
        );
        assert!(store.clear_quarantine("g", "u").unwrap());
    }

    #[test]
    fn guild_export_and_purge_are_scoped() {
        let store = Store::open(":memory:").unwrap();
        store.set_setting("g1", "welcome.channel", "10").unwrap();
        store
            .set_setting("g1", "automation.webhook_secret", "must-not-export")
            .unwrap();
        store.upsert_tag("g1", "rules", "be kind", "u1").unwrap();
        store
            .create_workflow("g1", "hello", "message", "hi", "reply", "Hello")
            .unwrap();
        store
            .record_case("g1", "warn", "u1", "mod", "reason", None)
            .unwrap();
        store
            .record_case("g2", "warn", "u2", "mod", "reason", None)
            .unwrap();

        let export = store.export_guild("g1").unwrap();
        assert_eq!(export["guildId"], "g1");
        assert_eq!(export["settings"][0]["key"], "welcome.channel");
        assert!(!export.to_string().contains("must-not-export"));
        assert_eq!(export["tags"][0]["name"], "rules");
        assert_eq!(export["workflows"][0]["name"], "hello");

        store.purge_guild("g1").unwrap();
        assert!(store.recent_cases("g1", 10).unwrap().is_empty());
        assert!(
            store
                .get_setting("g1", "welcome.channel")
                .unwrap()
                .is_none()
        );
        assert!(store.get_tag("g1", "rules").unwrap().is_none());
        assert_eq!(store.recent_cases("g2", 10).unwrap().len(), 1);
    }

    #[test]
    fn guild_config_import_is_validated_and_scoped() {
        let store = Store::open(":memory:").unwrap();
        store
            .set_setting("source", "welcome.channel", "10")
            .unwrap();
        store
            .upsert_tag("source", "rules", "be kind", "u1")
            .unwrap();
        store
            .create_workflow("source", "hello", "message", "hi", "reply", "Hello")
            .unwrap();
        let export = store.export_guild("source").unwrap();

        let imported = store.import_guild_config("target", &export).unwrap();
        assert_eq!(imported.settings, 1);
        assert_eq!(imported.tags, 1);
        assert_eq!(imported.workflows, 1);
        assert_eq!(
            store.get_setting("target", "welcome.channel").unwrap(),
            Some("10".into())
        );
        assert_eq!(
            store.get_tag("target", "rules").unwrap().unwrap().content,
            "be kind"
        );
        assert_eq!(
            store.active_workflows("target", "message").unwrap().len(),
            1
        );
        assert!(
            store
                .get_setting("source", "welcome.channel")
                .unwrap()
                .is_some()
        );

        let mut secret = export;
        secret["settings"] = serde_json::json!([{
            "key": "webhook_secret",
            "value": "must-reject"
        }]);
        assert!(store.import_guild_config("target", &secret).is_err());
    }

    #[test]
    fn user_privacy_export_and_purge_keep_moderation_records() {
        let store = Store::open(":memory:").unwrap();
        store.set_afk("g1", "u1", "away").unwrap();
        store.add_xp("g1", "u1", 42).unwrap();
        store.upsert_tag("g1", "mine", "hello", "u1").unwrap();
        store.create_suggestion("g1", "u1", "feature").unwrap();
        store
            .schedule_typed("g1", "reminder", "u1", 10, "{}")
            .unwrap();
        store.register_event("g1", "event-1", "u1").unwrap();
        store
            .record_case("g1", "warn", "u1", "mod", "kept", None)
            .unwrap();

        let export = store.export_user("g1", "u1").unwrap();
        assert_eq!(export["userId"], "u1");
        assert_eq!(export["moderationCases"].as_array().unwrap().len(), 1);
        assert!(export["voluntary"].as_array().unwrap().len() >= 3);
        assert_eq!(export["suggestions"].as_array().unwrap().len(), 1);

        let result = store.purge_user("g1", "u1").unwrap();
        assert!(
            result["deleted"]
                .as_object()
                .unwrap()
                .contains_key("levels")
        );
        assert!(store.recent_cases("g1", 10).unwrap().len() == 1);
        assert!(store.get_tag("g1", "mine").unwrap().is_none());
        assert!(store.get_afk("g1", "u1").unwrap().is_none());
    }
}
