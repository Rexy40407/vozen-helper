//! Aggregate-only Top.gg delivery health for the private Helper tracker.
//!
//! The record deliberately excludes provider responses, tokens and guild IDs.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::Store;

pub const TOPGG_STALE_AFTER_MS: i64 = 90 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TopggSyncDetail {
    Unconfigured,
    Delivered,
    V1AuthenticationFailed,
    ProjectNotFound,
    InvalidMetricsPayload,
    RateLimited,
    HttpFailure,
    TransportFailure,
    InvalidConfiguration,
    Unknown,
}

impl TopggSyncDetail {
    pub const fn as_storage(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::Delivered => "delivered",
            Self::V1AuthenticationFailed => "v1_authentication_failed",
            Self::ProjectNotFound => "project_not_found",
            Self::InvalidMetricsPayload => "invalid_metrics_payload",
            Self::RateLimited => "rate_limited",
            Self::HttpFailure => "http_failure",
            Self::TransportFailure => "transport_failure",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::Unknown => "unknown",
        }
    }

    fn from_storage(value: &str) -> Self {
        match value {
            "unconfigured" => Self::Unconfigured,
            "delivered" => Self::Delivered,
            "v1_authentication_failed" => Self::V1AuthenticationFailed,
            "project_not_found" => Self::ProjectNotFound,
            "invalid_metrics_payload" => Self::InvalidMetricsPayload,
            "rate_limited" => Self::RateLimited,
            "http_failure" => Self::HttpFailure,
            "transport_failure" => Self::TransportFailure,
            "invalid_configuration" => Self::InvalidConfiguration,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopggSyncStatus {
    pub last_attempt_at: i64,
    pub last_success_at: Option<i64>,
    pub last_status: Option<u16>,
    pub last_server_count: Option<i64>,
    pub last_detail: TopggSyncDetail,
    pub consecutive_failures: i64,
    pub stale: bool,
}

impl Store {
    /// Stores bounded operational context only: no secret or remote response body is retained.
    pub fn record_topgg_sync_attempt(
        &self,
        now_ms: i64,
        status: Option<u16>,
        server_count: Option<usize>,
        succeeded: bool,
        detail: TopggSyncDetail,
    ) -> Result<()> {
        let count = server_count.map(i64::try_from).transpose()?;
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO helper_topgg_sync_state
               (singleton,last_attempt_at,last_success_at,last_status,last_server_count,last_detail,consecutive_failures)
             VALUES(1,?1,?2,?3,?4,?5,?6)
             ON CONFLICT(singleton) DO UPDATE SET
               last_attempt_at=excluded.last_attempt_at,
               last_success_at=CASE WHEN ?7 THEN excluded.last_attempt_at ELSE helper_topgg_sync_state.last_success_at END,
               last_status=excluded.last_status,
               last_server_count=excluded.last_server_count,
               last_detail=excluded.last_detail,
               consecutive_failures=CASE WHEN ?7 THEN 0 ELSE helper_topgg_sync_state.consecutive_failures+1 END",
            params![
                now_ms,
                succeeded.then_some(now_ms),
                status.map(i64::from),
                count,
                detail.as_storage(),
                i64::from(!succeeded),
                succeeded,
            ],
        )?;
        Ok(())
    }

    pub fn topgg_sync_status(&self, now_ms: i64) -> Result<Option<TopggSyncStatus>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.query_row(
            "SELECT last_attempt_at,last_success_at,last_status,last_server_count,last_detail,consecutive_failures
             FROM helper_topgg_sync_state WHERE singleton=1",
            [],
            |row| {
                let last_success_at: Option<i64> = row.get(1)?;
                let raw_status: Option<i64> = row.get(2)?;
                Ok(TopggSyncStatus {
                    last_attempt_at: row.get(0)?,
                    last_success_at,
                    last_status: raw_status.and_then(|value| u16::try_from(value).ok()),
                    last_server_count: row.get(3)?,
                    last_detail: TopggSyncDetail::from_storage(&row.get::<_, String>(4)?),
                    consecutive_failures: row.get(5)?,
                    stale: last_success_at.is_none_or(|at| now_ms.saturating_sub(at) > TOPGG_STALE_AFTER_MS),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_delivery_resets_failures_and_invalid_token_is_observable() {
        let store = Store::open(":memory:").expect("store");
        assert_eq!(store.topgg_sync_status(1).expect("read"), None);
        store
            .record_topgg_sync_attempt(1_000, Some(204), Some(9), true, TopggSyncDetail::Delivered)
            .expect("success");
        let healthy = store.topgg_sync_status(1_001).expect("read").expect("row");
        assert_eq!(healthy.last_detail, TopggSyncDetail::Delivered);
        assert_eq!(healthy.consecutive_failures, 0);
        assert!(!healthy.stale);

        store
            .record_topgg_sync_attempt(
                2_000,
                Some(401),
                None,
                false,
                TopggSyncDetail::V1AuthenticationFailed,
            )
            .expect("failure");
        let failed = store
            .topgg_sync_status(1_000 + TOPGG_STALE_AFTER_MS + 1)
            .expect("read")
            .expect("row");
        assert_eq!(failed.last_status, Some(401));
        assert_eq!(failed.last_detail, TopggSyncDetail::V1AuthenticationFailed);
        assert_eq!(failed.consecutive_failures, 1);
        assert!(failed.stale);
    }
}
