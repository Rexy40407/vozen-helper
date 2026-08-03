//! Pure configuration and policy primitives. No Discord or HTTP side effects.

use anyhow::{Context, Result};
use helper_contracts::{
    AntiSpamDecision, AntiSpamObservation, AntiSpamPolicy, FeatureAdapterDescriptor,
    FeatureMaturity, Plan, ValidationIssue,
};
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
                .unwrap_or_else(|_| "https://rexy40407.github.io/Vozen_Helper/".into()),
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

/// Parse the JSON feature representation into the bounded anti-spam policy
/// shared by the API and gateway.
pub fn anti_spam_policy_from_json(value: &serde_json::Value) -> AntiSpamPolicy {
    let mut policy = AntiSpamPolicy::default();
    let Some(object) = value.as_object() else {
        return policy;
    };
    let number = |name: &str| object.get(name).and_then(serde_json::Value::as_u64);
    if let Some(value) = number("floodCount") {
        policy.flood_count = value.clamp(3, 30) as u32;
    }
    if let Some(value) = number("windowSeconds") {
        policy.window_seconds = value.clamp(3, 60);
    }
    if let Some(value) = number("duplicateLimit") {
        policy.duplicate_limit = value.clamp(2, 12) as u32;
    }
    if let Some(value) = number("mentionLimit") {
        policy.mention_limit = value.clamp(1, 30) as u32;
    }
    if let Some(value) = number("timeoutSeconds") {
        policy.timeout_seconds = value.min(86_400);
    }
    if let Some(value) = object
        .get("ignoredChannels")
        .and_then(serde_json::Value::as_array)
    {
        policy.ignored_channels = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(value) = object
        .get("ignoredRoles")
        .and_then(serde_json::Value::as_array)
    {
        policy.ignored_roles = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
        policy.alert_only = value;
    }
    policy
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScamPolicy {
    pub block_invites: bool,
    pub blocked_domains: Vec<String>,
    pub blocked_keywords: Vec<String>,
    pub ignored_channels: Vec<String>,
    pub alert_only: bool,
    pub timeout_seconds: u64,
}

impl Default for ScamPolicy {
    fn default() -> Self {
        Self {
            block_invites: true,
            blocked_domains: Vec::new(),
            blocked_keywords: vec![
                "free nitro".into(),
                "steam gift".into(),
                "claim your prize".into(),
                "verify your wallet".into(),
            ],
            ignored_channels: Vec::new(),
            alert_only: false,
            timeout_seconds: 300,
        }
    }
}

pub fn scam_policy_from_json(value: &serde_json::Value) -> ScamPolicy {
    let mut policy = ScamPolicy::default();
    let Some(object) = value.as_object() else {
        return policy;
    };
    if let Some(value) = object
        .get("blockInvites")
        .and_then(serde_json::Value::as_bool)
    {
        policy.block_invites = value;
    }
    if let Some(value) = object
        .get("blockedDomains")
        .and_then(serde_json::Value::as_array)
    {
        policy.blocked_domains = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|domain| domain.trim().to_ascii_lowercase())
            .filter(|domain| !domain.is_empty())
            .take(100)
            .collect();
    }
    if let Some(value) = object
        .get("blockedKeywords")
        .and_then(serde_json::Value::as_array)
    {
        policy.blocked_keywords = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|keyword| keyword.trim().to_ascii_lowercase())
            .filter(|keyword| !keyword.is_empty())
            .take(100)
            .collect();
    }
    if let Some(value) = object
        .get("ignoredChannels")
        .and_then(serde_json::Value::as_array)
    {
        policy.ignored_channels = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .take(100)
            .collect();
    }
    if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
        policy.alert_only = value;
    }
    if let Some(value) = object
        .get("timeoutSeconds")
        .and_then(serde_json::Value::as_u64)
    {
        policy.timeout_seconds = value.clamp(0, 86_400);
    }
    policy
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScamDecision {
    pub ignored: bool,
    pub matched: Vec<String>,
    pub should_act: bool,
    pub timeout_seconds: u64,
    pub reason: String,
}

pub fn evaluate_scam(policy: &ScamPolicy, channel_id: &str, content: &str) -> ScamDecision {
    if policy.ignored_channels.iter().any(|id| id == channel_id) {
        return ScamDecision {
            ignored: true,
            matched: Vec::new(),
            should_act: false,
            timeout_seconds: 0,
            reason: "ignored_channel".into(),
        };
    }
    let lower = content.to_ascii_lowercase();
    let mut matched = Vec::new();
    if policy.block_invites
        && (lower.contains("discord.gg/") || lower.contains("discord.com/invite/"))
    {
        matched.push("discord_invite".into());
    }
    for domain in &policy.blocked_domains {
        if lower.contains(domain) {
            matched.push(format!("domain:{domain}"));
        }
    }
    for keyword in &policy.blocked_keywords {
        if lower.contains(keyword) {
            matched.push(format!("keyword:{keyword}"));
        }
    }
    let should_act = !matched.is_empty() && !policy.alert_only;
    ScamDecision {
        ignored: false,
        matched,
        should_act,
        timeout_seconds: if should_act {
            policy.timeout_seconds
        } else {
            0
        },
        reason: if should_act {
            "blocked_scam_pattern".into()
        } else {
            "scam_pattern_monitoring".into()
        },
    }
}

/// Every configurable feature will eventually implement this contract. The
/// first adapter is intentionally small: it proves that the API and gateway
/// can consume one canonical schema/defaults/validator without pulling UI
/// concerns into the Discord crate.
pub trait FeatureAdapter: Sync {
    fn descriptor(&self) -> FeatureAdapterDescriptor;
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue>;
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)>;
}

/// Adapter for modules whose Discord behaviour is already command/interaction
/// driven and therefore has no additional server-side knobs yet.  An empty
/// schema is intentional: the panel can publish the feature toggle without
/// inventing controls that the runtime would ignore.
#[derive(Debug, Clone, Copy)]
pub struct ToggleOnlyAdapter {
    pub key: &'static str,
    pub source: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub dependencies: &'static [&'static str],
}

/// Settings for temporary voice rooms.  The command already creates and
/// cleans up rooms in the Discord runtime; exposing these bounded controls
/// makes the dashboard publish the same values that command handler reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct TempChannelsAdapter;

impl TempChannelsAdapter {
    pub const KEY: &'static str = "utility.temp_channels";
    pub const SOURCE: &'static str = "temp_channels_adapter_v2";
}

impl FeatureAdapter for TempChannelsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Temporary voice rooms",
                    "description": "Create bounded rooms with a predictable name and optional category.",
                    "fields": [
                        {"key":"categoryId","label":"Category (optional)","kind":"category","help":"New rooms are created inside this category when selected."},
                        {"key":"nameTemplate","label":"Room name template","kind":"text","min":1,"max":80,"help":"Use {user} for the member display name."},
                        {"key":"maxActive","label":"Maximum active rooms","kind":"number","min":1,"max":50}
                    ]
                }]
            }),
            defaults: serde_json::json!({"categoryId":"", "nameTemplate":"{user}'s room", "maxActive": 10}),
            dependencies: vec!["manage_channels".into(), "voice_states".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("categoryId")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "categoryId".into(),
                code: "invalid_category_id".into(),
                message: "Choose a valid Discord category.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("nameTemplate") {
            let valid = value.as_str().is_some_and(|text| {
                let count = text.chars().count();
                (1..=80).contains(&count) && !text.chars().any(char::is_control)
            });
            if !valid {
                issues.push(ValidationIssue {
                    path: "nameTemplate".into(),
                    code: "invalid_template".into(),
                    message: "The room name must be 1-80 characters without control characters."
                        .into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("maxActive") {
            if let Some(value) = value.as_i64() {
                if !(1..=50).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "maxActive".into(),
                        code: "out_of_range".into(),
                        message: "Maximum active rooms must be between 1 and 50.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "maxActive".into(),
                    code: "integer_required".into(),
                    message: "Maximum active rooms must be an integer.".into(),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        if let Some(value) = object.get("categoryId").and_then(serde_json::Value::as_str) {
            pairs.push(("utility.temp_channels.category_id".into(), value.into()));
        }
        if let Some(value) = object
            .get("nameTemplate")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("utility.temp_channels.name_template".into(), value.into()));
        }
        if let Some(value) = object.get("maxActive").and_then(serde_json::Value::as_i64) {
            pairs.push(("utility.temp_channels.max_active".into(), value.to_string()));
        }
        pairs
    }
}

/// Adapter for the tag-backed custom command module.  Tags are the bounded,
/// user-authored responses exposed by `/tag` and `/tag-set`; the adapter keeps
/// the limits in the same source of truth as the runtime instead of leaving
/// them as hard-coded command constants.
#[derive(Debug, Clone, Copy, Default)]
pub struct CustomCommandsAdapter;

impl CustomCommandsAdapter {
    pub const KEY: &'static str = "management.custom_commands";
    pub const SOURCE: &'static str = "custom_commands_adapter_v1";
}

impl FeatureAdapter for CustomCommandsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Custom command limits",
                    "description": "Saved responses are deliberately bounded and never execute code.",
                    "fields": [
                        {"key":"maxTags","label":"Maximum saved commands","kind":"number","min":1,"max":100,"help":"The maximum number of saved responses this server can keep."},
                        {"key":"maxResponseLength","label":"Maximum response length","kind":"number","min":1,"max":2000,"help":"Responses longer than this are rejected before they can be sent."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"maxTags": 100, "maxResponseLength": 1000}),
            dependencies: vec!["message_content".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max, label) in [
            ("maxTags", 1_i64, 100_i64, "Maximum saved commands"),
            (
                "maxResponseLength",
                1_i64,
                2_000_i64,
                "Maximum response length",
            ),
        ] {
            if let Some(value) = object.get(field) {
                if let Some(value) = value.as_i64() {
                    if !(min..=max).contains(&value) {
                        issues.push(ValidationIssue {
                            path: field.into(),
                            code: "out_of_range".into(),
                            message: format!("{label} must be between {min} and {max}."),
                            severity: "error".into(),
                        });
                    }
                } else {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "integer_required".into(),
                        message: format!("{label} must be an integer."),
                        severity: "error".into(),
                    });
                }
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object.get("maxTags").and_then(serde_json::Value::as_i64) {
            projection.push((
                "management.custom_commands.max_tags".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("maxResponseLength")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "management.custom_commands.max_response_length".into(),
                value.to_string(),
            ));
        }
        projection
    }
}

/// The audit feature controls the existing destructive-action guard and its
/// shadow mode.  Keeping these projections here makes the panel toggle and
/// the `/anti-nuke` command operate on the same persisted settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuditAdapter;

impl AuditAdapter {
    pub const KEY: &'static str = "management.audit";
    pub const SOURCE: &'static str = "audit_adapter_v1";
}

impl FeatureAdapter for AuditAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Destructive action protection",
                    "description": "Record audit events and contain repeated destructive changes.",
                    "fields": [
                        {"key":"threshold","label":"Actions before containment","kind":"number","min":2,"max":25},
                        {"key":"windowSeconds","label":"Detection window (seconds)","kind":"number","min":3,"max":60},
                        {"key":"shadowMode","label":"Shadow mode","kind":"toggle","help":"Record and alert without automatic containment."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"threshold": 3, "windowSeconds": 10, "shadowMode": false}),
            dependencies: vec!["guild_moderation".into(), "view_audit_log".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max, label) in [
            ("threshold", 2_i64, 25_i64, "Threshold"),
            ("windowSeconds", 3_i64, 60_i64, "Window"),
        ] {
            if let Some(value) = object.get(field) {
                if let Some(value) = value.as_i64() {
                    if !(min..=max).contains(&value) {
                        issues.push(ValidationIssue {
                            path: field.into(),
                            code: "out_of_range".into(),
                            message: format!("{label} must be between {min} and {max}."),
                            severity: "error".into(),
                        });
                    }
                } else {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "integer_required".into(),
                        message: format!("{label} must be an integer."),
                        severity: "error".into(),
                    });
                }
            }
        }
        if object
            .get("shadowMode")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "shadowMode".into(),
                code: "boolean_required".into(),
                message: "Shadow mode must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object.get("threshold").and_then(serde_json::Value::as_i64) {
            projection.push(("security.anti_nuke.actions".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("windowSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "security.anti_nuke.window_seconds".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("shadowMode")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("security.shadow_mode".into(), value.to_string()));
        }
        projection
    }
}

/// Import/export already exists in the durable store.  This adapter exposes
/// the capability with no invented JSON controls; the API gates those routes
/// by this feature toggle and keeps resource mapping/validation server-side.
#[derive(Debug, Clone, Copy, Default)]
pub struct TemplatesAdapter;

impl TemplatesAdapter {
    pub const KEY: &'static str = "management.templates";
    pub const SOURCE: &'static str = "templates_adapter_v1";
}

