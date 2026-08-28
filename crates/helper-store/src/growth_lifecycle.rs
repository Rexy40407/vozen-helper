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
    pub joins: i64,
    pub leaves: i64,
    pub net: i64,
    pub setup_rate: f64,
    pub activation_rate: f64,
    pub retained_w7: Option<f64>,
    pub retained_w30: Option<f64>,
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
            "INSERT INTO helper_growth_activity_day(guild_id,day) VALUES(?1,?2) ON CONFLICT(guild_id,day) DO NOTHING",
            params![guild_id, day],
        )? == 1 {
            increment(&tx, &day, &source, "active")?;
        }
        tx.commit()?;
        drop(conn);
        self.record_growth_first_value(guild_id, now_ms)
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
        Ok(GrowthOverview {
            active_guilds,
            joins,
            leaves,
            net: joins - leaves,
            setup_rate: setup as f64 / denominator,
            activation_rate: activated as f64 / denominator,
            retained_w7: retention_rate(&conn, now_ms, 7)?,
            retained_w30: retention_rate(&conn, now_ms, 30)?,
            daily,
        })
    }

    pub fn purge_growth_lifecycle(&self, now_ms: i64) -> Result<usize> {
        let cutoff = now_ms - Duration::days(RETENTION_DAYS).num_milliseconds();
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM helper_growth_lifecycle WHERE departed_at IS NOT NULL AND departed_at < ?1",
            [cutoff],
        )?)
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

fn retention_rate(conn: &rusqlite::Connection, now_ms: i64, days: i64) -> Result<Option<f64>> {
    let cutoff = now_ms - Duration::days(days).num_milliseconds();
    let cohort: i64 = conn.query_row(
        "SELECT COUNT(*) FROM helper_growth_lifecycle WHERE first_joined_at<=?1",
        [cutoff],
        |row| row.get(0),
    )?;
    if cohort == 0 {
        return Ok(None);
    }
    let retained: i64 = conn.query_row(
        "SELECT COUNT(*) FROM helper_growth_lifecycle WHERE first_joined_at<=?1 AND departed_at IS NULL AND last_activity_at>=?1",
        [cutoff],
        |row| row.get(0),
    )?;
    Ok(Some(retained as f64 / cohort as f64))
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
    }

    #[test]
    fn only_allowlisted_sources_are_retained() {
        assert_eq!(growth_source("helper-hero"), Some("helper-hero"));
        assert_eq!(growth_source("https://attacker.invalid"), None);
    }
}
