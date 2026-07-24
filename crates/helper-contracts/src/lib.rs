//! Stable contracts shared by the bot, API and panel.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const API_VERSION: &str = "v1";
pub const PRODUCT_ID: &str = "vozen-helper";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuildRef {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Plan {
    Free,
    Plus,
    Premium { guild_limit: u16 },
}

impl Plan {
    pub fn guild_limit(&self) -> u16 {
        match self {
            Self::Free => 1,
            Self::Plus => 1,
            Self::Premium { guild_limit } => *guild_limit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntitlementSnapshot {
    pub product: String,
    pub subject_id: String,
    pub plan: Plan,
    pub active: bool,
    pub version: u64,
    pub expires_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

impl EntitlementSnapshot {
    pub fn free(subject_id: impl Into<String>) -> Self {
        Self {
            product: PRODUCT_ID.to_string(),
            subject_id: subject_id.into(),
            plan: Plan::Free,
            active: true,
            version: 0,
            expires_at: None,
            fetched_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionClaims {
    pub session_id: Uuid,
    pub user_id: String,
    pub guild_id: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageQuota {
    pub key: String,
    pub used: u64,
    pub limit: u64,
    pub resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
}
