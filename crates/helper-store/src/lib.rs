//! SQLite persistence with an intentionally small, auditable surface.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, Utc};
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

/// Structured evidence for a sensitive operation. The record is append-only
/// during normal operation and binds the actor, tenant, reason and outcome to
/// one correlation id so support can trace a moderation action end to end.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEventRecord {
    pub id: i64,
    pub correlation_id: String,
    pub guild_id: String,
    pub actor_id: String,
    pub action: String,
    pub reason: String,
    pub before_json: String,
    pub after_json: String,
    pub outcome: String,
    pub created_at: i64,
}

/// Privacy-first activity entry.  Message content is never required here;
/// callers can store bounded metadata such as an id, channel and whether the
/// payload was available in the gateway event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityLogRecord {
    pub id: i64,
    pub guild_id: String,
    pub kind: String,
    pub user_id: String,
    pub user_tag: Option<String>,
    pub actor_id: Option<String>,
    pub detail: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FeatureSettingRecord {
    pub guild_id: String,
    pub key: String,
    pub enabled: bool,
    pub config_json: String,
    pub revision: u64,
    pub updated_at: i64,
    pub updated_by: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct YouTubeSubscriptionRecord {
    pub id: i64,
    pub guild_id: String,
    pub source_channel_id: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub interval_seconds: i64,
    pub last_video_id: Option<String>,
    pub next_poll_at: i64,
    pub failure_count: i64,
    pub last_error: Option<String>,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Subscription projection written together with a feature revision. Keeping
/// this input separate from the persisted record prevents callers from
/// mutating polling state while publishing configuration.
#[derive(Debug, Clone)]
pub struct YouTubeSubscriptionWrite {
    pub source_channel_id: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub interval_seconds: i64,
    pub created_by: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RssSubscriptionRecord {
    pub id: i64,
    pub guild_id: String,
    pub feed_url: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub interval_seconds: i64,
    pub last_item_id: Option<String>,
    pub next_poll_at: i64,
    pub failure_count: i64,
    pub last_error: Option<String>,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct RssSubscriptionWrite {
    pub feed_url: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub interval_seconds: i64,
    pub created_by: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TwitchSubscriptionRecord {
    pub id: i64,
    pub guild_id: String,
    pub source_login: String,
    pub source_user_id: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub pending_event_id: Option<String>,
    pub pending_stream_id: Option<String>,
    pub pending_started_at: Option<String>,
    pub last_event_id: Option<String>,
    pub next_poll_at: i64,
    pub failure_count: i64,
    pub last_error: Option<String>,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct TwitchSubscriptionWrite {
    pub source_login: String,
    pub source_user_id: String,
    pub target_channel_id: String,
    pub message_template: String,
    pub mention: String,
    pub enabled: bool,
    pub created_by: String,
}

fn youtube_subscription_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<YouTubeSubscriptionRecord> {
    Ok(YouTubeSubscriptionRecord {
        id: row.get(0)?,
        guild_id: row.get(1)?,
        source_channel_id: row.get(2)?,
        target_channel_id: row.get(3)?,
        message_template: row.get(4)?,
        mention: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        interval_seconds: row.get(7)?,
        last_video_id: row.get(8)?,
        next_poll_at: row.get(9)?,
        failure_count: row.get(10)?,
        last_error: row.get(11)?,
        created_by: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn rss_subscription_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RssSubscriptionRecord> {
    Ok(RssSubscriptionRecord {
        id: row.get(0)?,
        guild_id: row.get(1)?,
        feed_url: row.get(2)?,
        target_channel_id: row.get(3)?,
        message_template: row.get(4)?,
        mention: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        interval_seconds: row.get(7)?,
        last_item_id: row.get(8)?,
        next_poll_at: row.get(9)?,
        failure_count: row.get(10)?,
        last_error: row.get(11)?,
        created_by: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn twitch_subscription_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<TwitchSubscriptionRecord> {
    Ok(TwitchSubscriptionRecord {
        id: row.get(0)?,
        guild_id: row.get(1)?,
        source_login: row.get(2)?,
        source_user_id: row.get(3)?,
        target_channel_id: row.get(4)?,
        message_template: row.get(5)?,
        mention: row.get(6)?,
        enabled: row.get::<_, i64>(7)? != 0,
        pending_event_id: row.get(8)?,
        pending_stream_id: row.get(9)?,
        pending_started_at: row.get(10)?,
        last_event_id: row.get(11)?,
        next_poll_at: row.get(12)?,
        failure_count: row.get(13)?,
        last_error: row.get(14)?,
        created_by: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RetentionSummary {
    pub deleted: serde_json::Value,
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
pub struct BirthdayRecord {
    pub guild_id: String,
    pub user_id: String,
    pub month: u32,
    pub day: u32,
    pub last_announced_year: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EconomyAccount {
    pub guild_id: String,
    pub user_id: String,
    pub balance: i64,
    pub last_daily_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TempChannelRecord {
    pub guild_id: String,
    pub channel_id: String,
    pub owner_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LevelRecord {
    pub guild_id: String,
    pub user_id: String,
    pub xp: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VoiceSessionRecord {
    pub guild_id: String,
    pub user_id: String,
    pub channel_id: String,
    pub started_at: i64,
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
    pub birthdays: usize,
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
        conn.execute_batch("CREATE TABLE IF NOT EXISTS birthdays (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, month INTEGER NOT NULL, day INTEGER NOT NULL, last_announced_year INTEGER, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE INDEX IF NOT EXISTS idx_birthdays_day ON birthdays(month,day,last_announced_year); CREATE TABLE IF NOT EXISTS economy_accounts (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, balance INTEGER NOT NULL DEFAULT 0, last_daily_at INTEGER, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE INDEX IF NOT EXISTS idx_economy_guild_balance ON economy_accounts(guild_id,balance DESC); CREATE TABLE IF NOT EXISTS temp_channels (guild_id TEXT NOT NULL, channel_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_temp_channels_guild ON temp_channels(guild_id); CREATE TABLE IF NOT EXISTS voice_sessions (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, channel_id TEXT NOT NULL, started_at INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE INDEX IF NOT EXISTS idx_voice_sessions_guild ON voice_sessions(guild_id,started_at);")?;
        let oauth_verifier_exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('helper_oauth_states') WHERE name='code_verifier'",
            [],
            |row| row.get(0),
        )?;
        if oauth_verifier_exists == 0 {
            conn.execute(
                "ALTER TABLE helper_oauth_states ADD COLUMN code_verifier TEXT",
                [],
            )?;
        }
        // Keep the ticket schema compatible with the existing Node Helper DB.
        // The owner column is named `opener_id` there; changing it to `user_id`
        // would make an in-place Rust cutover fail on the live database.
        conn.execute_batch("CREATE TABLE IF NOT EXISTS tickets (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, opener_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'open', claimed_by TEXT, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_tickets_owner ON tickets(guild_id,opener_id,status); CREATE TABLE IF NOT EXISTS suggestions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, author_id TEXT NOT NULL, content TEXT NOT NULL, message_id TEXT, status TEXT NOT NULL DEFAULT 'pending', created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS suggestion_votes (suggestion_id INTEGER NOT NULL, user_id TEXT NOT NULL, vote INTEGER NOT NULL, PRIMARY KEY(suggestion_id,user_id)); CREATE TABLE IF NOT EXISTS giveaways (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, message_id TEXT, prize TEXT NOT NULL, winners INTEGER NOT NULL DEFAULT 1, end_at INTEGER NOT NULL, ended INTEGER NOT NULL DEFAULT 0, required_role_id TEXT, host_id TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS giveaway_entries (giveaway_id INTEGER NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY(giveaway_id,user_id)); CREATE TABLE IF NOT EXISTS polls (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, channel_id TEXT NOT NULL, message_id TEXT, question TEXT NOT NULL, options TEXT NOT NULL, end_at INTEGER NOT NULL, closed INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS poll_votes (poll_id INTEGER NOT NULL, user_id TEXT NOT NULL, choice INTEGER NOT NULL, PRIMARY KEY(poll_id,user_id)); CREATE TABLE IF NOT EXISTS workflows (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, name TEXT NOT NULL, trigger TEXT NOT NULL, condition TEXT NOT NULL DEFAULT '', action TEXT NOT NULL, payload TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS workflow_runs (id INTEGER PRIMARY KEY AUTOINCREMENT, workflow_id INTEGER NOT NULL, guild_id TEXT NOT NULL, source_id TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_workflows_trigger ON workflows(guild_id,trigger,enabled); CREATE TABLE IF NOT EXISTS quarantine (guild_id TEXT NOT NULL, user_id TEXT NOT NULL, role_ids TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, PRIMARY KEY(guild_id,user_id)); CREATE TABLE IF NOT EXISTS starboard (guild_id TEXT NOT NULL, original_message_id TEXT NOT NULL, starboard_message_id TEXT NOT NULL, star_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild_id,original_message_id));")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS audit_events (id INTEGER PRIMARY KEY AUTOINCREMENT, correlation_id TEXT NOT NULL UNIQUE, guild_id TEXT NOT NULL, actor_id TEXT NOT NULL, action TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', before_json TEXT NOT NULL DEFAULT '{}', after_json TEXT NOT NULL DEFAULT '{}', outcome TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_audit_events_guild_time ON audit_events(guild_id, created_at DESC); CREATE TABLE IF NOT EXISTS feature_settings (guild_id TEXT NOT NULL, key TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0, config_json TEXT NOT NULL DEFAULT '{}', revision INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL, updated_by TEXT NOT NULL DEFAULT '', PRIMARY KEY(guild_id,key)); CREATE TABLE IF NOT EXISTS feature_revisions (guild_id TEXT NOT NULL, key TEXT NOT NULL, revision INTEGER NOT NULL, enabled INTEGER NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL, updated_by TEXT NOT NULL, PRIMARY KEY(guild_id,key,revision)); CREATE INDEX IF NOT EXISTS idx_feature_revisions_lookup ON feature_revisions(guild_id,key,revision DESC);")?;
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
        conn.execute_batch("CREATE TABLE IF NOT EXISTS twitch_subscriptions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, source_login TEXT NOT NULL, source_user_id TEXT NOT NULL, target_channel_id TEXT NOT NULL, message_template TEXT NOT NULL DEFAULT '{broadcaster} is live!\\nhttps://twitch.tv/{login}', mention TEXT NOT NULL DEFAULT '', enabled INTEGER NOT NULL DEFAULT 1, pending_event_id TEXT, pending_stream_id TEXT, pending_started_at TEXT, last_event_id TEXT, next_poll_at INTEGER NOT NULL, failure_count INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_by TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE(guild_id,source_user_id,target_channel_id)); CREATE INDEX IF NOT EXISTS idx_twitch_due ON twitch_subscriptions(enabled,next_poll_at); CREATE INDEX IF NOT EXISTS idx_twitch_user ON twitch_subscriptions(source_user_id,enabled); CREATE INDEX IF NOT EXISTS idx_twitch_guild ON twitch_subscriptions(guild_id,updated_at DESC);")?;
        let twitch_last_event_exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('twitch_subscriptions') WHERE name='last_event_id'",
            [],
            |row| row.get(0),
        )?;
        if twitch_last_event_exists == 0 {
            conn.execute(
                "ALTER TABLE twitch_subscriptions ADD COLUMN last_event_id TEXT",
                [],
            )?;
        }
        conn.execute_batch("CREATE TABLE IF NOT EXISTS youtube_subscriptions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, source_channel_id TEXT NOT NULL, target_channel_id TEXT NOT NULL, message_template TEXT NOT NULL DEFAULT 'New video from {channel}: **{title}**\\n{url}', mention TEXT NOT NULL DEFAULT '', enabled INTEGER NOT NULL DEFAULT 1, interval_seconds INTEGER NOT NULL DEFAULT 300, last_video_id TEXT, next_poll_at INTEGER NOT NULL, failure_count INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_by TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE(guild_id,source_channel_id,target_channel_id)); CREATE INDEX IF NOT EXISTS idx_youtube_due ON youtube_subscriptions(enabled,next_poll_at); CREATE INDEX IF NOT EXISTS idx_youtube_guild ON youtube_subscriptions(guild_id,updated_at DESC);")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS rss_subscriptions (id INTEGER PRIMARY KEY AUTOINCREMENT, guild_id TEXT NOT NULL, feed_url TEXT NOT NULL, target_channel_id TEXT NOT NULL, message_template TEXT NOT NULL DEFAULT 'New post from {feed}: **{title}**\\n{url}', mention TEXT NOT NULL DEFAULT '', enabled INTEGER NOT NULL DEFAULT 1, interval_seconds INTEGER NOT NULL DEFAULT 300, last_item_id TEXT, next_poll_at INTEGER NOT NULL, failure_count INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_by TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, UNIQUE(guild_id,feed_url,target_channel_id)); CREATE INDEX IF NOT EXISTS idx_rss_due ON rss_subscriptions(enabled,next_poll_at); CREATE INDEX IF NOT EXISTS idx_rss_guild ON rss_subscriptions(guild_id,updated_at DESC);")?;
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

    pub fn register_oauth_state(
        &self,
        state_hash: &str,
        expires_at: i64,
        code_verifier: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "DELETE FROM helper_oauth_states WHERE expires_at < ?1 OR used_at < ?1 - 3600",
            [Utc::now().timestamp()],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO helper_oauth_states(state_hash,expires_at,used_at,code_verifier) VALUES(?1,?2,NULL,?3)",
            params![state_hash, expires_at, code_verifier],
        )?;
        Ok(())
    }

    pub fn consume_oauth_state(&self, state_hash: &str, now: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let verifier = conn
            .query_row(
                "SELECT code_verifier FROM helper_oauth_states WHERE state_hash=?1 AND used_at IS NULL AND expires_at>=?2",
                params![state_hash, now],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        if verifier.is_some() {
            conn.execute(
                "UPDATE helper_oauth_states SET used_at=?2 WHERE state_hash=?1 AND used_at IS NULL AND expires_at>=?2",
                params![state_hash, now],
            )?;
        }
        Ok(verifier.flatten())
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
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let created_at = Utc::now().timestamp_millis();
        tx.execute("INSERT INTO cases(guild_id,type,target_id,moderator_id,reason,duration_ms,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![guild_id, kind, target_id, moderator_id, reason, duration_ms, created_at])?;
        let case_id = tx.last_insert_rowid();
        let correlation_id = Uuid::new_v4().to_string();
        let before_json = serde_json::json!({"targetId": target_id}).to_string();
        let after_json = serde_json::json!({"caseId": case_id, "targetId": target_id, "durationMs": duration_ms}).to_string();
        tx.execute(
            "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![correlation_id, guild_id, moderator_id, kind, reason, before_json, after_json, "recorded", created_at],
        )?;
        tx.commit()?;
        Ok(case_id)
    }

    pub fn recent_audit_events(&self, guild_id: &str, limit: u32) -> Result<Vec<AuditEventRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at FROM audit_events WHERE guild_id=?1 ORDER BY id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(200))], |row| {
            Ok(AuditEventRecord {
                id: row.get(0)?,
                correlation_id: row.get(1)?,
                guild_id: row.get(2)?,
                actor_id: row.get(3)?,
                action: row.get(4)?,
                reason: row.get(5)?,
                before_json: row.get(6)?,
                after_json: row.get(7)?,
                outcome: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Append metadata-only activity.  Details are bounded before entering
    /// SQLite so an unexpected gateway payload cannot grow the audit table
    /// without limit.
    pub fn record_activity(
        &self,
        guild_id: &str,
        kind: &str,
        user_id: &str,
        user_tag: Option<&str>,
        actor_id: Option<&str>,
        detail: &str,
    ) -> Result<i64> {
        let bounded_detail = detail.chars().take(2_000).collect::<String>();
        let bounded_kind = kind.chars().take(80).collect::<String>();
        let bounded_user = user_id.chars().take(64).collect::<String>();
        let bounded_tag = user_tag.map(|value| value.chars().take(128).collect::<String>());
        let bounded_actor = actor_id.map(|value| value.chars().take(64).collect::<String>());
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO activity_log(guild_id,type,user_id,user_tag,actor_id,detail,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                guild_id,
                bounded_kind,
                bounded_user,
                bounded_tag,
                bounded_actor,
                bounded_detail,
                Utc::now().timestamp_millis()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_activity(&self, guild_id: &str, limit: u32) -> Result<Vec<ActivityLogRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id,guild_id,type,user_id,user_tag,actor_id,detail,created_at FROM activity_log WHERE guild_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![guild_id, i64::from(limit.min(200))], |row| {
            Ok(ActivityLogRecord {
                id: row.get(0)?,
                guild_id: row.get(1)?,
                kind: row.get(2)?,
                user_id: row.get(3)?,
                user_tag: row.get(4)?,
                actor_id: row.get(5)?,
                detail: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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

    /// Remove only voluntary member state. Moderation records remain intact so
    /// that the server's audit history is not silently rewritten when a member
    /// leaves or requests deletion of personal preferences.
    pub fn delete_member_voluntary_data(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<serde_json::Value> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let mut deleted = serde_json::Map::new();
        for (name, statement) in [
            (
                "suggestion_votes",
                "DELETE FROM suggestion_votes WHERE suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?2 AND author_id=?1) OR (user_id=?1 AND suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?2))",
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
                "voice_sessions",
                "DELETE FROM voice_sessions WHERE guild_id=?2 AND user_id=?1",
            ),
            (
                "birthdays",
                "DELETE FROM birthdays WHERE guild_id=?2 AND user_id=?1",
            ),
            (
                "economy_accounts",
                "DELETE FROM economy_accounts WHERE guild_id=?2 AND user_id=?1",
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
        Ok(serde_json::Value::Object(deleted))
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

    pub fn set_birthday(&self, guild_id: &str, user_id: &str, month: u32, day: u32) -> Result<()> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            bail!("invalid_birthday");
        }
        let now = Utc::now().timestamp_millis();
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO birthdays(guild_id,user_id,month,day,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(guild_id,user_id) DO UPDATE SET month=excluded.month,day=excluded.day,last_announced_year=NULL,updated_at=excluded.updated_at",
            params![guild_id, user_id, month, day, now],
        )?;
        Ok(())
    }

    pub fn remove_birthday(&self, guild_id: &str, user_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM birthdays WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
        )? > 0)
    }

    pub fn due_birthdays(
        &self,
        month: u32,
        day: u32,
        year: i32,
        limit: u32,
    ) -> Result<Vec<BirthdayRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT guild_id,user_id,month,day,last_announced_year FROM birthdays WHERE month=?1 AND day=?2 AND (last_announced_year IS NULL OR last_announced_year<>?3) LIMIT ?4")?;
        let rows = stmt.query_map(
            params![month, day, year, i64::from(limit.min(500))],
            |row| {
                Ok(BirthdayRecord {
                    guild_id: row.get(0)?,
                    user_id: row.get(1)?,
                    month: row.get(2)?,
                    day: row.get(3)?,
                    last_announced_year: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_birthday_announced(
        &self,
        guild_id: &str,
        user_id: &str,
        year: i32,
    ) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("UPDATE birthdays SET last_announced_year=?3,updated_at=?4 WHERE guild_id=?1 AND user_id=?2 AND (last_announced_year IS NULL OR last_announced_year<>?3)", params![guild_id, user_id, year, Utc::now().timestamp_millis()])? > 0)
    }

    pub fn economy_account(&self, guild_id: &str, user_id: &str) -> Result<EconomyAccount> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let now = Utc::now().timestamp_millis();
        conn.execute(
            "INSERT OR IGNORE INTO economy_accounts(guild_id,user_id,balance,created_at,updated_at) VALUES(?1,?2,0,?3,?3)",
            params![guild_id, user_id, now],
        )?;
        Ok(conn.query_row(
            "SELECT guild_id,user_id,balance,last_daily_at FROM economy_accounts WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
            |row| {
                Ok(EconomyAccount {
                    guild_id: row.get(0)?,
                    user_id: row.get(1)?,
                    balance: row.get(2)?,
                    last_daily_at: row.get(3)?,
                })
            },
        )?)
    }

    /// Atomically claim a daily reward. `None` means the account has already
    /// claimed within the bounded 24-hour cooldown window.
    pub fn claim_daily(
        &self,
        guild_id: &str,
        user_id: &str,
        reward: i64,
    ) -> Result<Option<EconomyAccount>> {
        let reward = reward.clamp(1, 10_000);
        let now = Utc::now().timestamp_millis();
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let existing: Option<(i64, Option<i64>)> = tx
            .query_row(
                "SELECT balance,last_daily_at FROM economy_accounts WHERE guild_id=?1 AND user_id=?2",
                params![guild_id, user_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (balance, last_daily_at) = existing.unwrap_or((0, None));
        if last_daily_at.is_some_and(|last| now.saturating_sub(last) < 86_400_000) {
            return Ok(None);
        }
        let next_balance = balance.saturating_add(reward);
        tx.execute(
            "INSERT INTO economy_accounts(guild_id,user_id,balance,last_daily_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(guild_id,user_id) DO UPDATE SET balance=excluded.balance,last_daily_at=excluded.last_daily_at,updated_at=excluded.updated_at",
            params![guild_id, user_id, next_balance, now, now],
        )?;
        tx.commit()?;
        Ok(Some(EconomyAccount {
            guild_id: guild_id.into(),
            user_id: user_id.into(),
            balance: next_balance,
            last_daily_at: Some(now),
        }))
    }

    pub fn register_temp_channel(
        &self,
        guild_id: &str,
        channel_id: &str,
        owner_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO temp_channels(guild_id,channel_id,owner_id,created_at) VALUES(?1,?2,?3,?4)",
            params![guild_id, channel_id, owner_id, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn temp_channel(&self, channel_id: &str) -> Result<Option<TempChannelRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT guild_id,channel_id,owner_id,created_at FROM temp_channels WHERE channel_id=?1",
                [channel_id],
                |row| {
                    Ok(TempChannelRecord {
                        guild_id: row.get(0)?,
                        channel_id: row.get(1)?,
                        owner_id: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn active_temp_channels(&self, guild_id: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.query_row(
            "SELECT COUNT(*) FROM temp_channels WHERE guild_id=?1",
            [guild_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    pub fn remove_temp_channel(&self, channel_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM temp_channels WHERE channel_id=?1",
            [channel_id],
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

    pub fn get_feature_setting(
        &self,
        guild_id: &str,
        key: &str,
    ) -> Result<Option<FeatureSettingRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT guild_id,key,enabled,config_json,revision,updated_at,updated_by FROM feature_settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| {
                    Ok(FeatureSettingRecord {
                        guild_id: row.get(0)?,
                        key: row.get(1)?,
                        enabled: row.get::<_, i64>(2)? != 0,
                        config_json: row.get(3)?,
                        revision: row.get::<_, i64>(4)?.try_into().unwrap_or(0),
                        updated_at: row.get(5)?,
                        updated_by: row.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn feature_revision(
        &self,
        guild_id: &str,
        key: &str,
        revision: u64,
    ) -> Result<Option<FeatureSettingRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT ?1,?2,enabled,config_json,revision,updated_at,updated_by FROM feature_revisions WHERE guild_id=?1 AND key=?2 AND revision=?3",
                params![guild_id, key, i64::try_from(revision).unwrap_or(i64::MAX)],
                |row| {
                    Ok(FeatureSettingRecord {
                        guild_id: row.get(0)?,
                        key: row.get(1)?,
                        enabled: row.get::<_, i64>(2)? != 0,
                        config_json: row.get(3)?,
                        revision: row.get::<_, i64>(4)?.try_into().unwrap_or(0),
                        updated_at: row.get(5)?,
                        updated_by: row.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn feature_revisions(
        &self,
        guild_id: &str,
        key: &str,
        limit: u32,
    ) -> Result<Vec<FeatureSettingRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT ?1,?2,enabled,config_json,revision,updated_at,updated_by FROM feature_revisions WHERE guild_id=?1 AND key=?2 ORDER BY revision DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![guild_id, key, i64::from(limit.min(100))], |row| {
            Ok(FeatureSettingRecord {
                guild_id: row.get(0)?,
                key: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                config_json: row.get(3)?,
                revision: row.get::<_, i64>(4)?.try_into().unwrap_or(0),
                updated_at: row.get(5)?,
                updated_by: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Publishes a feature and all of its runtime projections atomically.
    /// `expected_revision` prevents a stale panel tab from overwriting a
    /// newer change made by another moderator.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_feature_setting(
        &self,
        guild_id: &str,
        key: &str,
        enabled: bool,
        config_json: &str,
        expected_revision: Option<u64>,
        updated_by: &str,
        projections: &[(String, String)],
    ) -> Result<FeatureSettingRecord> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<(i64, i64, String, String)> = tx
            .query_row(
                "SELECT revision,enabled,config_json,updated_by FROM feature_settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let current_revision = current
            .as_ref()
            .and_then(|item| u64::try_from(item.0).ok())
            .unwrap_or(0);
        if expected_revision.is_some_and(|value| value != current_revision) {
            bail!("feature_revision_conflict:{current_revision}");
        }
        let next_revision = current_revision.saturating_add(1);
        let now = Utc::now().timestamp_millis();
        tx.execute(
            "INSERT INTO feature_settings(guild_id,key,enabled,config_json,revision,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(guild_id,key) DO UPDATE SET enabled=excluded.enabled,config_json=excluded.config_json,revision=excluded.revision,updated_at=excluded.updated_at,updated_by=excluded.updated_by",
            params![guild_id, key, if enabled { 1_i64 } else { 0_i64 }, config_json, i64::try_from(next_revision).unwrap_or(i64::MAX), now, updated_by],
        )?;
        tx.execute(
            "INSERT INTO feature_revisions(guild_id,key,revision,enabled,config_json,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![guild_id, key, i64::try_from(next_revision).unwrap_or(i64::MAX), if enabled { 1_i64 } else { 0_i64 }, config_json, now, updated_by],
        )?;
        for (projection_key, projection_value) in projections {
            tx.execute(
                "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                params![guild_id, projection_key, projection_value, now],
            )?;
        }
        let correlation_id = Uuid::new_v4().to_string();
        let before_json = current
            .as_ref()
            .map(|item| serde_json::json!({"enabled": item.1 != 0, "config": item.2, "revision": item.0}))
            .unwrap_or_else(|| serde_json::json!({}));
        let after_json = serde_json::json!({"enabled": enabled, "config": serde_json::from_str::<serde_json::Value>(config_json).unwrap_or_else(|_| serde_json::json!({})), "revision": next_revision});
        tx.execute(
            "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![correlation_id, guild_id, updated_by, "feature.publish", key, before_json.to_string(), after_json.to_string(), "published", now],
        )?;
        tx.commit()?;
        Ok(FeatureSettingRecord {
            guild_id: guild_id.to_string(),
            key: key.to_string(),
            enabled,
            config_json: config_json.to_string(),
            revision: next_revision,
            updated_at: now,
            updated_by: updated_by.to_string(),
        })
    }

    /// Publishes the YouTube feature and its subscription projection in the
    /// same SQLite transaction. This keeps a stale revision or a failed
    /// subscription write from leaving the panel and worker out of sync.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_youtube_feature_setting(
        &self,
        guild_id: &str,
        key: &str,
        enabled: bool,
        config_json: &str,
        expected_revision: Option<u64>,
        updated_by: &str,
        projections: &[(String, String)],
        subscription: Option<&YouTubeSubscriptionWrite>,
    ) -> Result<FeatureSettingRecord> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<(i64, i64, String, String)> = tx
            .query_row(
                "SELECT revision,enabled,config_json,updated_by FROM feature_settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let current_revision = current
            .as_ref()
            .and_then(|item| u64::try_from(item.0).ok())
            .unwrap_or(0);
        if expected_revision.is_some_and(|value| value != current_revision) {
            bail!("feature_revision_conflict:{current_revision}");
        }
        let next_revision = current_revision.saturating_add(1);
        let now = Utc::now().timestamp_millis();
        tx.execute(
            "INSERT INTO feature_settings(guild_id,key,enabled,config_json,revision,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(guild_id,key) DO UPDATE SET enabled=excluded.enabled,config_json=excluded.config_json,revision=excluded.revision,updated_at=excluded.updated_at,updated_by=excluded.updated_by",
            params![guild_id, key, if enabled { 1_i64 } else { 0_i64 }, config_json, i64::try_from(next_revision).unwrap_or(i64::MAX), now, updated_by],
        )?;
        tx.execute(
            "INSERT INTO feature_revisions(guild_id,key,revision,enabled,config_json,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![guild_id, key, i64::try_from(next_revision).unwrap_or(i64::MAX), if enabled { 1_i64 } else { 0_i64 }, config_json, now, updated_by],
        )?;
        for (projection_key, projection_value) in projections {
            tx.execute(
                "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                params![guild_id, projection_key, projection_value, now],
            )?;
        }
        if let Some(subscription) = subscription {
            let existing_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM youtube_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC LIMIT 1",
                    [guild_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing_id {
                tx.execute(
                    "UPDATE youtube_subscriptions SET source_channel_id=?2,target_channel_id=?3,message_template=?4,mention=?5,enabled=?6,interval_seconds=?7,last_video_id=NULL,next_poll_at=?8,failure_count=0,last_error=NULL,updated_at=?8 WHERE guild_id=?1 AND id=?9",
                    params![guild_id, subscription.source_channel_id, subscription.target_channel_id, subscription.message_template, subscription.mention, if subscription.enabled { 1_i64 } else { 0_i64 }, subscription.interval_seconds, now, id],
                )?;
            } else if subscription.enabled {
                tx.execute(
                    "INSERT INTO youtube_subscriptions(guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,1,?6,?7,?8,?7,?7)",
                    params![guild_id, subscription.source_channel_id, subscription.target_channel_id, subscription.message_template, subscription.mention, subscription.interval_seconds, now, subscription.created_by],
                )?;
            }
        }
        let correlation_id = Uuid::new_v4().to_string();
        let before_json = current
            .as_ref()
            .map(|item| serde_json::json!({"enabled": item.1 != 0, "config": item.2, "revision": item.0}))
            .unwrap_or_else(|| serde_json::json!({}));
        let after_json = serde_json::json!({"enabled": enabled, "config": serde_json::from_str::<serde_json::Value>(config_json).unwrap_or_else(|_| serde_json::json!({})), "revision": next_revision});
        tx.execute(
            "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![correlation_id, guild_id, updated_by, "feature.publish", key, before_json.to_string(), after_json.to_string(), "published", now],
        )?;
        tx.commit()?;
        Ok(FeatureSettingRecord {
            guild_id: guild_id.to_string(),
            key: key.to_string(),
            enabled,
            config_json: config_json.to_string(),
            revision: next_revision,
            updated_at: now,
            updated_by: updated_by.to_string(),
        })
    }

    /// Publishes the RSS feature and its subscription projection atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_rss_feature_setting(
        &self,
        guild_id: &str,
        key: &str,
        enabled: bool,
        config_json: &str,
        expected_revision: Option<u64>,
        updated_by: &str,
        projections: &[(String, String)],
        subscription: Option<&RssSubscriptionWrite>,
    ) -> Result<FeatureSettingRecord> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<(i64, i64, String, String)> = tx
            .query_row(
                "SELECT revision,enabled,config_json,updated_by FROM feature_settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let current_revision = current
            .as_ref()
            .and_then(|item| u64::try_from(item.0).ok())
            .unwrap_or(0);
        if expected_revision.is_some_and(|value| value != current_revision) {
            bail!("feature_revision_conflict:{current_revision}");
        }
        let next_revision = current_revision.saturating_add(1);
        let now = Utc::now().timestamp_millis();
        tx.execute(
            "INSERT INTO feature_settings(guild_id,key,enabled,config_json,revision,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(guild_id,key) DO UPDATE SET enabled=excluded.enabled,config_json=excluded.config_json,revision=excluded.revision,updated_at=excluded.updated_at,updated_by=excluded.updated_by",
            params![guild_id, key, if enabled { 1_i64 } else { 0_i64 }, config_json, i64::try_from(next_revision).unwrap_or(i64::MAX), now, updated_by],
        )?;
        tx.execute(
            "INSERT INTO feature_revisions(guild_id,key,revision,enabled,config_json,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![guild_id, key, i64::try_from(next_revision).unwrap_or(i64::MAX), if enabled { 1_i64 } else { 0_i64 }, config_json, now, updated_by],
        )?;
        for (projection_key, projection_value) in projections {
            tx.execute(
                "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                params![guild_id, projection_key, projection_value, now],
            )?;
        }
        if let Some(subscription) = subscription {
            let existing_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM rss_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC LIMIT 1",
                    [guild_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing_id {
                tx.execute(
                    "UPDATE rss_subscriptions SET feed_url=?2,target_channel_id=?3,message_template=?4,mention=?5,enabled=?6,interval_seconds=?7,last_item_id=NULL,next_poll_at=?8,failure_count=0,last_error=NULL,updated_at=?8 WHERE guild_id=?1 AND id=?9",
                    params![guild_id, subscription.feed_url, subscription.target_channel_id, subscription.message_template, subscription.mention, if subscription.enabled { 1_i64 } else { 0_i64 }, subscription.interval_seconds, now, id],
                )?;
            } else if subscription.enabled {
                tx.execute(
                    "INSERT INTO rss_subscriptions(guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,1,?6,?7,?8,?7,?7)",
                    params![guild_id, subscription.feed_url, subscription.target_channel_id, subscription.message_template, subscription.mention, subscription.interval_seconds, now, subscription.created_by],
                )?;
            }
        }
        let correlation_id = Uuid::new_v4().to_string();
        let before_json = current
            .as_ref()
            .map(|item| serde_json::json!({"enabled": item.1 != 0, "config": item.2, "revision": item.0}))
            .unwrap_or_else(|| serde_json::json!({}));
        let after_json = serde_json::json!({"enabled": enabled, "config": serde_json::from_str::<serde_json::Value>(config_json).unwrap_or_else(|_| serde_json::json!({})), "revision": next_revision});
        tx.execute(
            "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![correlation_id, guild_id, updated_by, "feature.publish", key, before_json.to_string(), after_json.to_string(), "published", now],
        )?;
        tx.commit()?;
        Ok(FeatureSettingRecord {
            guild_id: guild_id.to_string(),
            key: key.to_string(),
            enabled,
            config_json: config_json.to_string(),
            revision: next_revision,
            updated_at: now,
            updated_by: updated_by.to_string(),
        })
    }

    pub fn youtube_subscriptions(&self, guild_id: &str) -> Result<Vec<YouTubeSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,last_video_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM youtube_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt.query_map([guild_id], youtube_subscription_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_youtube_subscription(
        &self,
        guild_id: &str,
        source_channel_id: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
        interval_seconds: i64,
        created_by: &str,
    ) -> Result<YouTubeSubscriptionRecord> {
        let now = Utc::now().timestamp_millis();
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO youtube_subscriptions(guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?8,?8)", params![guild_id, source_channel_id, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, interval_seconds, now, created_by])
            .map_err(|error| if error.to_string().contains("UNIQUE") { anyhow!("youtube_subscription_exists") } else { error.into() })?;
        let id = conn.last_insert_rowid();
        conn.query_row("SELECT id,guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,last_video_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM youtube_subscriptions WHERE id=?1", [id], youtube_subscription_from_row).map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_youtube_subscription(
        &self,
        guild_id: &str,
        id: i64,
        source_channel_id: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
        interval_seconds: i64,
    ) -> Result<Option<YouTubeSubscriptionRecord>> {
        let now = Utc::now().timestamp_millis();
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("UPDATE youtube_subscriptions SET source_channel_id=?3,target_channel_id=?4,message_template=?5,mention=?6,enabled=?7,interval_seconds=?8,updated_at=?9 WHERE guild_id=?1 AND id=?2", params![guild_id, id, source_channel_id, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, interval_seconds, now])?;
        conn.query_row("SELECT id,guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,last_video_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM youtube_subscriptions WHERE guild_id=?1 AND id=?2", params![guild_id, id], youtube_subscription_from_row).optional()?.map_or(Ok(None), |record| Ok(Some(record)))
    }

    pub fn delete_youtube_subscription(&self, guild_id: &str, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM youtube_subscriptions WHERE guild_id=?1 AND id=?2",
            params![guild_id, id],
        )? > 0)
    }

    pub fn due_youtube_subscriptions(
        &self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<YouTubeSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,source_channel_id,target_channel_id,message_template,mention,enabled,interval_seconds,last_video_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM youtube_subscriptions WHERE enabled=1 AND next_poll_at<=?1 ORDER BY next_poll_at ASC LIMIT ?2")?;
        let rows = stmt.query_map(
            params![now_ms, i64::from(limit.min(100))],
            youtube_subscription_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_youtube_poll(
        &self,
        id: i64,
        last_video_id: Option<&str>,
        next_poll_at: i64,
        failure_count: i64,
        last_error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("UPDATE youtube_subscriptions SET last_video_id=?2,next_poll_at=?3,failure_count=?4,last_error=?5,updated_at=?3 WHERE id=?1", params![id, last_video_id, next_poll_at, failure_count, last_error])?;
        Ok(())
    }

    /// Publishes the Twitch feature and its EventSub delivery projection atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_twitch_feature_setting(
        &self,
        guild_id: &str,
        key: &str,
        enabled: bool,
        config_json: &str,
        expected_revision: Option<u64>,
        updated_by: &str,
        projections: &[(String, String)],
        subscription: Option<&TwitchSubscriptionWrite>,
    ) -> Result<FeatureSettingRecord> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let current: Option<(i64, i64, String, String)> = tx
            .query_row(
                "SELECT revision,enabled,config_json,updated_by FROM feature_settings WHERE guild_id=?1 AND key=?2",
                params![guild_id, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let current_revision = current
            .as_ref()
            .and_then(|item| u64::try_from(item.0).ok())
            .unwrap_or(0);
        if expected_revision.is_some_and(|value| value != current_revision) {
            bail!("feature_revision_conflict:{current_revision}");
        }
        let next_revision = current_revision.saturating_add(1);
        let now = Utc::now().timestamp_millis();
        tx.execute(
            "INSERT INTO feature_settings(guild_id,key,enabled,config_json,revision,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(guild_id,key) DO UPDATE SET enabled=excluded.enabled,config_json=excluded.config_json,revision=excluded.revision,updated_at=excluded.updated_at,updated_by=excluded.updated_by",
            params![guild_id, key, if enabled { 1_i64 } else { 0_i64 }, config_json, i64::try_from(next_revision).unwrap_or(i64::MAX), now, updated_by],
        )?;
        tx.execute(
            "INSERT INTO feature_revisions(guild_id,key,revision,enabled,config_json,updated_at,updated_by) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![guild_id, key, i64::try_from(next_revision).unwrap_or(i64::MAX), if enabled { 1_i64 } else { 0_i64 }, config_json, now, updated_by],
        )?;
        for (projection_key, projection_value) in projections {
            tx.execute(
                "INSERT INTO settings(guild_id,key,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                params![guild_id, projection_key, projection_value, now],
            )?;
        }
        if let Some(subscription) = subscription {
            let existing_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM twitch_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC LIMIT 1",
                    [guild_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing_id {
                tx.execute(
                    "UPDATE twitch_subscriptions SET source_login=?2,source_user_id=?3,target_channel_id=?4,message_template=?5,mention=?6,enabled=?7,pending_event_id=NULL,pending_stream_id=NULL,pending_started_at=NULL,last_event_id=NULL,next_poll_at=?8,failure_count=0,last_error=NULL,updated_at=?8 WHERE guild_id=?1 AND id=?9",
                    params![guild_id, subscription.source_login, subscription.source_user_id, subscription.target_channel_id, subscription.message_template, subscription.mention, if subscription.enabled { 1_i64 } else { 0_i64 }, i64::MAX, id],
                )?;
            } else if subscription.enabled {
                tx.execute(
                    "INSERT INTO twitch_subscriptions(guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?9)",
                    params![guild_id, subscription.source_login, subscription.source_user_id, subscription.target_channel_id, subscription.message_template, subscription.mention, i64::MAX, subscription.created_by, now],
                )?;
            }
        }
        let correlation_id = Uuid::new_v4().to_string();
        let before_json = current
            .as_ref()
            .map(|item| serde_json::json!({"enabled": item.1 != 0, "config": item.2, "revision": item.0}))
            .unwrap_or_else(|| serde_json::json!({}));
        let after_json = serde_json::json!({"enabled": enabled, "config": serde_json::from_str::<serde_json::Value>(config_json).unwrap_or_else(|_| serde_json::json!({})), "revision": next_revision});
        tx.execute(
            "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![correlation_id, guild_id, updated_by, "feature.publish", key, before_json.to_string(), after_json.to_string(), "published", now],
        )?;
        tx.commit()?;
        Ok(FeatureSettingRecord {
            guild_id: guild_id.to_string(),
            key: key.to_string(),
            enabled,
            config_json: config_json.to_string(),
            revision: next_revision,
            updated_at: now,
            updated_by: updated_by.to_string(),
        })
    }

    pub fn twitch_subscriptions(&self, guild_id: &str) -> Result<Vec<TwitchSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,pending_event_id,pending_stream_id,pending_started_at,last_event_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM twitch_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt.query_map([guild_id], twitch_subscription_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_twitch_subscription(
        &self,
        guild_id: &str,
        source_login: &str,
        source_user_id: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
        created_by: &str,
    ) -> Result<TwitchSubscriptionRecord> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO twitch_subscriptions(guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?8,?8)", params![guild_id, source_login, source_user_id, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, i64::MAX, created_by])
            .map_err(|error| if error.to_string().contains("UNIQUE") { anyhow!("twitch_subscription_exists") } else { error.into() })?;
        let id = conn.last_insert_rowid();
        conn.query_row("SELECT id,guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,pending_event_id,pending_stream_id,pending_started_at,last_event_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM twitch_subscriptions WHERE id=?1", [id], twitch_subscription_from_row).map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_twitch_subscription(
        &self,
        guild_id: &str,
        id: i64,
        source_login: &str,
        source_user_id: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
    ) -> Result<Option<TwitchSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("UPDATE twitch_subscriptions SET source_login=?3,source_user_id=?4,target_channel_id=?5,message_template=?6,mention=?7,enabled=?8,pending_event_id=NULL,pending_stream_id=NULL,pending_started_at=NULL,last_event_id=NULL,next_poll_at=?9,failure_count=0,last_error=NULL,updated_at=?9 WHERE guild_id=?1 AND id=?2", params![guild_id, id, source_login, source_user_id, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, i64::MAX])?;
        conn.query_row("SELECT id,guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,pending_event_id,pending_stream_id,pending_started_at,last_event_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM twitch_subscriptions WHERE guild_id=?1 AND id=?2", params![guild_id, id], twitch_subscription_from_row).optional()?.map_or(Ok(None), |record| Ok(Some(record)))
    }

    pub fn delete_twitch_subscription(&self, guild_id: &str, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM twitch_subscriptions WHERE guild_id=?1 AND id=?2",
            params![guild_id, id],
        )? > 0)
    }

    pub fn due_twitch_subscriptions(
        &self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<TwitchSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,source_login,source_user_id,target_channel_id,message_template,mention,enabled,pending_event_id,pending_stream_id,pending_started_at,last_event_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM twitch_subscriptions WHERE enabled=1 AND pending_event_id IS NOT NULL AND next_poll_at<=?1 ORDER BY next_poll_at ASC LIMIT ?2")?;
        let rows = stmt.query_map(
            params![now_ms, i64::from(limit.min(100))],
            twitch_subscription_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn stage_twitch_event(
        &self,
        source_user_id: &str,
        event_id: &str,
        stream_id: &str,
        started_at: &str,
    ) -> Result<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let now = Utc::now().timestamp_millis();
        Ok(conn.execute("UPDATE twitch_subscriptions SET pending_event_id=?2,pending_stream_id=?3,pending_started_at=?4,next_poll_at=?5,failure_count=0,last_error=NULL,updated_at=?5 WHERE source_user_id=?1 AND enabled=1 AND (pending_event_id IS NULL OR pending_event_id<>?2) AND (last_event_id IS NULL OR last_event_id<>?2)", params![source_user_id, event_id, stream_id, started_at, now])?)
    }

    pub fn ack_twitch_event(
        &self,
        id: i64,
        event_id: &str,
        next_poll_at: i64,
        failure_count: i64,
        last_error: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("UPDATE twitch_subscriptions SET pending_event_id=NULL,pending_stream_id=NULL,pending_started_at=NULL,last_event_id=?2,next_poll_at=?3,failure_count=?4,last_error=?5,updated_at=?3 WHERE id=?1 AND pending_event_id=?2", params![id, event_id, next_poll_at, failure_count, last_error])? > 0)
    }

    pub fn retry_twitch_event(
        &self,
        id: i64,
        event_id: &str,
        next_poll_at: i64,
        failure_count: i64,
        last_error: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute("UPDATE twitch_subscriptions SET next_poll_at=?3,failure_count=?4,last_error=?5,updated_at=?3 WHERE id=?1 AND pending_event_id=?2", params![id, event_id, next_poll_at, failure_count, last_error])? > 0)
    }

    pub fn rss_subscriptions(&self, guild_id: &str) -> Result<Vec<RssSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,last_item_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM rss_subscriptions WHERE guild_id=?1 ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt.query_map([guild_id], rss_subscription_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_rss_subscription(
        &self,
        guild_id: &str,
        feed_url: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
        interval_seconds: i64,
        created_by: &str,
    ) -> Result<RssSubscriptionRecord> {
        let now = Utc::now().timestamp_millis();
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("INSERT INTO rss_subscriptions(guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,next_poll_at,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?8,?8)", params![guild_id, feed_url, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, interval_seconds, now, created_by])
            .map_err(|error| if error.to_string().contains("UNIQUE") { anyhow!("rss_subscription_exists") } else { error.into() })?;
        let id = conn.last_insert_rowid();
        conn.query_row("SELECT id,guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,last_item_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM rss_subscriptions WHERE id=?1", [id], rss_subscription_from_row).map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_rss_subscription(
        &self,
        guild_id: &str,
        id: i64,
        feed_url: &str,
        target_channel_id: &str,
        message_template: &str,
        mention: &str,
        enabled: bool,
        interval_seconds: i64,
    ) -> Result<Option<RssSubscriptionRecord>> {
        let now = Utc::now().timestamp_millis();
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("UPDATE rss_subscriptions SET feed_url=?3,target_channel_id=?4,message_template=?5,mention=?6,enabled=?7,interval_seconds=?8,updated_at=?9 WHERE guild_id=?1 AND id=?2", params![guild_id, id, feed_url, target_channel_id, message_template, mention, if enabled { 1_i64 } else { 0_i64 }, interval_seconds, now])?;
        conn.query_row("SELECT id,guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,last_item_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM rss_subscriptions WHERE guild_id=?1 AND id=?2", params![guild_id, id], rss_subscription_from_row).optional()?.map_or(Ok(None), |record| Ok(Some(record)))
    }

    pub fn delete_rss_subscription(&self, guild_id: &str, id: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM rss_subscriptions WHERE guild_id=?1 AND id=?2",
            params![guild_id, id],
        )? > 0)
    }

    pub fn due_rss_subscriptions(
        &self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<RssSubscriptionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id,guild_id,feed_url,target_channel_id,message_template,mention,enabled,interval_seconds,last_item_id,next_poll_at,failure_count,last_error,created_by,created_at,updated_at FROM rss_subscriptions WHERE enabled=1 AND next_poll_at<=?1 ORDER BY next_poll_at ASC LIMIT ?2")?;
        let rows = stmt.query_map(
            params![now_ms, i64::from(limit.min(100))],
            rss_subscription_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_rss_poll(
        &self,
        id: i64,
        last_item_id: Option<&str>,
        next_poll_at: i64,
        failure_count: i64,
        last_error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("UPDATE rss_subscriptions SET last_item_id=?2,next_poll_at=?3,failure_count=?4,last_error=?5,updated_at=?3 WHERE id=?1", params![id, last_item_id, next_poll_at, failure_count, last_error])?;
        Ok(())
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

        let mut birthdays = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT user_id,month,day,created_at,updated_at FROM birthdays WHERE guild_id=?1 ORDER BY user_id",
        )?;
        for row in stmt.query_map([guild_id], |row| {
            Ok(serde_json::json!({
                "userId": row.get::<_, String>(0)?,
                "month": row.get::<_, u32>(1)?,
                "day": row.get::<_, u32>(2)?,
                "createdAt": row.get::<_, i64>(3)?,
                "updatedAt": row.get::<_, i64>(4)?,
            }))
        })? {
            birthdays.push(row?);
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
            "birthdays": birthdays,
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
             UNION ALL SELECT 'birthday' AS kind, printf('%02d-%02d', month, day), created_at FROM birthdays WHERE guild_id=?1 AND user_id=?2
             UNION ALL SELECT 'economy' AS kind, CAST(balance AS TEXT), created_at FROM economy_accounts WHERE guild_id=?1 AND user_id=?2
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
                "DELETE FROM suggestion_votes WHERE suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?2 AND author_id=?1) OR (user_id=?1 AND suggestion_id IN (SELECT id FROM suggestions WHERE guild_id=?2))",
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
                "voice_sessions",
                "DELETE FROM voice_sessions WHERE guild_id=?2 AND user_id=?1",
            ),
            (
                "birthdays",
                "DELETE FROM birthdays WHERE guild_id=?2 AND user_id=?1",
            ),
            (
                "economy_accounts",
                "DELETE FROM economy_accounts WHERE guild_id=?2 AND user_id=?1",
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
        let birthdays = object
            .get("birthdays")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if settings.len() > 200
            || tags.len() > 100
            || workflows.len() > 100
            || birthdays.len() > 500
        {
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

        let mut parsed_birthdays = Vec::with_capacity(birthdays.len());
        for item in &birthdays {
            let user_id = item
                .get("userId")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("invalid_birthday_user"))?
                .trim()
                .to_string();
            let month = item
                .get("month")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| anyhow!("invalid_birthday_month"))?;
            let day = item
                .get("day")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| anyhow!("invalid_birthday_day"))?;
            if !(1..=64).contains(&user_id.len())
                || !(1..=12).contains(&month)
                || !(1..=31).contains(&day)
            {
                bail!("invalid_birthday");
            }
            parsed_birthdays.push((user_id, month as u32, day as u32));
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
        for (user_id, month, day) in &parsed_birthdays {
            tx.execute(
                "INSERT INTO birthdays(guild_id,user_id,month,day,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(guild_id,user_id) DO UPDATE SET month=excluded.month,day=excluded.day,last_announced_year=NULL,updated_at=excluded.updated_at",
                params![guild_id, user_id, month, day, Utc::now().timestamp_millis()],
            )?;
        }
        tx.commit()?;
        Ok(ConfigImportSummary {
            settings: parsed_settings.len(),
            tags: parsed_tags.len(),
            workflows: parsed_workflows.len(),
            birthdays: parsed_birthdays.len(),
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
            "DELETE FROM audit_events WHERE guild_id=?1",
            "DELETE FROM activity_log WHERE guild_id=?1",
            "DELETE FROM scheduled_actions WHERE guild_id=?1",
            "DELETE FROM infractions WHERE guild_id=?1",
            "DELETE FROM afk WHERE guild_id=?1",
            "DELETE FROM tags WHERE guild_id=?1",
            "DELETE FROM levels WHERE guild_id=?1",
            "DELETE FROM voice_sessions WHERE guild_id=?1",
            "DELETE FROM birthdays WHERE guild_id=?1",
            "DELETE FROM economy_accounts WHERE guild_id=?1",
            "DELETE FROM temp_channels WHERE guild_id=?1",
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

    /// Apply bounded retention to operational data. The sweep is idempotent,
    /// guild-agnostic, and returns counts for an auditable scheduler log.
    pub fn prune_retention(&self, now_ms: i64) -> Result<RetentionSummary> {
        let now = DateTime::<Utc>::from_timestamp_millis(now_ms).unwrap_or_else(Utc::now);
        let cutoff_30d = (now - Duration::days(30)).timestamp_millis();
        let cutoff_90d = (now - Duration::days(90)).timestamp_millis();
        let cutoff_1y = (now - Duration::days(365)).timestamp_millis();
        let cutoff_2y = (now - Duration::days(730)).timestamp_millis();
        let cutoff_date_1y = (now - Duration::days(365)).format("%Y-%m-%d").to_string();
        let cutoff_rfc3339 = (now - Duration::hours(24)).to_rfc3339();
        let cutoff_seconds = (now - Duration::hours(24)).timestamp();

        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let mut deleted = serde_json::Map::new();
        let mut remove = |name: &str, count: usize| {
            if count > 0 {
                deleted.insert(name.to_string(), serde_json::json!(count));
            }
        };

        remove(
            "audit_events",
            tx.execute(
                "DELETE FROM audit_events WHERE created_at < ?1",
                [cutoff_30d],
            )?,
        );
        remove(
            "activity_log",
            tx.execute(
                "DELETE FROM activity_log WHERE created_at < ?1",
                [cutoff_90d],
            )?,
        );
        remove(
            "infractions",
            tx.execute("DELETE FROM infractions WHERE created_at < ?1", [cutoff_1y])?,
        );
        remove(
            "cases",
            tx.execute("DELETE FROM cases WHERE created_at < ?1", [cutoff_2y])?,
        );
        remove(
            "quarantine",
            tx.execute("DELETE FROM quarantine WHERE created_at < ?1", [cutoff_1y])?,
        );
        remove(
            "scheduled_actions",
            tx.execute(
                "DELETE FROM scheduled_actions WHERE execute_at < ?1",
                [cutoff_90d],
            )?,
        );
        remove(
            "tickets",
            tx.execute(
                "DELETE FROM tickets WHERE closed_at IS NOT NULL AND closed_at < ?1",
                [cutoff_1y],
            )?,
        );
        remove(
            "suggestion_votes",
            tx.execute(
                "DELETE FROM suggestion_votes WHERE suggestion_id IN (SELECT id FROM suggestions WHERE created_at < ?1)",
                [cutoff_1y],
            )?,
        );
        remove(
            "suggestions",
            tx.execute("DELETE FROM suggestions WHERE created_at < ?1", [cutoff_1y])?,
        );
        remove(
            "giveaway_entries",
            tx.execute(
                "DELETE FROM giveaway_entries WHERE giveaway_id IN (SELECT id FROM giveaways WHERE ended=1 AND end_at < ?1)",
                [cutoff_90d],
            )?,
        );
        remove(
            "giveaways",
            tx.execute(
                "DELETE FROM giveaways WHERE ended=1 AND end_at < ?1",
                [cutoff_90d],
            )?,
        );
        remove(
            "poll_votes",
            tx.execute(
                "DELETE FROM poll_votes WHERE poll_id IN (SELECT id FROM polls WHERE closed=1 AND end_at < ?1)",
                [cutoff_90d],
            )?,
        );
        remove(
            "polls",
            tx.execute(
                "DELETE FROM polls WHERE closed=1 AND end_at < ?1",
                [cutoff_90d],
            )?,
        );
        remove(
            "stats",
            tx.execute("DELETE FROM stats WHERE date < ?1", [&cutoff_date_1y])?,
        );
        // A gateway disconnect can leave a voice session without a matching
        // leave event. Sessions older than the bounded XP window are stale
        // and must not survive indefinitely or receive retroactive XP.
        remove(
            "voice_sessions",
            tx.execute(
                "DELETE FROM voice_sessions WHERE started_at < ?1",
                [cutoff_seconds],
            )?,
        );
        remove(
            "helper_session_guilds",
            tx.execute(
                "DELETE FROM helper_session_guilds WHERE session_id IN (SELECT id FROM helper_sessions WHERE expires_at < ?1 OR revoked_at < ?1)",
                [&cutoff_rfc3339],
            )?,
        );
        remove(
            "helper_sessions",
            tx.execute(
                "DELETE FROM helper_sessions WHERE expires_at < ?1 OR revoked_at < ?1",
                [&cutoff_rfc3339],
            )?,
        );
        remove(
            "helper_oauth_states",
            tx.execute(
                "DELETE FROM helper_oauth_states WHERE expires_at < ?1 OR used_at < ?1",
                [cutoff_seconds],
            )?,
        );
        tx.commit()?;
        Ok(RetentionSummary {
            deleted: serde_json::Value::Object(deleted),
        })
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

    /// Start or replace the active voice session for a member. Replaying the
    /// same gateway event is therefore harmless and a move between channels
    /// cannot leave two active sessions behind.
    pub fn start_voice_session(
        &self,
        guild_id: &str,
        user_id: &str,
        channel_id: &str,
        started_at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO voice_sessions(guild_id,user_id,channel_id,started_at) VALUES(?1,?2,?3,?4) ON CONFLICT(guild_id,user_id) DO UPDATE SET channel_id=excluded.channel_id, started_at=excluded.started_at",
            params![guild_id, user_id, channel_id, started_at],
        )?;
        Ok(())
    }

    /// Finish a voice session and return bounded whole minutes. The row is
    /// deleted in the same operation so duplicate leave events cannot award
    /// XP twice.
    pub fn finish_voice_session(
        &self,
        guild_id: &str,
        user_id: &str,
        ended_at: i64,
    ) -> Result<Option<i64>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let started: Option<i64> = conn
            .query_row(
                "SELECT started_at FROM voice_sessions WHERE guild_id=?1 AND user_id=?2",
                params![guild_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(started_at) = started else {
            return Ok(None);
        };
        conn.execute(
            "DELETE FROM voice_sessions WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
        )?;
        Ok(Some(((ended_at - started_at).max(0) / 60).min(24 * 60)))
    }

    pub fn active_voice_session(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<VoiceSessionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.query_row(
            "SELECT guild_id,user_id,channel_id,started_at FROM voice_sessions WHERE guild_id=?1 AND user_id=?2",
            params![guild_id, user_id],
            |row| {
                Ok(VoiceSessionRecord {
                    guild_id: row.get(0)?,
                    user_id: row.get(1)?,
                    channel_id: row.get(2)?,
                    started_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
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

    /// Position in the XP ranking (1 = highest XP). Users without a row have
    /// no rank and are represented as `None` by the caller.
    pub fn level_rank(&self, guild_id: &str, user_id: &str) -> Result<Option<u64>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let xp: Option<i64> = conn
            .query_row(
                "SELECT xp FROM levels WHERE guild_id=?1 AND user_id=?2",
                params![guild_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(xp) = xp else {
            return Ok(None);
        };
        let higher: i64 = conn.query_row(
            "SELECT COUNT(*) FROM levels WHERE guild_id=?1 AND xp>?2",
            params![guild_id, xp],
            |row| row.get(0),
        )?;
        Ok(Some((higher + 1).try_into().unwrap_or(u64::MAX)))
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
        let audit = store.recent_audit_events("guild", 10).unwrap();
        assert_eq!(audit.len(), 1);
        assert!(!audit[0].correlation_id.is_empty());
        assert_eq!(audit[0].actor_id, "mod");
        assert_eq!(audit[0].reason, "reason");
        assert_eq!(audit[0].outcome, "recorded");
        assert!(serde_json::from_str::<serde_json::Value>(&audit[0].before_json).is_ok());
        assert!(serde_json::from_str::<serde_json::Value>(&audit[0].after_json).is_ok());
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
    fn activity_log_is_metadata_only_and_bounded() {
        let store = Store::open(":memory:").unwrap();
        let oversized = "x".repeat(3_000);
        let id = store
            .record_activity(
                "guild",
                "message_edit",
                "user",
                Some("member#0001"),
                Some("user"),
                &oversized,
            )
            .unwrap();
        assert_eq!(id, 1);
        let rows = store.recent_activity("guild", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "message_edit");
        assert_eq!(rows[0].detail.chars().count(), 2_000);
        assert_eq!(rows[0].user_tag.as_deref(), Some("member#0001"));
    }

    #[test]
    fn sessions_revoke_and_entitlements_persist() {
        let store = Store::open(":memory:").unwrap();
        store.register_oauth_state("state", 10, "verifier").unwrap();
        assert_eq!(
            store.consume_oauth_state("state", 10).unwrap().as_deref(),
            Some("verifier")
        );
        assert!(store.consume_oauth_state("state", 10).unwrap().is_none());
        store
            .register_oauth_state("expired", 10, "verifier")
            .unwrap();
        assert!(store.consume_oauth_state("expired", 11).unwrap().is_none());
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
        store.add_xp("g", "other", 25).unwrap();
        assert_eq!(store.level_rank("g", "other").unwrap(), Some(1));
        assert_eq!(store.level_rank("g", "u").unwrap(), Some(2));
        assert_eq!(store.level_rank("g", "missing").unwrap(), None);
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
    fn feature_publish_is_revisioned_atomic_and_conflict_safe() {
        let store = Store::open(":memory:").unwrap();
        let first = store
            .publish_feature_setting(
                "guild-a",
                "protection.antispam",
                true,
                r#"{"floodCount":8}"#,
                None,
                "user-a",
                &[
                    ("feature.protection.antispam".into(), "true".into()),
                    ("security.antispam.flood_count".into(), "8".into()),
                ],
            )
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(
            store
                .get_setting("guild-a", "security.antispam.flood_count")
                .unwrap()
                .as_deref(),
            Some("8")
        );
        let conflict = store.publish_feature_setting(
            "guild-a",
            "protection.antispam",
            false,
            r#"{}"#,
            Some(0),
            "user-b",
            &[],
        );
        assert!(
            conflict
                .unwrap_err()
                .to_string()
                .starts_with("feature_revision_conflict:1")
        );
        let second = store
            .publish_feature_setting(
                "guild-a",
                "protection.antispam",
                false,
                r#"{}"#,
                Some(1),
                "user-b",
                &[("feature.protection.antispam".into(), "false".into())],
            )
            .unwrap();
        assert_eq!(second.revision, 2);
        assert_eq!(
            store
                .feature_revisions("guild-a", "protection.antispam", 10)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn youtube_feature_publish_keeps_subscription_and_revision_atomic() {
        let store = Store::open(":memory:").unwrap();
        let write = YouTubeSubscriptionWrite {
            source_channel_id: "UC_abc-123".into(),
            target_channel_id: "123456789012345678".into(),
            message_template: "New video: {url}".into(),
            mention: String::new(),
            enabled: true,
            interval_seconds: 300,
            created_by: "user-a".into(),
        };
        let first = store
            .publish_youtube_feature_setting(
                "guild-a",
                "social.youtube",
                true,
                r#"{"sourceChannelId":"UC_abc-123"}"#,
                None,
                "user-a",
                &[],
                Some(&write),
            )
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(store.youtube_subscriptions("guild-a").unwrap().len(), 1);

        let conflict = store.publish_youtube_feature_setting(
            "guild-a",
            "social.youtube",
            false,
            "{}",
            Some(0),
            "user-b",
            &[],
            Some(&YouTubeSubscriptionWrite {
                enabled: false,
                ..write.clone()
            }),
        );
        assert!(
            conflict
                .unwrap_err()
                .to_string()
                .starts_with("feature_revision_conflict:1")
        );
        assert!(store.youtube_subscriptions("guild-a").unwrap()[0].enabled);

        let second = store
            .publish_youtube_feature_setting(
                "guild-a",
                "social.youtube",
                false,
                "{}",
                Some(1),
                "user-b",
                &[],
                Some(&YouTubeSubscriptionWrite {
                    enabled: false,
                    ..write
                }),
            )
            .unwrap();
        assert_eq!(second.revision, 2);
        assert!(!store.youtube_subscriptions("guild-a").unwrap()[0].enabled);
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
        store.set_birthday("g1", "u1", 8, 3).unwrap();
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
        assert_eq!(export["birthdays"][0]["userId"], "u1");

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
    fn voice_sessions_are_idempotent_and_bounded() {
        let store = Store::open(":memory:").unwrap();
        store
            .start_voice_session("g1", "u1", "voice-a", 1_000)
            .unwrap();
        // A repeated gateway event replaces the snapshot rather than creating
        // a second session for the same member.
        store
            .start_voice_session("g1", "u1", "voice-a", 1_030)
            .unwrap();
        assert_eq!(
            store
                .active_voice_session("g1", "u1")
                .unwrap()
                .unwrap()
                .channel_id,
            "voice-a"
        );
        assert_eq!(
            store.finish_voice_session("g1", "u1", 4_690).unwrap(),
            Some(61)
        );
        // Finishing twice cannot award the same session twice.
        assert_eq!(store.finish_voice_session("g1", "u1", 9_000).unwrap(), None);

        store
            .start_voice_session("g1", "u1", "voice-b", 2_000)
            .unwrap();
        // Negative clock movement produces no negative XP.
        assert_eq!(
            store.finish_voice_session("g1", "u1", 1_900).unwrap(),
            Some(0)
        );
        store.start_voice_session("g1", "u1", "voice-c", 0).unwrap();
        assert_eq!(
            store.finish_voice_session("g1", "u1", 99_999_999).unwrap(),
            Some(1_440)
        );
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
        store.set_birthday("source", "u1", 8, 3).unwrap();
        store
            .create_workflow("source", "hello", "message", "hi", "reply", "Hello")
            .unwrap();
        let export = store.export_guild("source").unwrap();

        let imported = store.import_guild_config("target", &export).unwrap();
        assert_eq!(imported.settings, 1);
        assert_eq!(imported.tags, 1);
        assert_eq!(imported.workflows, 1);
        assert_eq!(imported.birthdays, 1);
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

        assert_eq!(
            store.export_guild("target").unwrap()["birthdays"][0]["userId"],
            "u1"
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
        store
            .start_voice_session("g1", "u1", "voice", Utc::now().timestamp())
            .unwrap();
        store.upsert_tag("g1", "mine", "hello", "u1").unwrap();
        store.set_birthday("g1", "u1", 8, 3).unwrap();
        let suggestion = store.create_suggestion("g1", "u1", "feature").unwrap();
        store.vote_suggestion(suggestion, "u2", 1).unwrap();
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
        assert!(store.due_birthdays(8, 3, 2026, 10).unwrap().is_empty());
        assert!(store.get_afk("g1", "u1").unwrap().is_none());
        assert!(store.active_voice_session("g1", "u1").unwrap().is_none());
        assert_eq!(store.suggestion_votes(suggestion).unwrap(), (0, 0));
    }

    #[test]
    fn retention_sweep_removes_expired_records_and_keeps_recent_cases() {
        let store = Store::open(":memory:").unwrap();
        let now = Utc::now().timestamp_millis();
        let old_31d = now - Duration::days(31).num_milliseconds();
        let old_91d = now - Duration::days(91).num_milliseconds();
        let old_3y = now - Duration::days(1_095).num_milliseconds();
        store
            .record_case("g", "warn", "recent", "mod", "keep", None)
            .unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO audit_events(correlation_id,guild_id,actor_id,action,reason,before_json,after_json,outcome,created_at) VALUES('old-correlation','g','mod','old','old','{}','{}','recorded',?1)",
                [old_31d],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO activity_log(guild_id,type,user_id,actor_id,detail,created_at) VALUES('g','leave','u','mod','{}',?1)",
                [old_91d],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cases(guild_id,type,target_id,moderator_id,reason,created_at) VALUES('g','old','u','mod','old',?1)",
                [old_3y],
            )
            .unwrap();
        }
        let summary = store.prune_retention(now).unwrap();
        assert_eq!(summary.deleted["audit_events"], 1);
        assert_eq!(summary.deleted["activity_log"], 1);
        assert_eq!(summary.deleted["cases"], 1);
        assert_eq!(store.recent_cases("g", 10).unwrap().len(), 1);
        assert_eq!(store.recent_audit_events("g", 10).unwrap().len(), 1);
    }

    #[test]
    fn youtube_subscriptions_round_trip_and_due_poll() {
        let store = Store::open(":memory:").unwrap();
        let record = store
            .create_youtube_subscription(
                "g",
                "UC123",
                "123456789012345",
                "{title} {url}",
                "",
                true,
                300,
                "u",
            )
            .unwrap();
        assert_eq!(store.youtube_subscriptions("g").unwrap().len(), 1);
        assert_eq!(
            store
                .due_youtube_subscriptions(Utc::now().timestamp_millis(), 10)
                .unwrap()
                .len(),
            1
        );
        store
            .update_youtube_poll(
                record.id,
                Some("video-1"),
                Utc::now().timestamp_millis() + 60_000,
                0,
                None,
            )
            .unwrap();
        let saved = store.youtube_subscriptions("g").unwrap().pop().unwrap();
        assert_eq!(saved.last_video_id.as_deref(), Some("video-1"));
        assert!(store.delete_youtube_subscription("g", record.id).unwrap());
        assert!(store.youtube_subscriptions("g").unwrap().is_empty());
    }

    #[test]
    fn rss_subscriptions_round_trip_and_due_poll() {
        let store = Store::open(":memory:").unwrap();
        let record = store
            .create_rss_subscription(
                "g",
                "https://example.com/feed.xml",
                "123456789012345",
                "{title} {url}",
                "",
                true,
                300,
                "u",
            )
            .unwrap();
        assert_eq!(store.rss_subscriptions("g").unwrap().len(), 1);
        assert_eq!(
            store
                .due_rss_subscriptions(Utc::now().timestamp_millis(), 10)
                .unwrap()
                .len(),
            1
        );
        store
            .update_rss_poll(
                record.id,
                Some("item-1"),
                Utc::now().timestamp_millis() + 60_000,
                0,
                None,
            )
            .unwrap();
        let saved = store.rss_subscriptions("g").unwrap().pop().unwrap();
        assert_eq!(saved.last_item_id.as_deref(), Some("item-1"));
        assert!(store.delete_rss_subscription("g", record.id).unwrap());
        assert!(store.rss_subscriptions("g").unwrap().is_empty());
    }

    #[test]
    fn twitch_subscriptions_stage_and_ack_are_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let record = store
            .create_twitch_subscription(
                "g",
                "creator",
                "12345",
                "123456789012345",
                "{broadcaster} {url}",
                "",
                true,
                "u",
            )
            .unwrap();
        assert_eq!(
            store
                .stage_twitch_event("12345", "event-1", "stream-1", "2026-08-02T00:00:00Z")
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .stage_twitch_event("12345", "event-1", "stream-1", "2026-08-02T00:00:00Z")
                .unwrap(),
            0
        );
        let due = store
            .due_twitch_subscriptions(Utc::now().timestamp_millis(), 10)
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].pending_event_id.as_deref(), Some("event-1"));
        assert!(
            store
                .ack_twitch_event(record.id, "event-1", i64::MAX, 0, None)
                .unwrap()
        );
        assert!(
            store
                .due_twitch_subscriptions(Utc::now().timestamp_millis(), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn member_departure_deletes_only_voluntary_state() {
        let store = Store::open(":memory:").unwrap();
        store.set_afk("g", "u", "away").unwrap();
        store.add_xp("g", "u", 42).unwrap();
        store.upsert_tag("g", "mine", "hello", "u").unwrap();
        store.set_birthday("g", "u", 8, 3).unwrap();
        let suggestion = store.create_suggestion("g", "u", "remove me").unwrap();
        store.vote_suggestion(suggestion, "u", 1).unwrap();
        store.register_event("g", "event-1", "u").unwrap();
        store
            .record_case("g", "warn", "u", "mod", "kept", None)
            .unwrap();
        let deleted = store.delete_member_voluntary_data("g", "u").unwrap();
        assert_eq!(deleted["levels"], 1);
        assert_eq!(deleted["afk"], 1);
        assert_eq!(deleted["tags"], 1);
        assert_eq!(deleted["birthdays"], 1);
        assert_eq!(deleted["suggestions"], 1);
        assert_eq!(deleted["event_registrations"], 1);
        assert_eq!(store.level_for("g", "u").unwrap(), 0);
        assert!(store.get_afk("g", "u").unwrap().is_none());
        assert!(store.get_tag("g", "mine").unwrap().is_none());
        assert!(store.due_birthdays(8, 3, 2026, 10).unwrap().is_empty());
        assert_eq!(store.suggestion_votes(suggestion).unwrap(), (0, 0));
        assert_eq!(store.recent_cases("g", 10).unwrap().len(), 1);
    }

    #[test]
    fn birthdays_are_idempotent_per_year() {
        let store = Store::open(":memory:").unwrap();
        store.set_birthday("g", "u", 8, 3).unwrap();
        assert_eq!(store.due_birthdays(8, 3, 2026, 10).unwrap().len(), 1);
        assert!(store.mark_birthday_announced("g", "u", 2026).unwrap());
        assert!(store.due_birthdays(8, 3, 2026, 10).unwrap().is_empty());
        assert_eq!(store.due_birthdays(8, 3, 2027, 10).unwrap().len(), 1);
        assert!(store.remove_birthday("g", "u").unwrap());
        assert!(store.due_birthdays(8, 3, 2027, 10).unwrap().is_empty());
    }

    #[test]
    fn economy_daily_is_atomic_and_rate_limited() {
        let store = Store::open(":memory:").unwrap();
        assert_eq!(store.economy_account("g", "u").unwrap().balance, 0);
        let first = store.claim_daily("g", "u", 250).unwrap().unwrap();
        assert_eq!(first.balance, 250);
        assert!(store.claim_daily("g", "u", 250).unwrap().is_none());
        assert_eq!(store.economy_account("g", "u").unwrap().balance, 250);
        store.purge_user("g", "u").unwrap();
        assert_eq!(store.economy_account("g", "u").unwrap().balance, 0);
    }

    #[test]
    fn temporary_channels_are_idempotent_and_removable() {
        let store = Store::open(":memory:").unwrap();
        store.register_temp_channel("g", "c", "u").unwrap();
        store.register_temp_channel("g", "c", "u").unwrap();
        assert_eq!(store.temp_channel("c").unwrap().unwrap().owner_id, "u");
        assert_eq!(store.active_temp_channels("g").unwrap(), 1);
        assert!(store.remove_temp_channel("c").unwrap());
        assert!(store.temp_channel("c").unwrap().is_none());
        assert_eq!(store.active_temp_channels("g").unwrap(), 0);
        assert!(!store.remove_temp_channel("c").unwrap());
    }
}