impl FeatureAdapter for TemplatesAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version": FEATURE_SCHEMA_VERSION, "source": Self::SOURCE, "sections": [{"title":"Portable templates", "description":"Save and transfer validated server configuration without copying secrets.", "fields": []}]}),
            defaults: serde_json::json!({}),
            dependencies: vec!["manager_session".into()],
        }
    }
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        if config.is_object() {
            Vec::new()
        } else {
            vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }]
        }
    }
    fn runtime_projection(&self, _config: &serde_json::Value) -> Vec<(String, String)> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BirthdaysAdapter;

impl BirthdaysAdapter {
    pub const KEY: &'static str = "community.birthdays";
    pub const SOURCE: &'static str = "birthdays_adapter_v1";
}

impl FeatureAdapter for BirthdaysAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Birthday announcements",
                    "description": "Store only day and month and announce birthdays once per year.",
                    "fields": [
                        {"key":"channel","label":"Announcement channel","kind":"channel"},
                        {"key":"message","label":"Announcement message","kind":"text","min":1,"max":1000,"help":"Use {user} for the member mention."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"channel":"", "message":"Happy birthday, {user}! 🎉"}),
            dependencies: vec![
                "guild_members".into(),
                "send_messages".into(),
                "scheduler".into(),
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if object
            .get("channel")
            .is_some_and(|value| value.as_str().is_none())
        {
            issues.push(ValidationIssue {
                path: "channel".into(),
                code: "string_required".into(),
                message: "Choose an announcement channel.".into(),
                severity: "error".into(),
            });
        }
        if let Some(message) = object.get("message") {
            if let Some(message) = message.as_str() {
                if !(1..=1000).contains(&message.chars().count()) {
                    issues.push(ValidationIssue {
                        path: "message".into(),
                        code: "out_of_range".into(),
                        message: "Birthday message must be between 1 and 1000 characters.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "message".into(),
                    code: "string_required".into(),
                    message: "Birthday message must be text.".into(),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object.get("channel").and_then(serde_json::Value::as_str) {
            projection.push(("community.birthdays.channel_id".into(), value.into()));
        }
        if let Some(value) = object.get("message").and_then(serde_json::Value::as_str) {
            projection.push(("community.birthdays.message".into(), value.into()));
        }
        projection
    }
}

impl FeatureAdapter for ToggleOnlyAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: self.source.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": self.source,
                "sections": [{
                    "title": self.title,
                    "description": self.description,
                    "fields": []
                }]
            }),
            defaults: serde_json::json!({}),
            dependencies: self
                .dependencies
                .iter()
                .map(|value| (*value).into())
                .collect(),
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        if config.is_object() {
            Vec::new()
        } else {
            vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }]
        }
    }

    fn runtime_projection(&self, _config: &serde_json::Value) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Shared schema for feed-backed alert surfaces.  RSS and podcast feeds are
/// validated and published through the same store/provider path, but each has
/// its own catalog key and user-facing copy.
#[derive(Debug, Clone, Copy)]
pub struct FeedAdapter {
    pub key: &'static str,
    pub source: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub dependencies: &'static [&'static str],
}

