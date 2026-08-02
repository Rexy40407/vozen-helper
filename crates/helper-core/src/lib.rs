//! Pure configuration and policy primitives. No Discord or HTTP side effects.

use anyhow::{Context, Result};
use helper_contracts::{FeatureMaturity, Plan};
use serde::Deserialize;
use std::{env, net::IpAddr, path::PathBuf, str::FromStr};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub discord_application_id: u64,
    pub database_url: PathBuf,
    pub bind_addr: String,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub oauth_redirect_uri: String,
    pub oauth_success_redirect: String,
    pub allow_legacy_session: bool,
    pub session_secret: String,
    pub entitlement_url: Option<String>,
    pub entitlement_secret: Option<String>,
    pub environment: String,
    pub api_only: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();
        let required = |name: &str| -> Result<String> {
            env::var(name).with_context(|| format!("missing required environment variable {name}"))
        };
        let discord_application_id = required("DISCORD_APPLICATION_ID")?
            .parse::<u64>()
            .context("DISCORD_APPLICATION_ID must be an integer")?;
        let bind_addr = env::var("HELPER_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3101".into());
        bind_addr
            .parse::<std::net::SocketAddr>()
            .context("HELPER_BIND_ADDR must be host:port")?;
        Ok(Self {
            discord_token: required("DISCORD_TOKEN")?,
            discord_application_id,
            database_url: PathBuf::from(
                env::var("HELPER_DATABASE_URL")
                    .unwrap_or_else(|_| "data/vozen-helper.sqlite".into()),
            ),
            bind_addr,
            oauth_client_id: required("DISCORD_OAUTH_CLIENT_ID")?,
            oauth_client_secret: required("DISCORD_OAUTH_CLIENT_SECRET")?,
            oauth_redirect_uri: required("DISCORD_OAUTH_REDIRECT_URI")?,
            oauth_success_redirect: env::var("HELPER_OAUTH_SUCCESS_REDIRECT")
                .unwrap_or_else(|_| "https://rexy40407.github.io/vozen-helper-bot/".into()),
            allow_legacy_session: env::var("HELPER_ALLOW_LEGACY_SESSION")
                .is_ok_and(|value| value.eq_ignore_ascii_case("true")),
            session_secret: required("HELPER_SESSION_SECRET")?,
            entitlement_url: env::var("VOZEN_ENTITLEMENT_URL").ok(),
            entitlement_secret: env::var("VOZEN_ENTITLEMENT_SECRET").ok(),
            environment: env::var("NODE_ENV").unwrap_or_else(|_| "production".into()),
            api_only: env::var("HELPER_API_ONLY")
                .is_ok_and(|value| value.eq_ignore_ascii_case("true")),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.discord_token.len() < 20 {
            anyhow::bail!("DISCORD_TOKEN is unexpectedly short");
        }
        if self.session_secret.len() < 32 {
            anyhow::bail!("HELPER_SESSION_SECRET must be at least 32 bytes");
        }
        if self.oauth_redirect_uri.starts_with("http://") && self.environment == "production" {
            anyhow::bail!("OAuth redirect URI must use HTTPS in production");
        }
        if self.oauth_success_redirect.starts_with("http://") && self.environment == "production" {
            anyhow::bail!("OAuth success redirect must use HTTPS in production");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Core,
    Studio,
    Security,
    Support,
    Events,
    Community,
    Automate,
    Insights,
}

/// Canonical allow-list shared by validation and migration code. The API
/// supplies copy and category metadata, but no request may publish a key that
/// is not present in this list.
pub const FEATURE_KEYS: &[&str] = &[
    "protection.antispam",
    "protection.antiscam",
    "protection.anti_raid",
    "protection.join_gate",
    "community.levels",
    "community.leaderboard",
    "community.starboard",
    "community.suggestions",
    "community.giveaways",
    "support.tickets",
    "support.welcome",
    "support.welcome_channel",
    "management.nickname",
    "management.workflows",
    "management.polls",
    "insights.stats",
    "studio.rank_card",
    "management.moderation",
    "management.custom_commands",
    "management.audit",
    "management.privacy",
    "management.templates",
    "community.role_panels",
    "community.events",
    "community.achievements",
    "management.invite_tracker",
    "utility.help",
    "utility.reminders",
    "utility.emojis",
    "utility.embeds",
    "utility.search",
    "utility.temp_channels",
    "social.twitch",
    "social.youtube",
    "social.instagram",
    "social.reddit",
    "social.x",
    "social.tiktok",
    "social.rss",
    "social.podcasts",
    "social.kick",
    "social.bluesky",
    "community.birthdays",
    "community.economy",
    "growth.monetization",
    "web3.nft_stats",
    "web3.nft_queries",
    "web3.nft_sales",
    "web3.crypto_stats",
    "web3.crypto_queries",
    "web3.gas_tracker",
    "web3.gating",
];

pub fn is_known_feature(key: &str) -> bool {
    FEATURE_KEYS.contains(&key)
}

/// Canonical lifecycle policy for the feature catalogue.  Labels and copy are
/// intentionally kept in the API response for now, but the runtime state is
/// decided here so the panel cannot mark a stored JSON blob as operational.
pub fn feature_maturity(key: &str) -> FeatureMaturity {
    match key {
        // These adapters are wired to the current Rust runtime.
        "protection.anti_raid"
        | "protection.join_gate"
        | "protection.antispam"
        | "community.levels"
        | "community.starboard"
        | "community.suggestions"
        | "community.giveaways"
        | "community.role_panels"
        | "community.events"
        | "support.tickets"
        | "support.welcome"
        | "management.polls"
        | "studio.rank_card" => FeatureMaturity::Operational,
        "social.youtube" | "social.rss" | "social.twitch" => FeatureMaturity::Beta,
        // Providers without an approved adapter or credentials must never be
        // presented as configurable, even if a legacy setting exists.
        "social.instagram"
        | "social.reddit"
        | "social.x"
        | "social.tiktok"
        | "social.podcasts"
        | "social.kick"
        | "social.bluesky"
        | "utility.search"
        | "growth.monetization"
        | "web3.nft_stats"
        | "web3.nft_queries"
        | "web3.nft_sales"
        | "web3.crypto_stats"
        | "web3.crypto_queries"
        | "web3.gas_tracker"
        | "web3.gating" => FeatureMaturity::Blocked,
        _ => FeatureMaturity::Planned,
    }
}

pub fn feature_is_configurable(key: &str) -> bool {
    matches!(
        feature_maturity(key),
        FeatureMaturity::Operational | FeatureMaturity::Beta
    )
}

pub const FEATURE_SCHEMA_VERSION: u32 = 1;

pub fn quota_limit(plan: &Plan, key: &str) -> u64 {
    let free = match key {
        "panels" => 1,
        "forms" => 1,
        "role_panels" => 3,
        "workflows" => 3,
        "workflow_runs" => 500,
        "feeds" => 2,
        "templates" => 5,
        "analytics_days" => 30,
        "audit_days" => 30,
        "transcript_days" => 30,
        "personal_drafts" | "personal_views" | "personal_templates" => 5,
        _ => 0,
    };
    let paid = match key {
        "panels" => 10,
        "forms" => 25,
        "role_panels" => 25,
        "workflows" => 25,
        "workflow_runs" => 25_000,
        "feeds" => 25,
        "templates" => 100,
        "analytics_days" => 365,
        "audit_days" => 365,
        "transcript_days" => 365,
        "personal_drafts" | "personal_views" | "personal_templates" => 100,
        _ => 0,
    };
    match plan {
        Plan::Free => free,
        // Plus is user-scoped: it can unlock personal drafts, views and
        // templates, but must not increase a guild's operational quotas.
        Plan::Plus => match key {
            "personal_drafts" | "personal_views" | "personal_templates" => paid,
            _ => free,
        },
        Plan::Premium { .. } => paid,
    }
}

pub fn parse_ip(value: &str) -> Result<IpAddr> {
    IpAddr::from_str(value).context("invalid IP address")
}

#[cfg(test)]
mod tests {
    use super::*;
    use helper_contracts::Plan;

    #[test]
    fn quota_matrix_keeps_free_small_and_paid_bounded() {
        assert_eq!(quota_limit(&Plan::Free, "workflow_runs"), 500);
        assert_eq!(quota_limit(&Plan::Plus, "workflow_runs"), 500);
        assert_eq!(quota_limit(&Plan::Plus, "personal_drafts"), 100);
        assert_eq!(quota_limit(&Plan::Premium { guild_limit: 8 }, "panels"), 10);
    }

    #[test]
    fn unknown_quota_is_closed() {
        assert_eq!(quota_limit(&Plan::Free, "unknown"), 0);
    }

    #[test]
    fn feature_registry_has_52_unique_keys_and_closed_unknowns() {
        let mut keys = FEATURE_KEYS.to_vec();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(FEATURE_KEYS.len(), 52);
        assert_eq!(keys.len(), 52);
        assert!(is_known_feature("community.leaderboard"));
        assert!(!is_known_feature("community.not_a_real_feature"));
    }
}
