//! Privacy-bounded acquisition and activation metrics for Vozen Helper.
//!
//! This module never exposes guild identifiers through an aggregate API.  The
//! lifecycle row exists only long enough to calculate activation and retention
//! and is erased thirty days after a bot leaves the guild.

use super::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, params};

const RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthDailyMetric {
    pub day: String,
    pub source: String,
    pub event: String,
    pub value: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthOverview {
    pub active_guilds: i64,
    pub configured_guilds: i64,
    pub used_guilds: i64,
    pub joins: i64,
    pub leaves: i64,
    pub net: i64,
    pub setup_rate: f64,
    pub activation_rate: f64,
    pub retained_w7_count: i64,
    pub eligible_w7: i64,
    pub retained_w30_count: i64,
    pub eligible_w30: i64,
    pub retained_w7: Option<f64>,
    pub retained_w30: Option<f64>,
    pub measurement_started_on: Option<String>,
    pub daily: Vec<GrowthDailyMetric>,
}

impl Store {
    pub fn record_growth_install(&self, guild_id: &str, source: &str, now_ms: i64) -> Result<()> {
        let source = growth_source(source).unwrap_or("unknown");
        self.record_growth_join_inner(guild_id, source, now_ms)
    }

    pub fn record_growth_join(&self, guild_id: &str, now_ms: i64) -> Result<()> {
        self.record_growth_join_inner(guild_id, "unknown", now_ms)
    }

    fn record_growth_join_inner(&self, guild_id: &str, source: &str, now_ms: i64) -> Result<()> {
        if guild_id.trim().is_empty() {
            return Ok(());
        }
        let day = utc_day(now_ms);
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let previous_departure: Option<Option<i64>> = tx
            .query_row(
                "SELECT departed_at FROM helper_growth_lifecycle WHERE guild_id=?1",
                [guild_id],
                |row| row.get(0),
            )
            .optional()?;
        let is_new_or_rejoined =
            previous_departure.is_none() || previous_departure.flatten().is_some();
        tx.execute(
            "INSERT INTO helper_growth_lifecycle(guild_id,first_joined_at,last_joined_at,install_source,departed_at) \
             VALUES(?1,?2,?2,?3,NULL) \
             ON CONFLICT(guild_id) DO UPDATE SET \
               last_joined_at=excluded.last_joined_at, \
               install_source=CASE WHEN helper_growth_lifecycle.install_source='unknown' THEN excluded.install_source ELSE helper_growth_lifecycle.install_source END, \
               departed_at=NULL",
            params![guild_id, now_ms, source],
        )?;
        if is_new_or_rejoined {
            increment(&tx, &day, source, "join")?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_growth_departure(&self, guild_id: &str, now_ms: i64) -> Result<()> {
        let day = utc_day(now_ms);
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let source: Option<String> = tx
            .query_row(
                "SELECT install_source FROM helper_growth_lifecycle WHERE guild_id=?1 AND departed_at IS NULL",
                [guild_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(source) = source {
            tx.execute(
                "UPDATE helper_growth_lifecycle SET departed_at=?2 WHERE guild_id=?1 AND departed_at IS NULL",
                params![guild_id, now_ms],
            )?;
            increment(&tx, &day, &source, "leave")?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_growth_setup_completed(&self, guild_id: &str, now_ms: i64) -> Result<()> {
        self.record_growth_once(guild_id, now_ms, "setup_completed", "setup_completed_at")
    }

    pub fn record_growth_first_value(&self, guild_id: &str, now_ms: i64) -> Result<()> {
        self.record_growth_once(guild_id, now_ms, "first_value", "first_value_at")
    }

    fn record_growth_once(
        &self,
        guild_id: &str,
        now_ms: i64,
        event: &str,
        column: &str,
    ) -> Result<()> {
        if guild_id.trim().is_empty() {
            return Ok(());
        }
        let day = utc_day(now_ms);
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO helper_growth_lifecycle(guild_id,first_joined_at,last_joined_at,install_source,departed_at) \
             VALUES(?1,?2,?2,'unknown',NULL) ON CONFLICT(guild_id) DO NOTHING",
            params![guild_id, now_ms],
        )?;
        let source: String = tx.query_row(
            "SELECT install_source FROM helper_growth_lifecycle WHERE guild_id=?1",
            [guild_id],
            |row| row.get(0),
        )?;
        let changed = tx.execute(
            &format!("UPDATE helper_growth_lifecycle SET {column}=?2 WHERE guild_id=?1 AND {column} IS NULL"),
            params![guild_id, now_ms],
        )?;
        if changed == 1 {
            increment(&tx, &day, &source, event)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_growth_activity(&self, guild_id: &str, now_ms: i64) -> Result<()> {
        if guild_id.trim().is_empty() {
            return Ok(());
        }
        let day = utc_day(now_ms);
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO helper_growth_lifecycle(guild_id,first_joined_at,last_joined_at,install_source,last_activity_at,departed_at) \
             VALUES(?1,?2,?2,'unknown',?2,NULL) \
             ON CONFLICT(guild_id) DO UPDATE SET last_activity_at=excluded.last_activity_at",
            params![guild_id, now_ms],
        )?;
        let source: String = tx.query_row(
            "SELECT install_source FROM helper_growth_lifecycle WHERE guild_id=?1",
            [guild_id],
            |row| row.get(0),
        )?;
        if tx.execute(
            "UPDATE helper_growth_lifecycle SET first_value_at=?2
             WHERE guild_id=?1 AND first_value_at IS NULL",
            params![guild_id, now_ms],
        )? == 1
        {
            increment(&tx, &day, &source, "first_value")?;
        }
        if tx.execute(
            "INSERT INTO helper_growth_activity_day(guild_id,day) VALUES(?1,?2) ON CONFLICT(guild_id,day) DO NOTHING",
            params![guild_id, day],
        )? == 1 {
            increment(&tx, &day, &source, "active")?;
        }
        record_due_retention(&tx, guild_id, now_ms, &source)?;
        tx.commit()?;
        Ok(())
    }

    pub fn growth_overview(
        &self,
        from_day: &str,
        to_day: &str,
        now_ms: i64,
    ) -> Result<GrowthOverview> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let active_guilds: i64 = conn.query_row(
            "SELECT COUNT(*) FROM helper_growth_lifecycle WHERE departed_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let joins = sum_event(&conn, from_day, to_day, "join")?;
        let leaves = sum_event(&conn, from_day, to_day, "leave")?;
        let activated: i64 = conn.query_row(
            "SELECT COUNT(*) FROM helper_growth_lifecycle WHERE first_value_at IS NOT NULL AND departed_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let setup: i64 = conn.query_row(
            "SELECT COUNT(*) FROM helper_growth_lifecycle WHERE setup_completed_at IS NOT NULL AND departed_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let denominator = active_guilds.max(1) as f64;
        let daily = {
            let mut statement = conn.prepare(
                "SELECT day,source,event,value FROM helper_growth_daily_metric WHERE day>=?1 AND day<=?2 ORDER BY day,source,event",
            )?;
            statement
                .query_map(params![from_day, to_day], |row| {
                    Ok(GrowthDailyMetric {
                        day: row.get(0)?,
                        source: row.get(1)?,
                        event: row.get(2)?,
                        value: row.get(3)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let eligible_w7 = eligible_count(&conn, now_ms, 7)?;
        let retained_w7_count = lifetime_sum_event(&conn, "retained_w7")?;
        let eligible_w30 = eligible_count(&conn, now_ms, 30)?;
        let retained_w30_count = lifetime_sum_event(&conn, "retained_w30")?;
        let measurement_started_on = conn.query_row(
            "SELECT MIN(day) FROM helper_growth_daily_metric",
            [],
            |row| row.get(0),
        )?;
        Ok(GrowthOverview {
            active_guilds,
            configured_guilds: setup,
            used_guilds: activated,
            joins,
            leaves,
            net: joins - leaves,
            setup_rate: setup as f64 / denominator,
            activation_rate: activated as f64 / denominator,
            retained_w7_count,
            eligible_w7,
            retained_w30_count,
            eligible_w30,
            retained_w7: retention_rate(retained_w7_count, eligible_w7),
            retained_w30: retention_rate(retained_w30_count, eligible_w30),
            measurement_started_on,
            daily,
        })
    }

    pub fn purge_growth_lifecycle(&self, now_ms: i64) -> Result<usize> {
        let cutoff = now_ms - Duration::days(RETENTION_DAYS).num_milliseconds();
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        for table in [
            "helper_growth_activity_day",
            "helper_growth_retention_record",
        ] {
            tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE guild_id IN
                     (SELECT guild_id FROM helper_growth_lifecycle
                      WHERE departed_at IS NOT NULL AND departed_at < ?1)"
                ),
                [cutoff],
            )?;
        }
        let deleted = tx.execute(
            "DELETE FROM helper_growth_lifecycle WHERE departed_at IS NOT NULL AND departed_at < ?1",
            [cutoff],
        )?;
        tx.commit()?;
        Ok(deleted)
    }
}

fn increment(tx: &rusqlite::Transaction<'_>, day: &str, source: &str, event: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO helper_growth_daily_metric(day,source,event,value) VALUES(?1,?2,?3,1) \
         ON CONFLICT(day,source,event) DO UPDATE SET value=value+1",
        params![day, source, event],
    )?;
    Ok(())
}

fn sum_event(
    conn: &rusqlite::Connection,
    from_day: &str,
    to_day: &str,
    event: &str,
) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(value),0) FROM helper_growth_daily_metric WHERE day>=?1 AND day<=?2 AND event=?3",
        params![from_day, to_day, event],
        |row| row.get(0),
    )?)
}

fn lifetime_sum_event(conn: &rusqlite::Connection, event: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(value),0) FROM helper_growth_daily_metric WHERE event=?1",
        [event],
        |row| row.get(0),
    )?)
}

fn eligible_count(conn: &rusqlite::Connection, now_ms: i64, days: i64) -> Result<i64> {
    let cutoff_day = utc_day(now_ms.saturating_sub(Duration::days(days).num_milliseconds()));
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(value),0) FROM helper_growth_daily_metric
         WHERE event='first_value' AND day<=?1",
        [cutoff_day],
        |row| row.get(0),
    )?)
}