impl FeatureAdapter for FeedAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: self.source.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": self.source,
                "sections": [{
                    "title": self.title,
                    "description": self.description,
                    "fields": [
                        {"key":"feedUrl","label":"Public feed URL","kind":"text"},
                        {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                        {"key":"intervalSeconds","label":"Polling interval (seconds)","kind":"number","min":300,"max":86400},
                        {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                        {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "feedUrl": "",
                "targetChannelId": "",
                "intervalSeconds": 900,
                "messageTemplate": "New update from {feed}: **{title}**\\n{url}",
                "mention": ""
            }),
            dependencies: self
                .dependencies
                .iter()
                .map(|value| (*value).into())
                .collect(),
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Feed configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["feedUrl", "targetChannelId"] {
            if object
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value.chars().count() > 2_000)
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "too_long".into(),
                    message: "Feed fields must be at most 2000 characters.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("intervalSeconds")
            && value
                .as_i64()
                .is_some_and(|value| !(300..=86_400).contains(&value))
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "out_of_range".into(),
                message: "Polling interval must be between 300 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, _config: &serde_json::Value) -> Vec<(String, String)> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TicketsAdapter;

impl TicketsAdapter {
    pub const KEY: &'static str = "support.tickets";
    pub const SOURCE: &'static str = "tickets_adapter_v1";
}

impl FeatureAdapter for TicketsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Support workflow",
                    "description": "Choose the real role, transcript channel and SLA used by ticket interactions.",
                    "fields": [
                        {"key":"staffRole","label":"Support team role","kind":"role","help":"This role can claim, close and reopen tickets."},
                        {"key":"transcriptChannel","label":"Transcript channel","kind":"channel","help":"Closed ticket transcripts are sent here."},
                        {"key":"closeAfterHours","label":"SLA reminder (hours)","kind":"number","min":1,"max":168,"help":"The Helper records an overdue ticket job after this period."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"staffRole":"","transcriptChannel":"","closeAfterHours":1}),
            dependencies: vec![
                "manage_channels".into(),
                "send_messages".into(),
                "manage_roles".into(),
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["staffRole", "transcriptChannel"] {
            if let Some(value) = object.get(field)
                && !(value
                    .as_str()
                    .is_some_and(|raw| raw.is_empty() || raw.parse::<u64>().is_ok()))
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "invalid_discord_id".into(),
                    message: "Choose a real Discord role or channel.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("closeAfterHours")
            && !value
                .as_i64()
                .is_some_and(|hours| (1..=168).contains(&hours))
        {
            issues.push(ValidationIssue {
                path: "closeAfterHours".into(),
                code: "out_of_range".into(),
                message: "The SLA must be between 1 and 168 hours.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        if let Some(value) = object.get("staffRole").and_then(serde_json::Value::as_str) {
            pairs.push(("support.ticket.staff_role_id".into(), value.into()));
        }
        if let Some(value) = object
            .get("transcriptChannel")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("support.ticket.transcript_channel_id".into(), value.into()));
        }
        if let Some(value) = object
            .get("closeAfterHours")
            .and_then(serde_json::Value::as_i64)
        {
            pairs.push((
                "support.ticket.sla_ms".into(),
                (value * 3_600_000).to_string(),
            ));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WelcomeAdapter;

impl WelcomeAdapter {
    pub const KEY: &'static str = "support.welcome";
    pub const SOURCE: &'static str = "welcome_adapter_v1";
}

impl FeatureAdapter for WelcomeAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Welcome message",
                    "description": "Configure the channel, message, optional DM and real auto-role used on member join.",
                    "fields": [
                        {"key":"channel","label":"Welcome channel","kind":"channel"},
                        {"key":"message","label":"Public message","kind":"textarea","maxLength":2000,"help":"Variables: {member} and {server}."},
                        {"key":"delaySeconds","label":"Delay before sending (seconds)","kind":"number","min":0,"max":300,"advanced":true},
                        {"key":"sendDm","label":"Send a direct message","kind":"toggle"},
                        {"key":"dmMessage","label":"Direct message","kind":"textarea","maxLength":2000,"advanced":true},
                        {"key":"autoRole","label":"Automatic role","kind":"role","advanced":true},
                        {"key":"farewellChannel","label":"Farewell channel","kind":"channel","advanced":true},
                        {"key":"farewellMessage","label":"Farewell message","kind":"textarea","maxLength":2000,"advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({"channel":"","message":"Welcome {member} to {server}!","delaySeconds":0,"sendDm":false,"dmMessage":"Hello {member}, welcome to {server}!","autoRole":"","farewellChannel":"","farewellMessage":"Goodbye {member}. We hope to see you again!"}),
            dependencies: vec!["guild_members_intent".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["channel", "autoRole", "farewellChannel"] {
            if object.get(field).is_some_and(|value| {
                !value
                    .as_str()
                    .is_some_and(|raw| raw.is_empty() || raw.parse::<u64>().is_ok())
            }) {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "invalid_discord_id".into(),
                    message: "Choose a real Discord resource.".into(),
                    severity: "error".into(),
                });
            }
        }
        for field in ["message", "dmMessage", "farewellMessage"] {
            if object.get(field).is_some_and(|value| {
                !value.as_str().is_some_and(|raw| {
                    raw.chars().count() <= 2_000 && !raw.chars().any(char::is_control)
                })
            }) {
                issues.push(ValidationIssue { path: field.into(), code: "invalid_message".into(), message: "Messages must be at most 2000 characters and contain no control characters.".into(), severity: "error".into() });
            }
        }
        if object
            .get("sendDm")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "sendDm".into(),
                code: "boolean_required".into(),
                message: "Send DM must be true or false.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("delaySeconds")
            .is_some_and(|value| !value.as_i64().is_some_and(|raw| (0..=300).contains(&raw)))
        {
            issues.push(ValidationIssue {
                path: "delaySeconds".into(),
                code: "out_of_range".into(),
                message: "The welcome delay must be between 0 and 300 seconds.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (field, key) in [
            ("channel", "support.welcome.channel_id"),
            ("message", "support.welcome.message"),
            ("dmMessage", "support.welcome.dm_message"),
            ("autoRole", "support.welcome.auto_role"),
            ("farewellChannel", "support.welcome.farewell_channel_id"),
            ("farewellMessage", "support.welcome.farewell_message"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                pairs.push((key.into(), value.into()));
            }
        }
        if let Some(value) = object.get("sendDm").and_then(serde_json::Value::as_bool) {
            pairs.push(("support.welcome.send_dm".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("delaySeconds")
            .and_then(serde_json::Value::as_i64)
        {
            pairs.push(("support.welcome.delay_seconds".into(), value.to_string()));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AntiSpamAdapter;

#[derive(Debug, Clone, Copy, Default)]
pub struct NicknameAdapter;

impl NicknameAdapter {
    pub const KEY: &'static str = "management.nickname";
    pub const SOURCE: &'static str = "nickname_adapter_v1";

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "version": FEATURE_SCHEMA_VERSION,
            "source": Self::SOURCE,
            "sections": [{
                "title": "Helper identity",
                "description": "Choose the display name the Helper uses in this server.",
                "fields": [{
                    "key": "nickname",
                    "label": "Server nickname",
                    "kind": "text",
                    "min": 0,
                    "max": 32,
                    "help": "Discord allows up to 32 characters. Leave it empty when disabling to restore the default name."
                }]
            }]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({"nickname": ""})
    }
}

impl FeatureAdapter for NicknameAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: Self::schema(),
            defaults: Self::defaults(),
            dependencies: vec!["change_nickname".into(), "manage_nicknames".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let Some(value) = object.get("nickname").and_then(serde_json::Value::as_str) else {
            return vec![ValidationIssue {
                path: "nickname".into(),
                code: "required".into(),
                message: "Enter the Helper nickname.".into(),
                severity: "error".into(),
            }];
        };
        if value.chars().count() > 32 || value.chars().any(char::is_control) {
            return vec![ValidationIssue {
                path: "nickname".into(),
                code: "invalid_nickname".into(),
                message: "The nickname must be at most 32 characters and cannot contain control characters.".into(),
                severity: "error".into(),
            }];
        }
        Vec::new()
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        config
            .get("nickname")
            .and_then(serde_json::Value::as_str)
            .map(|value| vec![("identity.nickname".into(), value.to_owned())])
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReminderAdapter;

impl ReminderAdapter {
    pub const KEY: &'static str = "utility.reminders";
    pub const SOURCE: &'static str = "reminders_adapter_v1";

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "version": FEATURE_SCHEMA_VERSION,
            "source": Self::SOURCE,
            "sections": [{
                "title": "Reminder limits",
                "description": "Keep reminders useful and bounded for every member.",
                "fields": [
                    {"key":"maxDelayHours","label":"Maximum delay (hours)","kind":"number","min":1,"max":8760,"help":"The longest time a member can schedule a reminder."},
                    {"key":"maxTextLength","label":"Maximum message length","kind":"number","min":50,"max":500,"help":"Longer text is rejected before a job is created."},
                    {"key":"notifyUser","label":"Mention the member when it fires","kind":"toggle","help":"Turn off to post a quiet reminder without a mention."}
                ]
            }]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({"maxDelayHours": 168, "maxTextLength": 500, "notifyUser": true})
    }
}

impl FeatureAdapter for ReminderAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: Self::schema(),
            defaults: Self::defaults(),
            dependencies: vec!["scheduler".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max) in [
            ("maxDelayHours", 1_i64, 8_760_i64),
            ("maxTextLength", 50, 500),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64)
                && !(min..=max).contains(&value)
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: format!("The value must be between {min} and {max}."),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("maxDelayHours")
            .is_some_and(|value| !value.is_i64())
        {
            issues.push(ValidationIssue {
                path: "maxDelayHours".into(),
                code: "integer_required".into(),
                message: "Maximum delay must be an integer.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("maxTextLength")
            .is_some_and(|value| !value.is_i64())
        {
            issues.push(ValidationIssue {
                path: "maxTextLength".into(),
                code: "integer_required".into(),
                message: "Maximum message length must be an integer.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("notifyUser")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "notifyUser".into(),
                code: "boolean_required".into(),
                message: "Mention preference must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object
            .get("maxDelayHours")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "utility.reminders.max_delay_hours".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("maxTextLength")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "utility.reminders.max_text_length".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("notifyUser")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("utility.reminders.notify_user".into(), value.to_string()));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LeaderboardAdapter;

impl LeaderboardAdapter {
    pub const KEY: &'static str = "community.leaderboard";
    pub const SOURCE: &'static str = "leaderboard_adapter_v1";

    fn descriptor_schema() -> serde_json::Value {
        serde_json::json!({
            "version": FEATURE_SCHEMA_VERSION,
            "source": Self::SOURCE,
            "sections": [{
                "title": "Leaderboard privacy",
                "description": "Control how many members appear in the XP leaderboard.",
                "fields": [
                    {"key":"maxEntries","label":"Members shown","kind":"number","min":1,"max":100,"help":"Keep the public result concise and predictable."},
                    {"key":"public","label":"Show the leaderboard publicly","kind":"toggle","help":"When disabled, only the requesting member sees the result."}
                ]
            }]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({"maxEntries": 10, "public": true})
    }
}

impl FeatureAdapter for LeaderboardAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: Self::descriptor_schema(),
            defaults: Self::defaults(),
            dependencies: vec!["levels".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("maxEntries") {
            if let Some(value) = value.as_i64() {
                if !(1..=100).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "maxEntries".into(),
                        code: "out_of_range".into(),
                        message: "Members shown must be between 1 and 100.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "maxEntries".into(),
                    code: "integer_required".into(),
                    message: "Members shown must be an integer.".into(),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("public")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "public".into(),
                code: "boolean_required".into(),
                message: "Public visibility must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object.get("maxEntries").and_then(serde_json::Value::as_i64) {
            projection.push((
                "community.leaderboard.max_entries".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object.get("public").and_then(serde_json::Value::as_bool) {
            projection.push(("community.leaderboard.public".into(), value.to_string()));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowAdapter;

impl WorkflowAdapter {
    pub const KEY: &'static str = "management.workflows";
    pub const SOURCE: &'static str = "workflows_adapter_v1";

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "version": FEATURE_SCHEMA_VERSION,
            "source": Self::SOURCE,
            "sections": [{
                "title": "Automation safety limits",
                "description": "Bounded message automations keep your server responsive.",
                "fields": [
                    {"key":"maxWorkflows","label":"Maximum workflows","kind":"number","min":1,"max":100,"help":"The maximum number of active and paused workflows stored for this server."},
                    {"key":"maxReplyLength","label":"Maximum reply length","kind":"number","min":1,"max":1500,"help":"Longer generated replies are rejected before they can be sent."},
                    {"key":"allowMentions","label":"Allow mentions in replies","kind":"toggle","help":"When disabled, everyone/here mentions are neutralised."}
                ]
            }]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({"maxWorkflows": 10, "maxReplyLength": 1000, "allowMentions": false})
    }
}

impl FeatureAdapter for WorkflowAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: Self::schema(),
            defaults: Self::defaults(),
            dependencies: vec!["message_content".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max, label) in [
            ("maxWorkflows", 1_i64, 100_i64, "Maximum workflows"),
            ("maxReplyLength", 1_i64, 1_500_i64, "Maximum reply length"),
        ] {
            if let Some(value) = object.get(field) {
                if let Some(value) = value.as_i64() {
                    if !(min..=max).contains(&value) {
                        issues.push(ValidationIssue {
                            path: field.into(),
                            code: "out_of_range".into(),
                            message: format!("{label} must be between {min} and {max}."),
                            severity: "error".into(),
                        });
                    }
                } else {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "integer_required".into(),
                        message: format!("{label} must be an integer."),
                        severity: "error".into(),
                    });
                }
            }
        }
        if object
            .get("allowMentions")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "allowMentions".into(),
                code: "boolean_required".into(),
                message: "Mention permission must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object
            .get("maxWorkflows")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "management.workflows.max_workflows".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("maxReplyLength")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "management.workflows.max_reply_length".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("allowMentions")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push((
                "management.workflows.allow_mentions".into(),
                value.to_string(),
            ));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PrivacyAdapter;

impl PrivacyAdapter {
    pub const KEY: &'static str = "management.privacy";
    pub const SOURCE: &'static str = "privacy_adapter_v1";
}

impl FeatureAdapter for PrivacyAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Member data controls",
                    "description": "Choose which self-service privacy requests members can make.",
                    "fields": [
                        {"key":"allowMemberExport","label":"Allow member exports","kind":"toggle","help":"Members can request their own JSON export through /privacy data."},
                        {"key":"allowMemberErase","label":"Allow member erasure","kind":"toggle","help":"Voluntary profile data is erased; moderation audit records stay retained."},
                        {"key":"maxExportBytes","label":"Maximum export size","kind":"number","min":65536,"max":10000000,"help":"Protects DMs and storage from oversized exports."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"allowMemberExport": true, "allowMemberErase": true, "maxExportBytes": 1000000}),
            dependencies: vec!["direct_messages".into(), "manager_session".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["allowMemberExport", "allowMemberErase"] {
            if object.get(field).is_some_and(|value| !value.is_boolean()) {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "boolean_required".into(),
                    message: "This privacy option must be true or false.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("maxExportBytes") {
            if let Some(value) = value.as_i64() {
                if !(65_536..=10_000_000).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "maxExportBytes".into(),
                        code: "out_of_range".into(),
                        message: "Export size must be between 65536 and 10000000 bytes.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "maxExportBytes".into(),
                    code: "integer_required".into(),
                    message: "Export size must be an integer.".into(),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        for (field, key) in [
            (
                "allowMemberExport",
                "management.privacy.allow_member_export",
            ),
            ("allowMemberErase", "management.privacy.allow_member_erase"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_bool) {
                projection.push((key.into(), value.to_string()));
            }
        }
        if let Some(value) = object
            .get("maxExportBytes")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "management.privacy.max_export_bytes".into(),
                value.to_string(),
            ));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StatsAdapter;

impl StatsAdapter {
    pub const KEY: &'static str = "insights.stats";
    pub const SOURCE: &'static str = "stats_adapter_v1";
}

impl FeatureAdapter for StatsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Server statistics",
                    "description": "Control the period and visibility of /serverstats.",
                    "fields": [
                        {"key":"windowDays","label":"Reporting window (days)","kind":"number","min":1,"max":30,"help":"Number of recent daily snapshots included."},
                        {"key":"public","label":"Show publicly","kind":"toggle","help":"When disabled, only the requesting member sees the summary."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"windowDays": 7, "public": false}),
            dependencies: vec!["message_events".into(), "scheduler".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("windowDays") {
            if let Some(value) = value.as_i64() {
                if !(1..=30).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "windowDays".into(),
                        code: "out_of_range".into(),
                        message: "Reporting window must be between 1 and 30 days.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "windowDays".into(),
                    code: "integer_required".into(),
                    message: "Reporting window must be an integer.".into(),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("public")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "public".into(),
                code: "boolean_required".into(),
                message: "Public visibility must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        if let Some(value) = object.get("windowDays").and_then(serde_json::Value::as_i64) {
            projection.push(("insights.stats.window_days".into(), value.to_string()));
        }
        if let Some(value) = object.get("public").and_then(serde_json::Value::as_bool) {
            projection.push(("insights.stats.public".into(), value.to_string()));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HelpAdapter;

impl HelpAdapter {
    pub const KEY: &'static str = "utility.help";
    pub const SOURCE: &'static str = "help_adapter_v1";
}

impl FeatureAdapter for HelpAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Help message",
                    "description": "Choose the links and module list shown by /help.",
                    "fields": [
                        {"key":"showModules","label":"Show module list","kind":"toggle","help":"Include the enabled module summary in the response."},
                        {"key":"showDashboard","label":"Include dashboard link","kind":"toggle","help":"Append the dashboard URL for server managers."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"showModules": true, "showDashboard": true}),
            dependencies: vec!["send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        ["showModules", "showDashboard"]
            .into_iter()
            .filter(|field| object.get(*field).is_some_and(|value| !value.is_boolean()))
            .map(|field| ValidationIssue {
                path: field.into(),
                code: "boolean_required".into(),
                message: "This help option must be true or false.".into(),
                severity: "error".into(),
            })
            .collect()
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        [
            ("showModules", "utility.help.show_modules"),
            ("showDashboard", "utility.help.show_dashboard"),
        ]
        .into_iter()
        .filter_map(|(field, key)| {
            object
                .get(field)
                .and_then(serde_json::Value::as_bool)
                .map(|value| (key.into(), value.to_string()))
        })
        .collect()
    }
}

/// Controls the safety envelope for manual moderation commands.  The command
/// handlers read the projected values before touching Discord, so the panel
/// can no longer expose limits that are ignored by the runtime.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModerationAdapter;

impl ModerationAdapter {
    pub const KEY: &'static str = "management.moderation";
    pub const SOURCE: &'static str = "moderation_adapter_v1";
}

impl FeatureAdapter for ModerationAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Moderation safety",
                    "description": "Set guardrails used by manual moderation commands.",
                    "fields": [
                        {"key":"requireReason","label":"Require a reason","kind":"toggle","help":"Reject destructive actions without a useful reason."},
                        {"key":"maxPurge","label":"Maximum purge count","kind":"number","min":1,"max":100,"help":"Upper bound for one purge command."},
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "requireReason": true,
                "maxPurge": 100
            }),
            dependencies: vec!["moderate_members".into(), "manage_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["requireReason"] {
            if object.get(field).is_some_and(|value| !value.is_boolean()) {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "boolean_required".into(),
                    message: "This option must be true or false.".into(),
                    severity: "error".into(),
                });
            }
        }
        for (field, min, max) in [("maxPurge", 1_i64, 100_i64)] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64)
                && !(min..=max).contains(&value)
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: format!("The value must be between {min} and {max}."),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (field, setting) in [("requireReason", "management.moderation.require_reason")] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_bool) {
                pairs.push((setting.into(), value.to_string()));
            }
        }
        for (field, setting) in [("maxPurge", "management.moderation.max_purge")] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64) {
                pairs.push((setting.into(), value.to_string()));
            }
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AntiScamAdapter;

impl AntiScamAdapter {
    pub const KEY: &'static str = "protection.antiscam";
    pub const SOURCE: &'static str = "anti_scam_adapter_v1";
}

impl FeatureAdapter for AntiScamAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Scam protection",
                    "description": "Detect invite scams, suspicious domains and common social-engineering phrases.",
                    "fields": [
                        {"key":"blockInvites","label":"Block unsolicited Discord invites","kind":"toggle"},
                        {"key":"blockedDomains","label":"Blocked domains","kind":"tags","max":100,"advanced":true},
                        {"key":"blockedKeywords","label":"Blocked phrases","kind":"tags","max":100,"advanced":true},
                        {"key":"ignoredChannels","label":"Ignored channels","kind":"channels","max":100,"advanced":true},
                        {"key":"logChannel","label":"Log channel","kind":"channel","advanced":true},
                        {"key":"timeoutSeconds","label":"Timeout (seconds)","kind":"number","min":0,"max":86400,"advanced":true},
                        {"key":"alertOnly","label":"Monitor only","kind":"toggle","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "blockInvites": true,
                "blockedDomains": [],
                "blockedKeywords": ["free nitro", "steam gift", "claim your prize", "verify your wallet"],
                "ignoredChannels": [],
                "logChannel": "",
                "timeoutSeconds": 300,
                "alertOnly": false
            }),
            dependencies: vec!["message_content_intent".into(), "manage_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["blockInvites", "alertOnly"] {
            if object.get(field).is_some_and(|value| !value.is_boolean()) {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "boolean_required".into(),
                    message: "This option must be true or false.".into(),
                    severity: "error".into(),
                });
            }
        }
        for (field, max, allow_url) in [
            ("blockedDomains", 100_usize, true),
            ("blockedKeywords", 100, false),
            ("ignoredChannels", 100, false),
        ] {
            if let Some(value) = object.get(field) {
                let valid = value.as_array().is_some_and(|items| {
                    items.len() <= max
                        && items.iter().all(|item| {
                            item.as_str().is_some_and(|text| {
                                let text = text.trim();
                                !text.is_empty()
                                    && text.chars().count() <= 200
                                    && !text.chars().any(char::is_control)
                                    && (!allow_url || !text.contains('/'))
                            })
                        })
                });
                if !valid {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "invalid_list".into(),
                        message: "Use a bounded list of non-empty values.".into(),
                        severity: "error".into(),
                    });
                }
            }
        }
        if let Some(value) = object
            .get("timeoutSeconds")
            .and_then(serde_json::Value::as_i64)
            && !(0..=86_400).contains(&value)
        {
            issues.push(ValidationIssue {
                path: "timeoutSeconds".into(),
                code: "out_of_range".into(),
                message: "Timeout must be between 0 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("logChannel")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "logChannel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel for protection logs.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        if let Some(value) = object
            .get("blockInvites")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push(("security.antiscam.block_invites".into(), value.to_string()));
        }
        if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
            pairs.push(("security.antiscam.alert_only".into(), value.to_string()));
        }
        if let Some(value) = object.get("logChannel").and_then(serde_json::Value::as_str) {
            pairs.push(("security.antiscam.log_channel".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("timeoutSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            pairs.push((
                "security.antiscam.timeout_seconds".into(),
                value.to_string(),
            ));
        }
        for (field, setting) in [
            ("blockedDomains", "security.antiscam.blocked_domains"),
            ("blockedKeywords", "security.antiscam.blocked_keywords"),
            ("ignoredChannels", "security.antiscam.ignored_channels"),
        ] {
            if let Some(values) = object.get(field).and_then(serde_json::Value::as_array) {
                pairs.push((
                    setting.into(),
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
            }
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WelcomeChannelAdapter;

impl WelcomeChannelAdapter {
    pub const KEY: &'static str = "support.welcome_channel";
    pub const SOURCE: &'static str = "welcome_channel_adapter_v1";
}

impl FeatureAdapter for WelcomeChannelAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Guided welcome channel",
                    "description": "Post the first steps and server rules when a new member arrives.",
                    "fields": [
                        {"key":"channelId","label":"Welcome channel","kind":"channel"},
                        {"key":"message","label":"Guide message","kind":"textarea","min":1,"max":2000,"help":"Use {member} and {server} as placeholders."}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "channelId": "",
                "message": "Welcome {member}! Start with the rules, introduce yourself and check the server channels."
            }),
            dependencies: vec!["send_messages".into(), "view_channel".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if !object
            .get("channelId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.is_empty() || value.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "channelId".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel.".into(),
                severity: "error".into(),
            });
        }
        if !object
            .get("message")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty() && value.chars().count() <= 2_000)
        {
            issues.push(ValidationIssue {
                path: "message".into(),
                code: "invalid_message".into(),
                message: "The guide message must contain 1-2000 characters.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        if let Some(value) = object.get("channelId").and_then(serde_json::Value::as_str) {
            pairs.push(("support.welcome_channel.channel_id".into(), value.into()));
        }
        if let Some(value) = object.get("message").and_then(serde_json::Value::as_str) {
            pairs.push(("support.welcome_channel.message".into(), value.into()));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LevelsAdapter;

impl LevelsAdapter {
    pub const KEY: &'static str = "community.levels";
    pub const SOURCE: &'static str = "levels_adapter_v1";
}

impl FeatureAdapter for LevelsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "XP progression",
                    "description": "Tune message XP and level-up announcements.",
                    "fields": [
                        {"key":"xpMin","label":"Minimum XP per message","kind":"number","min":1,"max":1000},
                        {"key":"xpMax","label":"Maximum XP per message","kind":"number","min":1,"max":2000},
                        {"key":"cooldownSeconds","label":"XP cooldown (seconds)","kind":"number","min":0,"max":3600},
                        {"key":"voiceXpEnabled","label":"Award XP in voice channels","kind":"toggle","advanced":true},
                        {"key":"voiceXpPerMinute","label":"Voice XP per minute","kind":"number","min":0,"max":30,"advanced":true},
                        {"key":"ignoredChannels","label":"Channels that do not award XP","kind":"channels","max":100,"advanced":true},
                        {"key":"levelRoles","label":"Level role rewards (level=role ID)","kind":"tags","max":50,"advanced":true},
                        {"key":"stackRoles","label":"Keep previous level roles","kind":"toggle","advanced":true},
                        {"key":"announceChannel","label":"Level-up channel","kind":"channel","advanced":true},
                        {"key":"announceTemplate","label":"Level-up message","kind":"text","max":1000,"advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "xpMin": 15,
                "xpMax": 30,
                "cooldownSeconds": 60,
                "voiceXpEnabled": false,
                "voiceXpPerMinute": 2,
                "ignoredChannels": [],
                "levelRoles": [],
                "stackRoles": true,
                "announceChannel": "",
                "announceTemplate": "{member} reached level {level}!"
            }),
            dependencies: vec!["message_content_intent".into(), "send_messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max) in [
            ("xpMin", 1_i64, 1_000_i64),
            ("xpMax", 1, 2_000),
            ("cooldownSeconds", 0, 3_600),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64)
                && !(min..=max).contains(&value)
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: format!("The value must be between {min} and {max}."),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("xpMin")
            .and_then(serde_json::Value::as_i64)
            .zip(object.get("xpMax").and_then(serde_json::Value::as_i64))
            .is_some_and(|(min, max)| max < min)
        {
            issues.push(ValidationIssue {
                path: "xpMax".into(),
                code: "must_be_at_least_minimum".into(),
                message: "Maximum XP must be at least minimum XP.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("announceChannel")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "announceChannel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("announceTemplate")
            && !value
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= 1_000)
        {
            issues.push(ValidationIssue {
                path: "announceTemplate".into(),
                code: "invalid_template".into(),
                message: "The announcement must contain 1-1000 characters.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("voiceXpEnabled")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "voiceXpEnabled".into(),
                code: "boolean_required".into(),
                message: "Voice XP enabled must be true or false.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("voiceXpPerMinute") {
            if let Some(value) = value.as_i64() {
                if !(0..=30).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "voiceXpPerMinute".into(),
                        code: "out_of_range".into(),
                        message: "Voice XP per minute must be between 0 and 30.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "voiceXpPerMinute".into(),
                    code: "integer_required".into(),
                    message: "Voice XP per minute must be an integer.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("ignoredChannels") {
            if let Some(values) = value.as_array() {
                if values.len() > 100
                    || values.iter().any(|item| {
                        !item
                            .as_str()
                            .is_some_and(|text| text.parse::<u64>().is_ok())
                    })
                {
                    issues.push(ValidationIssue {
                        path: "ignoredChannels".into(),
                        code: "invalid_channel_ids".into(),
                        message: "Ignored channels must contain at most 100 Discord channel IDs."
                            .into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "ignoredChannels".into(),
                    code: "array_required".into(),
                    message: "Ignored channels must be an array.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("levelRoles") {
            if let Some(values) = value.as_array() {
                if values.len() > 50
                    || values.iter().any(|item| {
                        let Some(entry) = item.as_str() else {
                            return true;
                        };
                        let mut parts = entry.split('=');
                        let level = parts
                            .next()
                            .and_then(|part| part.trim().parse::<u32>().ok());
                        let role = parts
                            .next()
                            .and_then(|part| part.trim().parse::<u64>().ok());
                        level.is_none() || role.is_none() || parts.next().is_some()
                    })
                {
                    issues.push(ValidationIssue {
                        path: "levelRoles".into(),
                        code: "invalid_level_roles".into(),
                        message: "Rewards must use level=role ID, with at most 50 entries.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "levelRoles".into(),
                    code: "array_required".into(),
                    message: "Level rewards must be an array.".into(),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("stackRoles")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "stackRoles".into(),
                code: "boolean_required".into(),
                message: "Stack roles must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (field, setting) in [
            ("xpMin", "community.levels.xp_min"),
            ("xpMax", "community.levels.xp_max"),
            ("cooldownSeconds", "community.levels.cooldown_seconds"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64) {
                pairs.push((setting.into(), value.to_string()));
            }
        }
        for (field, setting) in [
            ("announceChannel", "community.levels.announce_channel"),
            ("announceTemplate", "community.levels.announce_template"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                pairs.push((setting.into(), value.into()));
            }
        }
        if let Some(value) = object
            .get("voiceXpEnabled")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push((
                "community.levels.voice_xp_enabled".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("voiceXpPerMinute")
            .and_then(serde_json::Value::as_i64)
        {
            pairs.push((
                "community.levels.voice_xp_per_minute".into(),
                value.to_string(),
            ));
        }
        if let Some(values) = object
            .get("ignoredChannels")
            .and_then(serde_json::Value::as_array)
        {
            pairs.push((
                "community.levels.ignored_channels".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if let Some(values) = object
            .get("levelRoles")
            .and_then(serde_json::Value::as_array)
        {
            pairs.push((
                "community.levels.level_roles".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if let Some(value) = object
            .get("stackRoles")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push(("community.levels.stack_roles".into(), value.to_string()));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StarboardAdapter;

impl StarboardAdapter {
    pub const KEY: &'static str = "community.starboard";
    pub const SOURCE: &'static str = "starboard_adapter_v1";
}

impl FeatureAdapter for StarboardAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Starboard",
                    "description": "Choose the channel and reaction score required to feature a message.",
                    "fields": [
                        {"key":"channel","label":"Starboard channel","kind":"channel"},
                        {"key":"threshold","label":"Stars required","kind":"number","min":1,"max":100},
                        {"key":"emoji","label":"Reaction emoji","kind":"text","min":1,"max":32},
                        {"key":"allowSelfStar","label":"Allow authors to star their own message","kind":"toggle","advanced":true},
                        {"key":"includeImages","label":"Include image attachments","kind":"toggle","advanced":true},
                        {"key":"ignoredChannels","label":"Ignored channels","kind":"channels","max":100,"advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "channel": "",
                "threshold": 3,
                "emoji": "⭐",
                "allowSelfStar": false,
                "includeImages": true,
                "ignoredChannels": []
            }),
            dependencies: vec![
                "read_message_history".into(),
                "add_reactions".into(),
                "send_messages".into(),
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("threshold").and_then(serde_json::Value::as_i64)
            && !(1..=100).contains(&value)
        {
            issues.push(ValidationIssue {
                path: "threshold".into(),
                code: "out_of_range".into(),
                message: "Stars required must be between 1 and 100.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("channel")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "channel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (field, setting) in [
            ("channel", "community.starboard.channel_id"),
            ("emoji", "community.starboard.emoji"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                pairs.push((setting.into(), value.into()));
            }
        }
        if let Some(value) = object.get("threshold").and_then(serde_json::Value::as_i64) {
            pairs.push(("community.starboard.threshold".into(), value.to_string()));
        }
        for (field, setting) in [
            ("allowSelfStar", "community.starboard.allow_self_star"),
            ("includeImages", "community.starboard.include_images"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_bool) {
                pairs.push((setting.into(), value.to_string()));
            }
        }
        if let Some(values) = object
            .get("ignoredChannels")
            .and_then(serde_json::Value::as_array)
        {
            pairs.push((
                "community.starboard.ignored_channels".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AntiRaidAdapter;

impl AntiRaidAdapter {
    pub const KEY: &'static str = "protection.anti_raid";
    pub const SOURCE: &'static str = "anti_raid_adapter_v1";
}

impl FeatureAdapter for AntiRaidAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Join burst protection",
                    "description": "Enable the gate when too many members arrive in a short window.",
                    "fields": [
                        {"key":"joinThreshold","label":"Joins before containment","kind":"number","min":2,"max":100},
                        {"key":"windowSeconds","label":"Window (seconds)","kind":"number","min":3,"max":60},
                        {"key":"incidentMinutes","label":"Containment duration (minutes)","kind":"number","min":1,"max":120},
                        {"key":"verification","label":"Verification level","kind":"select","options":[["medium","Medium"],["high","High"],["very_high","Very high"]]},
                        {"key":"alertChannel","label":"Alert channel","kind":"channel","advanced":true},
                        {"key":"alertOnly","label":"Monitor only","kind":"toggle","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "joinThreshold": 10,
                "windowSeconds": 10,
                "incidentMinutes": 10,
                "verification": "high",
                "alertChannel": "",
                "alertOnly": false
            }),
            dependencies: vec!["guild_members_intent".into(), "manage_roles".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for (field, min, max) in [
            ("joinThreshold", 2_i64, 100_i64),
            ("windowSeconds", 3, 60),
            ("incidentMinutes", 1, 120),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64)
                && !(min..=max).contains(&value)
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: format!("The value must be between {min} and {max}."),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("alertChannel")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "alertChannel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("alertOnly")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "alertOnly".into(),
                code: "boolean_required".into(),
                message: "Monitor only must be true or false.".into(),
                severity: "error".into(),
            });
        }
        if object.get("verification").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|value| matches!(value, "medium" | "high" | "very_high"))
        }) {
            issues.push(ValidationIssue {
                path: "verification".into(),
                code: "invalid_choice".into(),
                message: "Choose medium, high or very high verification.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (field, setting) in [
            ("joinThreshold", "security.anti_raid.joins"),
            ("windowSeconds", "security.anti_raid.window_seconds"),
            ("incidentMinutes", "security.anti_raid.incident_minutes"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64) {
                pairs.push((setting.into(), value.to_string()));
            }
        }
        if let Some(value) = object
            .get("alertChannel")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("security.anti_raid.alert_channel".into(), value.into()));
        }
        if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
            pairs.push(("security.anti_raid.alert_only".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("verification")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("security.anti_raid.verification".into(), value.into()));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JoinGateAdapter;

impl JoinGateAdapter {
    pub const KEY: &'static str = "protection.join_gate";
    pub const SOURCE: &'static str = "join_gate_adapter_v1";
}

impl FeatureAdapter for JoinGateAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Join verification",
                    "description": "Hold accounts below the configured age behind a verification role.",
                    "fields": [
                        {"key":"minimumAccountDays","label":"Minimum account age (days)","kind":"number","min":0,"max":365},
                        {"key":"requireAvatar","label":"Require a profile avatar","kind":"toggle"},
                        {"key":"action","label":"Action for suspicious accounts","kind":"select","options":[["quarantine","Quarantine"],["alert","Alert only"]]},
                        {"key":"verifiedRole","label":"Quarantine role","kind":"role"},
                        {"key":"autoRole","label":"Role for verified members","kind":"role"},
                        {"key":"blockedNamePatterns","label":"Blocked name patterns","kind":"tags","advanced":true},
                        {"key":"logChannel","label":"Log channel","kind":"channel","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({"minimumAccountDays": 0, "requireAvatar": false, "action": "quarantine", "verifiedRole": "", "autoRole": "", "blockedNamePatterns": [], "logChannel": ""}),
            dependencies: vec!["guild_members_intent".into(), "manage_roles".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object
            .get("minimumAccountDays")
            .and_then(serde_json::Value::as_i64)
            && !(0..=365).contains(&value)
        {
            issues.push(ValidationIssue {
                path: "minimumAccountDays".into(),
                code: "out_of_range".into(),
                message: "Account age must be between 0 and 365 days.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("verifiedRole")
            && !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "verifiedRole".into(),
                code: "invalid_role_id".into(),
                message: "Choose a valid Discord role.".into(),
                severity: "error".into(),
            });
        }
        if object.get("autoRole").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        }) {
            issues.push(ValidationIssue {
                path: "autoRole".into(),
                code: "invalid_role_id".into(),
                message: "Choose a valid Discord role.".into(),
                severity: "error".into(),
            });
        }
        if object.get("logChannel").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok())
        }) {
            issues.push(ValidationIssue {
                path: "logChannel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("requireAvatar")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "requireAvatar".into(),
                code: "boolean_required".into(),
                message: "Require avatar must be true or false.".into(),
                severity: "error".into(),
            });
        }
        if object.get("action").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|value| matches!(value, "quarantine" | "alert"))
        }) {
            issues.push(ValidationIssue {
                path: "action".into(),
                code: "invalid_choice".into(),
                message: "Choose quarantine or alert.".into(),
                severity: "error".into(),
            });
        }
        if let Some(patterns) = object.get("blockedNamePatterns") {
            let valid = patterns.as_array().is_some_and(|values| {
                values.len() <= 20
                    && values.iter().all(|value| {
                        value.as_str().is_some_and(|pattern| {
                            let length = pattern.chars().count();
                            (1..=64).contains(&length) && !pattern.chars().any(char::is_control)
                        })
                    })
            });
            if !valid {
                issues.push(ValidationIssue {
                    path: "blockedNamePatterns".into(),
                    code: "invalid_patterns".into(),
                    message: "Use up to 20 name patterns, each 1-64 characters.".into(),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        if let Some(value) = object
            .get("minimumAccountDays")
            .and_then(serde_json::Value::as_i64)
        {
            pairs.push(("security.join_gate.min_age_days".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("verifiedRole")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("security.join_gate.role_id".into(), value.into()));
        }
        if let Some(value) = object
            .get("requireAvatar")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push((
                "security.join_gate.require_avatar".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object.get("action").and_then(serde_json::Value::as_str) {
            pairs.push(("security.join_gate.action".into(), value.into()));
        }
        if let Some(value) = object.get("autoRole").and_then(serde_json::Value::as_str) {
            pairs.push(("security.join_gate.auto_role_id".into(), value.into()));
        }
        if let Some(value) = object.get("logChannel").and_then(serde_json::Value::as_str) {
            pairs.push(("security.join_gate.log_channel".into(), value.into()));
        }
        if let Some(values) = object
            .get("blockedNamePatterns")
            .and_then(serde_json::Value::as_array)
        {
            pairs.push((
                "security.join_gate.blocked_name_patterns".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
        pairs
    }
}

impl AntiSpamAdapter {
    pub const KEY: &'static str = "protection.antispam";
    pub const SOURCE: &'static str = "anti_spam_adapter_v1";

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "version": FEATURE_SCHEMA_VERSION,
            "source": Self::SOURCE,
            "sections": [
                {
                    "title": "Resposta automática",
                    "description": "Deteta padrões de flood, repetição e menções com limites seguros.",
                    "fields": [
                        {"key":"floodCount","label":"Mensagens no intervalo","kind":"number","min":3,"max":30,"help":"Número de mensagens do mesmo membro antes de sinalizar."},
                        {"key":"windowSeconds","label":"Janela de tempo (segundos)","kind":"number","min":3,"max":60,"help":"As mensagens antigas saem automaticamente desta janela."},
                        {"key":"duplicateLimit","label":"Repetições iguais","kind":"number","min":2,"max":12,"help":"Quantas mensagens iguais acionam a regra."},
                        {"key":"timeoutSeconds","label":"Timeout inicial (segundos)","kind":"number","min":0,"max":86400,"help":"Usa 0 para apenas registar o incidente."}
                    ]
                },
                {
                    "title": "Exceções e alertas",
                    "description": "Escolhe recursos reais do servidor para evitar falsos positivos e receber contexto.",
                    "fields": [
                        {"key":"mentionLimit","label":"Limite de menções por mensagem","kind":"number","min":1,"max":30,"advanced":true},
                        {"key":"ignoredChannels","label":"Canais ignorados","kind":"channels","help":"Mensagens nestes canais não entram no avaliador.","advanced":true},
                        {"key":"ignoredRoles","label":"Cargos ignorados","kind":"roles","help":"Membros com um destes cargos não entram no avaliador.","advanced":true},
                        {"key":"logChannel","label":"Canal de registo","kind":"channel","help":"O Helper publica aqui o motivo e o modo da decisão.","advanced":true},
                        {"key":"alertOnly","label":"Apenas alertar, sem aplicar castigo","kind":"toggle","help":"Mantém a deteção e auditoria, mas não aplica timeout.","advanced":true}
                    ]
                }
            ]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({
            "floodCount": 6,
            "windowSeconds": 10,
            "duplicateLimit": 3,
            "timeoutSeconds": 60,
            "mentionLimit": 5,
            "ignoredChannels": [],
            "ignoredRoles": [],
            "logChannel": "",
            "alertOnly": false
        })
    }
}

impl FeatureAdapter for AntiSpamAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: Self::schema(),
            defaults: Self::defaults(),
            dependencies: vec!["message_content_intent".into(), "moderate_members".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if !config.is_object() {
            issues.push(ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "A configuração tem de ser um objeto.".into(),
                severity: "error".into(),
            });
            return issues;
        }
        for (name, min, max) in [
            ("floodCount", 3_i64, 30_i64),
            ("windowSeconds", 3, 60),
            ("duplicateLimit", 2, 12),
            ("timeoutSeconds", 0, 86_400),
            ("mentionLimit", 1, 30),
        ] {
            if let Some(value) = config.get(name).and_then(serde_json::Value::as_i64)
                && !(min..=max).contains(&value)
            {
                issues.push(ValidationIssue {
                    path: name.into(),
                    code: "out_of_range".into(),
                    message: format!("O valor tem de estar entre {min} e {max}."),
                    severity: "error".into(),
                });
            }
        }
        for field in ["ignoredChannels", "ignoredRoles"] {
            if let Some(value) = config.get(field) {
                let valid = value.as_array().is_some_and(|items| {
                    items.len() <= 100
                        && items.iter().all(|item| {
                            item.as_str().is_some_and(|text| {
                                !text.is_empty()
                                    && text.chars().count() <= 64
                                    && !text.chars().any(char::is_control)
                            })
                        })
                });
                if !valid {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "invalid_exemptions".into(),
                        message: "Indica no máximo 100 IDs ou nomes válidos.".into(),
                        severity: "error".into(),
                    });
                }
            }
        }
        if let Some(value) = config.get("logChannel") {
            let valid = value
                .as_str()
                .is_some_and(|text| text.is_empty() || text.parse::<u64>().is_ok());
            if !valid {
                issues.push(ValidationIssue {
                    path: "logChannel".into(),
                    code: "invalid_channel_id".into(),
                    message: "Escolhe um canal Discord válido para o registo.".into(),
                    severity: "error".into(),
                });
            }
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        let mut add = |name: &str, value: String| pairs.push((name.to_string(), value));
        for (field, setting) in [
            ("floodCount", "security.antispam.flood_count"),
            ("windowSeconds", "security.antispam.window_seconds"),
            ("duplicateLimit", "security.antispam.duplicate_limit"),
            ("timeoutSeconds", "security.antispam.timeout_seconds"),
            ("mentionLimit", "security.antispam.mention_limit"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64) {
                add(setting, value.to_string());
            }
        }
        if let Some(value) = object.get("logChannel").and_then(serde_json::Value::as_str) {
            add("security.antispam.log_channel", value.to_string());
        }
        for (field, setting) in [
            ("ignoredChannels", "security.antispam.ignored_channels"),
            ("ignoredRoles", "security.antispam.ignored_roles"),
        ] {
            if let Some(values) = object.get(field).and_then(serde_json::Value::as_array) {
                add(
                    setting,
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
        }
        if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
            add("security.antispam.alert_only", value.to_string());
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AchievementsAdapter;

impl AchievementsAdapter {
    pub const KEY: &'static str = "community.achievements";
    pub const SOURCE: &'static str = "achievements_adapter_v2";
}

impl FeatureAdapter for AchievementsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "XP milestones",
                    "description": "Choose the real XP thresholds used by the achievements command.",
                    "fields": [
                        {"key":"firstThreshold","label":"First steps threshold","kind":"number","min":1,"max":1000000},
                        {"key":"regularThreshold","label":"Regular threshold","kind":"number","min":1,"max":1000000},
                        {"key":"pillarThreshold","label":"Community pillar threshold","kind":"number","min":1,"max":1000000}
                    ]
                }]
            }),
            defaults: serde_json::json!({"firstThreshold":100,"regularThreshold":1000,"pillarThreshold":10000}),
            dependencies: vec![
                "message_content".into(),
                "levels".into(),
                "send_messages".into(),
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Achievement settings must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        for field in ["firstThreshold", "regularThreshold", "pillarThreshold"] {
            match object.get(field).and_then(serde_json::Value::as_i64) {
                Some(value) if (1..=1_000_000).contains(&value) => {}
                Some(_) => issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: "XP threshold must be between 1 and 1000000.".into(),
                    severity: "error".into(),
                }),
                None => issues.push(ValidationIssue {
                    path: field.into(),
                    code: "integer_required".into(),
                    message: "XP threshold must be an integer.".into(),
                    severity: "error".into(),
                }),
            }
        }
        if let (Some(first), Some(regular), Some(pillar)) = (
            object
                .get("firstThreshold")
                .and_then(serde_json::Value::as_i64),
            object
                .get("regularThreshold")
                .and_then(serde_json::Value::as_i64),
            object
                .get("pillarThreshold")
                .and_then(serde_json::Value::as_i64),
        ) && !(first <= regular && regular <= pillar)
        {
            issues.push(ValidationIssue {
                path: "thresholds".into(),
                code: "ordered_required".into(),
                message: "Milestones must be ordered from smallest to largest.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        [
            ("firstThreshold", "community.achievements.first_threshold"),
            (
                "regularThreshold",
                "community.achievements.regular_threshold",
            ),
            ("pillarThreshold", "community.achievements.pillar_threshold"),
        ]
        .into_iter()
        .filter_map(|(field, key)| {
            config
                .get(field)
                .and_then(serde_json::Value::as_i64)
                .map(|value| (key.into(), value.to_string()))
        })
        .collect()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InviteTrackerAdapter;

impl InviteTrackerAdapter {
    pub const KEY: &'static str = "management.invite_tracker";
    pub const SOURCE: &'static str = "invite_tracker_adapter_v2";
}

impl FeatureAdapter for InviteTrackerAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version":FEATURE_SCHEMA_VERSION,"source":Self::SOURCE,"sections":[{"title":"Invite overview","description":"Control the staff-facing invite list without scraping or storing invite tokens.","fields":[{"key":"maxEntries","label":"Invites shown","kind":"number","min":1,"max":50},{"key":"includeInviter","label":"Show inviter names","kind":"toggle"}]}]}),
            defaults: serde_json::json!({"maxEntries":10,"includeInviter":true}),
            dependencies: vec![
                "manage_guild".into(),
                "guild_invites".into(),
                "send_messages".into(),
            ],
        }
    }
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Invite tracker settings must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("maxEntries")
            && !value
                .as_i64()
                .is_some_and(|value| (1..=50).contains(&value))
        {
            issues.push(ValidationIssue {
                path: "maxEntries".into(),
                code: "out_of_range".into(),
                message: "Invites shown must be between 1 and 50.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("includeInviter")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "includeInviter".into(),
                code: "boolean_required".into(),
                message: "Include inviter names must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(value) = config.get("maxEntries").and_then(serde_json::Value::as_i64) {
            pairs.push((
                "management.invite_tracker.max_entries".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = config
            .get("includeInviter")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push((
                "management.invite_tracker.include_inviter".into(),
                value.to_string(),
            ));
        }
        pairs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmojisAdapter;

impl EmojisAdapter {
    pub const KEY: &'static str = "utility.emojis";
    pub const SOURCE: &'static str = "emojis_adapter_v2";
}

impl FeatureAdapter for EmojisAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version":FEATURE_SCHEMA_VERSION,"source":Self::SOURCE,"sections":[{"title":"Emoji inventory","description":"Choose how many custom emojis the staff command lists.","fields":[{"key":"maxEntries","label":"Emojis shown","kind":"number","min":1,"max":100},{"key":"animatedOnly","label":"Only animated emojis","kind":"toggle"}]}]}),
            defaults: serde_json::json!({"maxEntries":50,"animatedOnly":false}),
            dependencies: vec!["manage_expressions".into(), "send_messages".into()],
        }
    }
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Emoji settings must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("maxEntries")
            && !value
                .as_i64()
                .is_some_and(|value| (1..=100).contains(&value))
        {
            issues.push(ValidationIssue {
                path: "maxEntries".into(),
                code: "out_of_range".into(),
                message: "Emojis shown must be between 1 and 100.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("animatedOnly")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "animatedOnly".into(),
                code: "boolean_required".into(),
                message: "Animated-only must be true or false.".into(),
                severity: "error".into(),
            });
        }
        issues
    }
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(value) = config.get("maxEntries").and_then(serde_json::Value::as_i64) {
            pairs.push(("utility.emojis.max_entries".into(), value.to_string()));
        }
        if let Some(value) = config
            .get("animatedOnly")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push(("utility.emojis.animated_only".into(), value.to_string()));
        }
        pairs
    }
}

static ANTI_SPAM_ADAPTER: AntiSpamAdapter = AntiSpamAdapter;
static NICKNAME_ADAPTER: NicknameAdapter = NicknameAdapter;
static REMINDER_ADAPTER: ReminderAdapter = ReminderAdapter;
static LEADERBOARD_ADAPTER: LeaderboardAdapter = LeaderboardAdapter;
static WORKFLOW_ADAPTER: WorkflowAdapter = WorkflowAdapter;
static PRIVACY_ADAPTER: PrivacyAdapter = PrivacyAdapter;
static STATS_ADAPTER: StatsAdapter = StatsAdapter;
static HELP_ADAPTER: HelpAdapter = HelpAdapter;
static MODERATION_ADAPTER: ModerationAdapter = ModerationAdapter;
static ANTI_SCAM_ADAPTER: AntiScamAdapter = AntiScamAdapter;
static WELCOME_CHANNEL_ADAPTER: WelcomeChannelAdapter = WelcomeChannelAdapter;
static LEVELS_ADAPTER: LevelsAdapter = LevelsAdapter;
static STARBOARD_ADAPTER: StarboardAdapter = StarboardAdapter;
static ANTI_RAID_ADAPTER: AntiRaidAdapter = AntiRaidAdapter;
static JOIN_GATE_ADAPTER: JoinGateAdapter = JoinGateAdapter;
static TICKETS_ADAPTER: TicketsAdapter = TicketsAdapter;
static WELCOME_ADAPTER: WelcomeAdapter = WelcomeAdapter;
static SUGGESTIONS_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "community.suggestions",
    source: "suggestions_adapter_v1",
    title: "Suggestion workflow",
    description: "Suggestions, voting and moderation commands are enabled for this server.",
    dependencies: &["send_messages", "interactions"],
};
static GIVEAWAYS_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "community.giveaways",
    source: "giveaways_adapter_v1",
    title: "Giveaway workflow",
    description: "Giveaway commands, entries, scheduling and rerolls are enabled for this server.",
    dependencies: &["send_messages", "scheduler", "interactions"],
};
static POLLS_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "management.polls",
    source: "polls_adapter_v1",
    title: "Poll workflow",
    description: "Poll creation, voting and scheduled closing are enabled for this server.",
    dependencies: &["send_messages", "scheduler", "interactions"],
};
static EVENTS_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "community.events",
    source: "events_adapter_v1",
    title: "Discord events",
    description: "Native scheduled events, registration, capacity and check-in are enabled for this server.",
    dependencies: &["manage_events", "scheduler"],
};
static ROLE_PANELS_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "community.role_panels",
    source: "role_panels_adapter_v1",
    title: "Role panel workflow",
    description: "Role panel commands and interaction-based assignment are enabled for this server.",
    dependencies: &["manage_roles", "interactions"],
};
static CUSTOM_COMMANDS_ADAPTER: CustomCommandsAdapter = CustomCommandsAdapter;
static AUDIT_ADAPTER: AuditAdapter = AuditAdapter;
static TEMPLATES_ADAPTER: TemplatesAdapter = TemplatesAdapter;
static BIRTHDAYS_ADAPTER: BirthdaysAdapter = BirthdaysAdapter;
static EMOJIS_ADAPTER: EmojisAdapter = EmojisAdapter;
static ACHIEVEMENTS_ADAPTER: AchievementsAdapter = AchievementsAdapter;
static INVITE_TRACKER_ADAPTER: InviteTrackerAdapter = InviteTrackerAdapter;

static TEMP_CHANNELS_ADAPTER: TempChannelsAdapter = TempChannelsAdapter;

static YOUTUBE_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "social.youtube",
    source: "youtube_data_api_v3_adapter_v1",
    title: "YouTube alerts",
    description: "Polls the official YouTube Data API and delivers deduplicated new-video alerts.",
    dependencies: &["YOUTUBE_API_KEY", "send_messages", "embed_links"],
};
// XP card uses a dedicated endpoint and renderer instead of the generic
// feature-settings publisher. Registering it here keeps catalogue health
// truthful while the specialised editor owns its colours and backgrounds.
static RANK_CARD_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "studio.rank_card",
    source: "rank_card_adapter_v1",
    title: "XP card",
    description: "Render the curated XP card configuration for Discord.",
    dependencies: &["attach_files"],
};
static RSS_ADAPTER: FeedAdapter = FeedAdapter {
    key: "social.rss",
    source: "rss_atom_adapter_v1",
    title: "RSS alerts",
    description: "Polls validated RSS and Atom feeds with SSRF protection and deduplication.",
    dependencies: &["outbound_https", "send_messages"],
};
static PODCASTS_ADAPTER: FeedAdapter = FeedAdapter {
    key: "social.podcasts",
    source: "podcast_rss_adapter_v1",
    title: "Podcast alerts",
    description: "Polls a public podcast RSS or Atom feed with the same SSRF protection and deduplication as RSS alerts.",
    dependencies: &["outbound_https", "send_messages"],
};
static TWITCH_ADAPTER: ToggleOnlyAdapter = ToggleOnlyAdapter {
    key: "social.twitch",
    source: "twitch_eventsub_adapter_v1",
    title: "Twitch alerts",
    description: "Uses the official Helix/EventSub API with signed webhook verification and deduplication.",
    dependencies: &[
        "TWITCH_CLIENT_ID",
        "TWITCH_CLIENT_SECRET",
        "TWITCH_EVENTSUB_SECRET",
        "send_messages",
    ],
};

#[derive(Debug, Clone, Copy, Default)]
pub struct EmbedsAdapter;

impl EmbedsAdapter {
    pub const KEY: &'static str = "utility.embeds";
    pub const SOURCE: &'static str = "embeds_adapter_v1";
}

impl FeatureAdapter for EmbedsAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version": FEATURE_SCHEMA_VERSION, "source": Self::SOURCE, "sections": [{"title":"Safe embed publishing", "description":"Publish a bounded embed from a slash command; mentions are disabled by default.", "fields":[{"key":"maxDescription","label":"Maximum description length","kind":"number","min":1,"max":4000}]}]}),
            defaults: serde_json::json!({"maxDescription": 2000}),
            dependencies: vec!["send_messages".into(), "embed_links".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        match object.get("maxDescription") {
            Some(value)
                if value
                    .as_i64()
                    .is_some_and(|value| (1..=4000).contains(&value)) =>
            {
                Vec::new()
            }
            Some(value) if value.as_i64().is_some() => vec![ValidationIssue {
                path: "maxDescription".into(),
                code: "out_of_range".into(),
                message: "Maximum description length must be between 1 and 4000.".into(),
                severity: "error".into(),
            }],
            Some(_) => vec![ValidationIssue {
                path: "maxDescription".into(),
                code: "integer_required".into(),
                message: "Maximum description length must be an integer.".into(),
                severity: "error".into(),
            }],
            None => Vec::new(),
        }
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        config
            .get("maxDescription")
            .and_then(serde_json::Value::as_i64)
            .map(|value| vec![("utility.embeds.max_description".into(), value.to_string())])
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EconomyAdapter;

impl EconomyAdapter {
    pub const KEY: &'static str = "community.economy";
    pub const SOURCE: &'static str = "economy_adapter_v1";
}

impl FeatureAdapter for EconomyAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version": FEATURE_SCHEMA_VERSION, "source": Self::SOURCE, "sections":[{"title":"Daily reward", "description":"A bounded daily reward backed by an auditable balance ledger.", "fields":[{"key":"dailyReward","label":"Daily reward","kind":"number","min":1,"max":10000}]}]}),
            defaults: serde_json::json!({"dailyReward": 100}),
            dependencies: vec!["scheduler".into()],
        }
    }
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "The configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        match object.get("dailyReward") {
            Some(value)
                if value
                    .as_i64()
                    .is_some_and(|value| (1..=10_000).contains(&value)) =>
            {
                Vec::new()
            }
            Some(value) if value.as_i64().is_some() => vec![ValidationIssue {
                path: "dailyReward".into(),
                code: "out_of_range".into(),
                message: "Daily reward must be between 1 and 10000.".into(),
                severity: "error".into(),
            }],
            Some(_) => vec![ValidationIssue {
                path: "dailyReward".into(),
                code: "integer_required".into(),
                message: "Daily reward must be an integer.".into(),
                severity: "error".into(),
            }],
            None => Vec::new(),
        }
    }
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        config
            .get("dailyReward")
            .and_then(serde_json::Value::as_i64)
            .map(|value| vec![("community.economy.daily_reward".into(), value.to_string())])
            .unwrap_or_default()
    }
}
static EMBEDS_ADAPTER: EmbedsAdapter = EmbedsAdapter;
static ECONOMY_ADAPTER: EconomyAdapter = EconomyAdapter;

pub fn feature_adapter(key: &str) -> Option<&'static dyn FeatureAdapter> {
    match key {
        AntiSpamAdapter::KEY => Some(&ANTI_SPAM_ADAPTER as &dyn FeatureAdapter),
        NicknameAdapter::KEY => Some(&NICKNAME_ADAPTER as &dyn FeatureAdapter),
        ReminderAdapter::KEY => Some(&REMINDER_ADAPTER as &dyn FeatureAdapter),
        LeaderboardAdapter::KEY => Some(&LEADERBOARD_ADAPTER as &dyn FeatureAdapter),
        WorkflowAdapter::KEY => Some(&WORKFLOW_ADAPTER as &dyn FeatureAdapter),
        PrivacyAdapter::KEY => Some(&PRIVACY_ADAPTER as &dyn FeatureAdapter),
        StatsAdapter::KEY => Some(&STATS_ADAPTER as &dyn FeatureAdapter),
        HelpAdapter::KEY => Some(&HELP_ADAPTER as &dyn FeatureAdapter),
        ModerationAdapter::KEY => Some(&MODERATION_ADAPTER as &dyn FeatureAdapter),
        AntiScamAdapter::KEY => Some(&ANTI_SCAM_ADAPTER as &dyn FeatureAdapter),
        WelcomeChannelAdapter::KEY => Some(&WELCOME_CHANNEL_ADAPTER as &dyn FeatureAdapter),
        LevelsAdapter::KEY => Some(&LEVELS_ADAPTER as &dyn FeatureAdapter),
        StarboardAdapter::KEY => Some(&STARBOARD_ADAPTER as &dyn FeatureAdapter),
        AntiRaidAdapter::KEY => Some(&ANTI_RAID_ADAPTER as &dyn FeatureAdapter),
        JoinGateAdapter::KEY => Some(&JOIN_GATE_ADAPTER as &dyn FeatureAdapter),
        TicketsAdapter::KEY => Some(&TICKETS_ADAPTER as &dyn FeatureAdapter),
        WelcomeAdapter::KEY => Some(&WELCOME_ADAPTER as &dyn FeatureAdapter),
        "community.suggestions" => Some(&SUGGESTIONS_ADAPTER as &dyn FeatureAdapter),
        "community.giveaways" => Some(&GIVEAWAYS_ADAPTER as &dyn FeatureAdapter),
        "management.polls" => Some(&POLLS_ADAPTER as &dyn FeatureAdapter),
        "community.events" => Some(&EVENTS_ADAPTER as &dyn FeatureAdapter),
        "community.role_panels" => Some(&ROLE_PANELS_ADAPTER as &dyn FeatureAdapter),
        CustomCommandsAdapter::KEY => Some(&CUSTOM_COMMANDS_ADAPTER as &dyn FeatureAdapter),
        AuditAdapter::KEY => Some(&AUDIT_ADAPTER as &dyn FeatureAdapter),
        TemplatesAdapter::KEY => Some(&TEMPLATES_ADAPTER as &dyn FeatureAdapter),
        BirthdaysAdapter::KEY => Some(&BIRTHDAYS_ADAPTER as &dyn FeatureAdapter),
        EmojisAdapter::KEY => Some(&EMOJIS_ADAPTER as &dyn FeatureAdapter),
        AchievementsAdapter::KEY => Some(&ACHIEVEMENTS_ADAPTER as &dyn FeatureAdapter),
        InviteTrackerAdapter::KEY => Some(&INVITE_TRACKER_ADAPTER as &dyn FeatureAdapter),
        "utility.temp_channels" => Some(&TEMP_CHANNELS_ADAPTER as &dyn FeatureAdapter),
        "social.youtube" => Some(&YOUTUBE_ADAPTER as &dyn FeatureAdapter),
        "social.rss" => Some(&RSS_ADAPTER as &dyn FeatureAdapter),
        "social.podcasts" => Some(&PODCASTS_ADAPTER as &dyn FeatureAdapter),
        "social.twitch" => Some(&TWITCH_ADAPTER as &dyn FeatureAdapter),
        "studio.rank_card" => Some(&RANK_CARD_ADAPTER as &dyn FeatureAdapter),
        EmbedsAdapter::KEY => Some(&EMBEDS_ADAPTER as &dyn FeatureAdapter),
        EconomyAdapter::KEY => Some(&ECONOMY_ADAPTER as &dyn FeatureAdapter),
        _ => None,
    }
}

/// Evaluate one message observation without Discord side effects. This is the
/// sole place where anti-spam matching precedence is decided.
pub fn evaluate_anti_spam(
    policy: &AntiSpamPolicy,
    observation: &AntiSpamObservation,
) -> AntiSpamDecision {
    let ignored = policy
        .ignored_channels
        .iter()
        .any(|channel| channel == &observation.channel_id)
        || observation
            .role_ids
            .iter()
            .any(|role| policy.ignored_roles.iter().any(|ignored| ignored == role));
    if ignored {
        return AntiSpamDecision {
            ignored: true,
            matched: Vec::new(),
            should_act: false,
            timeout_seconds: 0,
            reason: "channel_or_role_exempt".into(),
        };
    }
    let mut matched = Vec::new();
    if observation.message_count >= policy.flood_count {
        matched.push("flood".into());
    }
    if observation.duplicate_count >= policy.duplicate_limit {
        matched.push("duplicate".into());
    }
    if observation.mention_count >= policy.mention_limit {
        matched.push("mentions".into());
    }
    let should_act = !matched.is_empty() && !policy.alert_only;
    let reason = if matched.is_empty() {
        "no_match".into()
    } else {
        format!("matched:{}", matched.join(","))
    };
    AntiSpamDecision {
        ignored: false,
        matched,
        should_act,
        timeout_seconds: if should_act {
            policy.timeout_seconds
        } else {
            0
        },
        reason,
    }
}

/// Canonical lifecycle policy for the feature catalogue.  Labels and copy are
/// intentionally kept in the API response for now, but the runtime state is
/// decided here so the panel cannot mark a stored JSON blob as operational.
pub fn feature_maturity(key: &str) -> FeatureMaturity {
    match key {
        // These adapters are wired to the current Rust runtime.
        "protection.antiscam"
        | "protection.anti_raid"
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
        | "support.welcome_channel"
        | "management.polls"
        | "management.nickname"
        | "utility.reminders"
        | "community.leaderboard"
        | "management.workflows"
        | "management.privacy"
        | "insights.stats"
        | "utility.help"
        | "management.moderation"
        | "management.custom_commands"
        | "management.audit"
        | "management.templates"
        | "community.birthdays"
        | "community.achievements"
        | "management.invite_tracker"
        | "utility.emojis"
        | "utility.embeds"
        | "utility.temp_channels"
        | "community.economy"
        | "studio.rank_card" => FeatureMaturity::Operational,
        "social.youtube" | "social.rss" | "social.twitch" => FeatureMaturity::Beta,
        // Podcast feeds reuse the official RSS/Atom transport and its SSRF
        // protection, so they do not require a second provider or secret.
        "social.podcasts" => FeatureMaturity::Operational,
        // Providers without an approved adapter or credentials must never be
        // presented as configurable, even if a legacy setting exists.
        "social.instagram"
        | "social.reddit"
        | "social.x"
        | "social.tiktok"
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

    #[test]
    fn anti_spam_evaluator_explains_matches_and_respects_alert_only() {
        let policy = anti_spam_policy_from_json(&serde_json::json!({
            "floodCount": 5,
            "windowSeconds": 12,
            "duplicateLimit": 2,
            "mentionLimit": 4,
            "timeoutSeconds": 90,
        }));
        let decision = evaluate_anti_spam(
            &policy,
            &AntiSpamObservation {
                channel_id: "general".into(),
                role_ids: vec![],
                message_count: 5,
                duplicate_count: 2,
                mention_count: 4,
            },
        );
        assert_eq!(decision.matched, vec!["flood", "duplicate", "mentions"]);
        assert!(decision.should_act);
        assert_eq!(decision.timeout_seconds, 90);
        assert!(decision.reason.contains("matched:flood,duplicate,mentions"));

        let monitor = AntiSpamPolicy {
            alert_only: true,
            ..policy
        };
        let decision = evaluate_anti_spam(
            &monitor,
            &AntiSpamObservation {
                channel_id: "general".into(),
                role_ids: vec![],
                message_count: 5,
                duplicate_count: 0,
                mention_count: 0,
            },
        );
        assert!(!decision.should_act);
        assert_eq!(decision.timeout_seconds, 0);
        assert_eq!(decision.matched, vec!["flood"]);
    }

    #[test]
    fn anti_spam_exemptions_short_circuit_before_matching() {
        let policy = AntiSpamPolicy {
            ignored_channels: vec!["staff".into()],
            ignored_roles: vec!["trusted".into()],
            ..AntiSpamPolicy::default()
        };
        for (channel_id, role_ids) in [("staff", vec![]), ("general", vec!["trusted".into()])] {
            let decision = evaluate_anti_spam(
                &policy,
                &AntiSpamObservation {
                    channel_id: channel_id.into(),
                    role_ids,
                    message_count: 99,
                    duplicate_count: 99,
                    mention_count: 99,
                },
            );
            assert!(decision.ignored);
            assert!(!decision.should_act);
            assert!(decision.matched.is_empty());
        }
    }

    #[test]
    fn anti_spam_adapter_is_the_single_schema_and_validation_source() {
        let adapter = feature_adapter("protection.antispam").expect("adapter registered");
        let descriptor = adapter.descriptor();
        assert_eq!(descriptor.source, "anti_spam_adapter_v1");
        assert_eq!(descriptor.defaults["floodCount"], 6);
        assert!(descriptor.schema["sections"].as_array().unwrap().len() >= 2);
        let issues = adapter.validate(&serde_json::json!({
            "floodCount": 2,
            "ignoredChannels": [""],
            "logChannel": "not-a-discord-id"
        }));
        assert!(issues.iter().any(|issue| issue.path == "floodCount"));
        assert!(issues.iter().any(|issue| issue.path == "ignoredChannels"));
        assert!(issues.iter().any(|issue| issue.path == "logChannel"));
        let projection = adapter.runtime_projection(&serde_json::json!({
            "floodCount": 8,
            "ignoredChannels": ["123"],
            "alertOnly": true
        }));
        assert!(projection.contains(&("security.antispam.flood_count".into(), "8".into())));
        assert!(projection.contains(&("security.antispam.ignored_channels".into(), "123".into())));
        assert!(projection.contains(&("security.antispam.alert_only".into(), "true".into())));
        assert!(feature_adapter("community.levels").is_some());
    }

    #[test]
    fn existing_discord_modules_have_runtime_owned_panel_contracts() {
        let tickets = feature_adapter("support.tickets").expect("tickets adapter registered");
        assert_eq!(tickets.descriptor().source, "tickets_adapter_v1");
        assert!(
            tickets
                .validate(&serde_json::json!({"closeAfterHours": 0}))
                .iter()
                .any(|issue| issue.path == "closeAfterHours")
        );
        assert!(
            tickets
                .runtime_projection(&serde_json::json!({
                    "staffRole": "123",
                    "transcriptChannel": "456",
                    "closeAfterHours": 2
                }))
                .contains(&("support.ticket.sla_ms".into(), "7200000".into()))
        );

        let welcome = feature_adapter("support.welcome").expect("welcome adapter registered");
        assert_eq!(welcome.descriptor().source, "welcome_adapter_v1");
        assert!(
            welcome
                .runtime_projection(&serde_json::json!({
                    "channel": "123",
                    "message": "Welcome {member}",
                    "sendDm": true,
                    "dmMessage": "Hello {member}",
                    "autoRole": "456"
                }))
                .contains(&("support.welcome.send_dm".into(), "true".into()))
        );

        for key in [
            "community.suggestions",
            "community.giveaways",
            "management.polls",
            "community.events",
            "community.role_panels",
        ] {
            let adapter = feature_adapter(key).expect("toggle-only adapter registered");
            assert!(
                adapter.descriptor().schema["sections"][0]["fields"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            );
            assert!(adapter.validate(&serde_json::json!({})).is_empty());
        }

        let temp = feature_adapter("utility.temp_channels").expect("temporary channels adapter");
        assert_eq!(temp.descriptor().source, "temp_channels_adapter_v2");
        assert!(
            temp.validate(&serde_json::json!({"maxActive": 0}))
                .iter()
                .any(|issue| issue.path == "maxActive")
        );
        assert!(temp
            .runtime_projection(&serde_json::json!({"categoryId": "123", "nameTemplate": "Room {user}", "maxActive": 4}))
            .contains(&("utility.temp_channels.max_active".into(), "4".into())));

        for key in [
            "social.youtube",
            "social.rss",
            "social.podcasts",
            "social.twitch",
        ] {
            let adapter = feature_adapter(key).expect("official provider adapter registered");
            assert!(adapter.descriptor().dependencies.len() >= 2);
        }
    }

    #[test]
    fn custom_commands_audit_and_templates_have_real_contracts() {
        let custom = feature_adapter("management.custom_commands").expect("custom command adapter");
        assert_eq!(custom.descriptor().defaults["maxTags"], 100);
        assert!(
            custom
                .validate(&serde_json::json!({"maxTags": 0}))
                .iter()
                .any(|issue| issue.path == "maxTags")
        );
        assert!(
            custom
                .runtime_projection(&serde_json::json!({"maxTags": 12, "maxResponseLength": 700}))
                .contains(&("management.custom_commands.max_tags".into(), "12".into()))
        );

        let audit = feature_adapter("management.audit").expect("audit adapter");
        assert_eq!(audit.descriptor().defaults["threshold"], 3);
        let audit_projection = audit.runtime_projection(&serde_json::json!({
            "threshold": 5,
            "windowSeconds": 20,
            "shadowMode": true
        }));
        assert!(audit_projection.contains(&("security.anti_nuke.actions".into(), "5".into())));
        assert!(audit_projection.contains(&("security.shadow_mode".into(), "true".into())));

        let templates = feature_adapter("management.templates").expect("templates adapter");
        assert!(templates.validate(&serde_json::json!({})).is_empty());
        assert_eq!(
            feature_maturity("management.templates"),
            FeatureMaturity::Operational
        );

        let birthdays = feature_adapter("community.birthdays").expect("birthdays adapter");
        assert!(
            birthdays
                .runtime_projection(&serde_json::json!({"channel":"123", "message":"Happy {user}"}))
                .contains(&("community.birthdays.channel_id".into(), "123".into()))
        );
        assert!(
            birthdays
                .validate(&serde_json::json!({"message": ""}))
                .iter()
                .any(|issue| issue.path == "message")
        );
        assert_eq!(
            feature_maturity("community.birthdays"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn nickname_adapter_owns_schema_validation_and_projection() {
        let adapter = feature_adapter("management.nickname").expect("adapter registered");
        let descriptor = adapter.descriptor();
        assert_eq!(descriptor.source, "nickname_adapter_v1");
        assert_eq!(descriptor.defaults["nickname"], "");
        assert!(
            adapter
                .validate(&serde_json::json!({"nickname": "A\u{0007}"}))
                .iter()
                .any(|issue| issue.code == "invalid_nickname")
        );
        assert_eq!(
            adapter.runtime_projection(&serde_json::json!({"nickname": "Vozen Helper"})),
            vec![("identity.nickname".into(), "Vozen Helper".into())]
        );
        assert_eq!(
            feature_maturity("management.nickname"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn reminder_adapter_projects_real_runtime_limits() {
        let adapter = feature_adapter("utility.reminders").expect("adapter registered");
        let descriptor = adapter.descriptor();
        assert_eq!(descriptor.source, "reminders_adapter_v1");
        assert_eq!(descriptor.defaults["maxDelayHours"], 168);
        assert!(adapter
            .validate(&serde_json::json!({"maxDelayHours": 0, "maxTextLength": 500, "notifyUser": true}))
            .iter()
            .any(|issue| issue.path == "maxDelayHours"));
        let projection = adapter.runtime_projection(&serde_json::json!({
            "maxDelayHours": 24,
            "maxTextLength": 240,
            "notifyUser": false
        }));
        assert!(projection.contains(&("utility.reminders.max_delay_hours".into(), "24".into())));
        assert!(projection.contains(&("utility.reminders.max_text_length".into(), "240".into())));
        assert!(projection.contains(&("utility.reminders.notify_user".into(), "false".into())));
        assert_eq!(
            feature_maturity("utility.reminders"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn leaderboard_and_workflow_adapters_project_runtime_limits() {
        let leaderboard = feature_adapter("community.leaderboard").expect("leaderboard adapter");
        assert_eq!(leaderboard.descriptor().source, "leaderboard_adapter_v1");
        assert!(
            leaderboard
                .validate(&serde_json::json!({"maxEntries": 101, "public": true}))
                .iter()
                .any(|issue| issue.path == "maxEntries")
        );
        assert!(
            leaderboard
                .runtime_projection(&serde_json::json!({"maxEntries": 25, "public": false}))
                .contains(&("community.leaderboard.max_entries".into(), "25".into()))
        );
        assert_eq!(
            feature_maturity("community.leaderboard"),
            FeatureMaturity::Operational
        );

        let workflows = feature_adapter("management.workflows").expect("workflow adapter");
        assert_eq!(workflows.descriptor().source, "workflows_adapter_v1");
        assert!(workflows
            .validate(&serde_json::json!({"maxWorkflows": 10, "maxReplyLength": 0, "allowMentions": false}))
            .iter()
            .any(|issue| issue.path == "maxReplyLength"));
        assert!(
            workflows
                .runtime_projection(&serde_json::json!({
                    "maxWorkflows": 20,
                    "maxReplyLength": 750,
                    "allowMentions": true
                }))
                .contains(&("management.workflows.allow_mentions".into(), "true".into()))
        );
        assert_eq!(
            feature_maturity("management.workflows"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn moderation_adapter_projects_guardrails_used_by_commands() {
        let adapter = feature_adapter("management.moderation").expect("moderation adapter");
        assert_eq!(adapter.descriptor().source, "moderation_adapter_v1");
        assert!(
            adapter
                .validate(&serde_json::json!({"requireReason": true, "maxPurge": 0}))
                .iter()
                .any(|issue| issue.path == "maxPurge")
        );
        let projection = adapter.runtime_projection(&serde_json::json!({
            "requireReason": false,
            "maxPurge": 42
        }));
        assert!(projection.contains(&(
            "management.moderation.require_reason".into(),
            "false".into()
        )));
        assert!(projection.contains(&("management.moderation.max_purge".into(), "42".into())));
        assert_eq!(
            feature_maturity("management.moderation"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn scam_policy_is_bounded_and_explainable() {
        let adapter = feature_adapter("protection.antiscam").expect("anti-scam adapter");
        assert_eq!(adapter.descriptor().source, "anti_scam_adapter_v1");
        assert!(
            adapter
                .validate(&serde_json::json!({"blockedDomains": ["bad/domain"]}))
                .iter()
                .any(|issue| issue.path == "blockedDomains")
        );
        let policy = scam_policy_from_json(&serde_json::json!({
            "blockInvites": true,
            "blockedDomains": ["example.invalid"],
            "blockedKeywords": ["free coins"],
            "timeoutSeconds": 60
        }));
        let decision = evaluate_scam(
            &policy,
            "123",
            "Claim your free coins at https://example.invalid now",
        );
        assert!(decision.should_act);
        assert!(
            decision
                .matched
                .iter()
                .any(|value| value.starts_with("domain:"))
        );
        assert_eq!(decision.timeout_seconds, 60);
        assert!(evaluate_scam(&policy, "123", "https://discord.gg/example").should_act);
        assert!(evaluate_scam(&policy, "123", "hello").matched.is_empty());
    }

    #[test]
    fn welcome_channel_adapter_projects_a_real_member_join_message() {
        let adapter = feature_adapter("support.welcome_channel").expect("welcome adapter");
        assert_eq!(adapter.descriptor().source, "welcome_channel_adapter_v1");
        assert!(
            adapter
                .validate(&serde_json::json!({"channelId": "not-an-id", "message": "Hi"}))
                .iter()
                .any(|issue| issue.path == "channelId")
        );
        assert!(
            adapter
                .runtime_projection(&serde_json::json!({
                    "channelId": "123",
                    "message": "Welcome {member}!"
                }))
                .contains(&("support.welcome_channel.channel_id".into(), "123".into()))
        );
        assert_eq!(
            feature_maturity("support.welcome_channel"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn levels_and_starboard_adapters_cover_runtime_settings() {
        let levels = feature_adapter("community.levels").expect("levels adapter");
        assert_eq!(levels.descriptor().source, "levels_adapter_v1");
        assert!(
            levels
                .validate(&serde_json::json!({"xpMin": 40, "xpMax": 20}))
                .iter()
                .any(|issue| issue.path == "xpMax")
        );
        assert!(
            levels
                .runtime_projection(&serde_json::json!({"xpMin": 10, "xpMax": 25}))
                .contains(&("community.levels.xp_max".into(), "25".into()))
        );
        assert!(
            levels
                .validate(&serde_json::json!({"voiceXpEnabled": true, "voiceXpPerMinute": 31}))
                .iter()
                .any(|issue| issue.path == "voiceXpPerMinute")
        );
        assert!(
            levels
                .runtime_projection(&serde_json::json!({
                    "voiceXpEnabled": true,
                    "voiceXpPerMinute": 4
                }))
                .contains(&("community.levels.voice_xp_enabled".into(), "true".into()))
        );
        assert!(
            levels
                .runtime_projection(&serde_json::json!({
                    "voiceXpEnabled": true,
                    "voiceXpPerMinute": 4
                }))
                .contains(&("community.levels.voice_xp_per_minute".into(), "4".into()))
        );
        assert!(
            levels
                .validate(&serde_json::json!({"levelRoles": ["5=123"]}))
                .is_empty()
        );
        assert!(
            levels
                .runtime_projection(&serde_json::json!({
                    "ignoredChannels": ["456"],
                    "levelRoles": ["5=123"],
                    "stackRoles": false
                }))
                .contains(&("community.levels.ignored_channels".into(), "456".into()))
        );

        let starboard = feature_adapter("community.starboard").expect("starboard adapter");
        assert_eq!(starboard.descriptor().source, "starboard_adapter_v1");
        assert!(
            starboard
                .validate(&serde_json::json!({"threshold": 0}))
                .iter()
                .any(|issue| issue.path == "threshold")
        );
        assert!(
            starboard
                .runtime_projection(&serde_json::json!({
                    "channel": "123",
                    "threshold": 5,
                    "emoji": "⭐"
                }))
                .contains(&("community.starboard.threshold".into(), "5".into()))
        );
    }

    #[test]
    fn join_protection_adapters_project_real_gateway_limits() {
        let raid = feature_adapter("protection.anti_raid").expect("anti-raid adapter");
        assert_eq!(raid.descriptor().source, "anti_raid_adapter_v1");
        assert!(
            raid.runtime_projection(&serde_json::json!({
                "joinThreshold": 20,
                "windowSeconds": 15,
                "incidentMinutes": 30,
                "verification": "very_high",
                "alertOnly": true
            }))
            .contains(&("security.anti_raid.joins".into(), "20".into()))
        );
        assert!(
            raid.runtime_projection(
                &serde_json::json!({"incidentMinutes": 30, "verification": "very_high"})
            )
            .contains(&("security.anti_raid.incident_minutes".into(), "30".into()))
        );
        assert!(
            raid.runtime_projection(
                &serde_json::json!({"incidentMinutes": 30, "verification": "very_high"})
            )
            .contains(&("security.anti_raid.verification".into(), "very_high".into()))
        );
        let gate = feature_adapter("protection.join_gate").expect("join gate adapter");
        assert_eq!(gate.descriptor().source, "join_gate_adapter_v1");
        assert!(
            gate.validate(&serde_json::json!({"minimumAccountDays": 400}))
                .iter()
                .any(|issue| issue.path == "minimumAccountDays")
        );
        assert!(
            gate.runtime_projection(&serde_json::json!({
                "minimumAccountDays": 7,
                "verifiedRole": "123",
                "requireAvatar": true,
                "action": "quarantine",
                "autoRole": "456",
                "blockedNamePatterns": ["spam"],
                "logChannel": "789"
            }))
            .contains(&("security.join_gate.role_id".into(), "123".into()))
        );
        assert!(gate.runtime_projection(&serde_json::json!({"requireAvatar": true, "action": "alert", "autoRole": "456", "blockedNamePatterns": ["spam"], "logChannel": "789"})).contains(&("security.join_gate.require_avatar".into(), "true".into())));
    }

    #[test]
    fn privacy_stats_and_help_adapters_expose_bounded_runtime_options() {
        let privacy = feature_adapter("management.privacy").expect("privacy adapter");
        assert_eq!(privacy.descriptor().source, "privacy_adapter_v1");
        assert!(privacy
            .validate(&serde_json::json!({"maxExportBytes": 1, "allowMemberExport": true, "allowMemberErase": true}))
            .iter()
            .any(|issue| issue.path == "maxExportBytes"));
        assert!(
            privacy
                .runtime_projection(&serde_json::json!({
                    "maxExportBytes": 500_000,
                    "allowMemberExport": false,
                    "allowMemberErase": true
                }))
                .contains(&(
                    "management.privacy.allow_member_export".into(),
                    "false".into()
                ))
        );

        let stats = feature_adapter("insights.stats").expect("stats adapter");
        assert_eq!(stats.descriptor().source, "stats_adapter_v1");
        assert!(
            stats
                .validate(&serde_json::json!({"windowDays": 31, "public": false}))
                .iter()
                .any(|issue| issue.path == "windowDays")
        );
        assert_eq!(
            feature_maturity("insights.stats"),
            FeatureMaturity::Operational
        );

        let help = feature_adapter("utility.help").expect("help adapter");
        assert_eq!(help.descriptor().source, "help_adapter_v1");
        assert!(
            help.validate(&serde_json::json!({"showModules": "yes", "showDashboard": true}))
                .iter()
                .any(|issue| issue.path == "showModules")
        );
        assert_eq!(
            feature_maturity("utility.help"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn achievements_invites_and_emojis_adapters_project_real_command_settings() {
        let achievements = feature_adapter("community.achievements").expect("achievements adapter");
        assert_eq!(achievements.descriptor().source, "achievements_adapter_v2");
        assert!(achievements
            .validate(&serde_json::json!({"firstThreshold": 1000, "regularThreshold": 100, "pillarThreshold": 10000}))
            .iter()
            .any(|issue| issue.code == "ordered_required"));
        assert!(achievements
            .runtime_projection(&serde_json::json!({"firstThreshold": 50, "regularThreshold": 500, "pillarThreshold": 5000}))
            .contains(&("community.achievements.regular_threshold".into(), "500".into())));

        let invites = feature_adapter("management.invite_tracker").expect("invite tracker adapter");
        assert_eq!(invites.descriptor().source, "invite_tracker_adapter_v2");
        assert!(
            invites
                .runtime_projection(&serde_json::json!({"maxEntries": 25, "includeInviter": false}))
                .contains(&(
                    "management.invite_tracker.include_inviter".into(),
                    "false".into()
                ))
        );

        let emojis = feature_adapter("utility.emojis").expect("emojis adapter");
        assert_eq!(emojis.descriptor().source, "emojis_adapter_v2");
        assert!(
            emojis
                .runtime_projection(&serde_json::json!({"maxEntries": 8, "animatedOnly": true}))
                .contains(&("utility.emojis.animated_only".into(), "true".into()))
        );
    }
}