fn retention_rate(retained: i64, eligible: i64) -> Option<f64> {
    (eligible > 0).then_some(retained as f64 / eligible as f64)
}

fn record_due_retention(
    tx: &rusqlite::Transaction<'_>,
    guild_id: &str,
    now_ms: i64,
    source: &str,
) -> Result<()> {
    let first_value_at: Option<i64> = tx.query_row(
        "SELECT first_value_at FROM helper_growth_lifecycle WHERE guild_id=?1",
        [guild_id],
        |row| row.get(0),
    )?;
    let Some(first_value_at) = first_value_at else {
        return Ok(());
    };
    let day = utc_day(now_ms);
    for (window_days, event) in [(7_i64, "retained_w7"), (30_i64, "retained_w30")] {
        let threshold =
            first_value_at.saturating_add(Duration::days(window_days).num_milliseconds());
        if now_ms < threshold {
            continue;
        }
        if tx.execute(
            "INSERT OR IGNORE INTO helper_growth_retention_record(guild_id,window_days)
             VALUES(?1,?2)",
            params![guild_id, window_days],
        )? == 1
        {
            increment(tx, &day, source, event)?;
        }
    }
    Ok(())
}

pub(super) fn backfill_growth_retention(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let candidates = {
        let mut statement = tx.prepare(
            "SELECT guild_id,first_value_at,last_activity_at,install_source
             FROM helper_growth_lifecycle
             WHERE first_value_at IS NOT NULL AND last_activity_at IS NOT NULL",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (guild_id, first_value_at, last_activity_at, source) in candidates {
        for (window_days, event) in [(7_i64, "retained_w7"), (30_i64, "retained_w30")] {
            let threshold =
                first_value_at.saturating_add(Duration::days(window_days).num_milliseconds());
            if last_activity_at < threshold {
                continue;
            }
            if tx.execute(
                "INSERT OR IGNORE INTO helper_growth_retention_record(guild_id,window_days)
                 VALUES(?1,?2)",
                params![guild_id, window_days],
            )? == 1
            {
                increment(tx, &utc_day(last_activity_at), &source, event)?;
            }
        }
    }
    Ok(())
}

fn utc_day(now_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(now_ms)
        .unwrap_or_else(Utc::now)
        .format("%Y-%m-%d")
        .to_string()
}

pub fn growth_source(value: &str) -> Option<&'static str> {
    match value {
        "home" => Some("home"),
        "helper-hero" => Some("helper-hero"),
        "helper-pricing" => Some("helper-pricing"),
        "commands" => Some("commands"),
        "topgg" => Some("topgg"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn records_one_activation_funnel_and_purges_departed_guilds() {
        let store = Store::open(":memory:").expect("store");
        let now = 1_800_000_000_000_i64;
        store
            .record_growth_install("g1", "helper-hero", now)
            .unwrap();
        store
            .record_growth_install("g1", "helper-hero", now)
            .unwrap();
        store.record_growth_setup_completed("g1", now + 1).unwrap();
        store.record_growth_activity("g1", now + 2).unwrap();
        store.record_growth_activity("g1", now + 3).unwrap();
        let overview = store
            .growth_overview("2027-01-15", "2027-01-15", now + 2)
            .unwrap();
        assert_eq!((overview.joins, overview.leaves, overview.net), (1, 0, 1));
        assert_eq!(
            overview
                .daily
                .iter()
                .filter(|row| row.event == "active")
                .map(|row| row.value)
                .sum::<i64>(),
            1
        );
        store.record_growth_departure("g1", now + 4).unwrap();
        assert_eq!(
            store
                .purge_growth_lifecycle(now + Duration::days(31).num_milliseconds())
                .unwrap(),
            1
        );
        let conn = store.conn.lock().expect("store mutex");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM helper_growth_activity_day",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .expect("activity identities"),
            0
        );
    }

    #[test]
    fn only_allowlisted_sources_are_retained() {
        assert_eq!(growth_source("helper-hero"), Some("helper-hero"));
        assert_eq!(growth_source("https://attacker.invalid"), None);
    }

    #[test]
    fn retention_outcomes_survive_the_required_guild_identity_purge() {
        let store = Store::open(":memory:").expect("store");
        let start = 1_800_000_000_000_i64;
        let day = Duration::days(1).num_milliseconds();
        for guild_id in ["week", "month"] {
            store
                .record_growth_install(guild_id, "home", start)
                .expect("install");
            store
                .record_growth_activity(guild_id, start)
                .expect("activate");
        }
        store
            .record_growth_activity("week", start + 8 * day)
            .expect("week return");
        store
            .record_growth_activity("month", start + 31 * day)
            .expect("month return");

        let before = store
            .growth_overview("2027-01-01", "2027-03-31", start + 40 * day)
            .expect("overview before purge");
        assert_eq!(before.retained_w7, Some(1.0));
        assert_eq!(before.retained_w30, Some(0.5));

        store.purge_guild("week").expect("purge week");
        store.purge_guild("month").expect("purge month");
        let after = store
            .growth_overview("2027-01-01", "2027-03-31", start + 40 * day)
            .expect("overview after purge");
        assert_eq!(after.retained_w7, Some(1.0));
        assert_eq!(after.retained_w30, Some(0.5));
        assert_eq!(
            store
                .conn
                .lock()
                .expect("store mutex")
                .query_row(
                    "SELECT COUNT(*) FROM helper_growth_retention_record",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("retention identities"),
            0
        );
    }

    #[test]
    fn migration_backfills_existing_retention_outcomes_once() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vozen-helper-growth-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let start = 1_800_000_000_000_i64;
        let day = Duration::days(1).num_milliseconds();
        let store = Store::open(&path).expect("initial store");
        store
            .conn
            .lock()
            .expect("store mutex")
            .execute_batch(&format!(
                "INSERT INTO helper_growth_lifecycle
                   (guild_id,first_joined_at,last_joined_at,install_source,
                    first_value_at,last_activity_at)
                 VALUES
                   ('week',{start},{start},'home',{start},{}),
                   ('month',{start},{start},'home',{start},{});
                 INSERT INTO helper_growth_daily_metric(day,source,event,value)
                 VALUES('{}','home','first_value',2);",
                start + 8 * day,
                start + 31 * day,
                utc_day(start),
            ))
            .expect("historical lifecycle");
        drop(store);

        for _ in 0..2 {
            let reopened = Store::open(&path).expect("reopen migrated store");
            let overview = reopened
                .growth_overview("2027-01-01", "2027-03-31", start + 40 * day)
                .expect("overview");
            assert_eq!(
                (
                    overview.eligible_w7,
                    overview.retained_w7_count,
                    overview.eligible_w30,
                    overview.retained_w30_count,
                ),
                (2, 2, 2, 1)
            );
            assert_eq!(
                reopened
                    .conn
                    .lock()
                    .expect("store mutex")
                    .query_row(
                        "SELECT COUNT(*) FROM helper_growth_retention_record",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .expect("retention records"),
                3
            );
            drop(reopened);
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
