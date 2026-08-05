//! Pure configuration and policy primitives. No Discord or HTTP side effects.

use anyhow::{Context, Result};
use helper_contracts::{
    AntiSpamDecision, AntiSpamObservation, AntiSpamPolicy, FeatureAdapterDescriptor,
    FeatureMaturity, Plan, RANK_CARD_BACKGROUND_PRESETS, RankCardConfig, ValidationIssue,
};
use serde::Deserialize;
use std::{collections::HashSet, env, net::IpAddr, path::PathBuf, str::FromStr};

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

/// Convert the bounded fixture accepted by the API into the observation used
/// by the Discord gateway.  Both camelCase (panel payloads) and snake_case
/// (Rust fixtures) are accepted so the simulation contract remains stable
/// across clients without duplicating the decision logic.
pub fn anti_spam_observation_from_json(value: &serde_json::Value) -> AntiSpamObservation {
    let object = value.as_object();
    let string = |camel: &str, snake: &str, fallback: &str| {
        object
            .and_then(|values| values.get(camel).or_else(|| values.get(snake)))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    let number = |camel: &str, snake: &str, fallback: u32| {
        object
            .and_then(|values| values.get(camel).or_else(|| values.get(snake)))
            .and_then(serde_json::Value::as_u64)
            .map(|value| value.min(u32::MAX as u64) as u32)
            .unwrap_or(fallback)
    };
    let roles = object
        .and_then(|values| values.get("roleIds").or_else(|| values.get("role_ids")))
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .take(100)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    AntiSpamObservation {
        channel_id: string("channelId", "channel_id", "preview-channel"),
        role_ids: roles,
        message_count: number("messageCount", "message_count", 6),
        duplicate_count: number("duplicateCount", "duplicate_count", 3),
        mention_count: number("mentionCount", "mention_count", 5),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScamPolicy {
    pub block_invites: bool,
    pub blocked_domains: Vec<String>,
    pub blocked_keywords: Vec<String>,
    pub ignored_channels: Vec<String>,
    pub ignored_roles: Vec<String>,
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
            ignored_roles: Vec::new(),
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
    if let Some(value) = object
        .get("ignoredRoles")
        .and_then(serde_json::Value::as_array)
    {
        policy.ignored_roles = value
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|role| !role.is_empty())
            .take(100)
            .map(str::to_owned)
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
    evaluate_scam_with_roles(policy, channel_id, &[], content)
}

/// Evaluate scam policy with the member's roles. Keeping this pure makes the
/// API simulator and the Discord gateway share exactly the same exemptions
/// and matching rules.
pub fn evaluate_scam_with_roles(
    policy: &ScamPolicy,
    channel_id: &str,
    role_ids: &[String],
    content: &str,
) -> ScamDecision {
    if policy.ignored_channels.iter().any(|id| id == channel_id)
        || role_ids
            .iter()
            .any(|role| policy.ignored_roles.iter().any(|ignored| ignored == role))
    {
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
    if policy.block_invites && contains_invite_link(&lower) {
        matched.push("discord_invite".into());
    }
    for domain in &policy.blocked_domains {
        if contains_blocked_domain(&lower, domain) {
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

fn contains_invite_link(content: &str) -> bool {
    content.split_whitespace().any(|token| {
        let host = normalize_host(token);
        (host == "discord.gg" && token.contains("discord.gg/"))
            || (host == "discord.com" && token.contains("discord.com/invite/"))
    })
}

/// Match a blocked host without treating `example.com.evil.test` as
/// `example.com`. Subdomains of a blocked domain remain blocked. Inputs are
/// intentionally ASCII-bounded so look-alike Unicode hosts cannot silently
/// bypass an administrator's domain list.
fn contains_blocked_domain(content: &str, configured: &str) -> bool {
    let domain = normalize_host(configured);
    if domain.is_empty() || !domain.contains('.') {
        return false;
    }
    content.split_whitespace().any(|token| {
        let host = normalize_host(token);
        !host.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
    })
}

fn normalize_host(value: &str) -> String {
    let trimmed = value
        .trim()
        .trim_matches(|character: char| "<>[](){}\"'`,;!?".contains(character));
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_www = without_scheme
        .strip_prefix("www.")
        .unwrap_or(without_scheme);
    let host = without_www
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty()
        || !host.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '.'
        })
    {
        return String::new();
    }
    host
}

/// Bounded anti-raid policy shared by the API simulator and the Discord
/// member-join handler.  The gateway is responsible for counting joins in a
/// time window; this pure evaluator decides what that count means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiRaidPolicy {
    pub join_threshold: u32,
    pub window_seconds: u64,
    pub incident_minutes: u64,
    pub verification: String,
    pub alert_only: bool,
    /// When enabled, an armed incident temporarily enables the join gate.
    /// Discord does not expose a safe global "pause invites" switch; the
    /// gate is the bounded equivalent that can be restored after expiry.
    pub pause_invites: bool,
}

impl Default for AntiRaidPolicy {
    fn default() -> Self {
        Self {
            join_threshold: 10,
            window_seconds: 10,
            incident_minutes: 10,
            verification: "high".into(),
            alert_only: false,
            pause_invites: true,
        }
    }
}

pub fn anti_raid_policy_from_json(value: &serde_json::Value) -> AntiRaidPolicy {
    let mut policy = AntiRaidPolicy::default();
    let Some(object) = value.as_object() else {
        return policy;
    };
    if let Some(value) = object
        .get("joinThreshold")
        .and_then(serde_json::Value::as_u64)
    {
        policy.join_threshold = value.clamp(2, 100) as u32;
    }
    if let Some(value) = object
        .get("windowSeconds")
        .and_then(serde_json::Value::as_u64)
    {
        policy.window_seconds = value.clamp(3, 60);
    }
    if let Some(value) = object
        .get("incidentMinutes")
        .and_then(serde_json::Value::as_u64)
    {
        policy.incident_minutes = value.clamp(1, 120);
    }
    if let Some(value) = object
        .get("verification")
        .and_then(serde_json::Value::as_str)
        && matches!(value, "medium" | "high" | "very_high")
    {
        policy.verification = value.into();
    }
    if let Some(value) = object.get("alertOnly").and_then(serde_json::Value::as_bool) {
        policy.alert_only = value;
    }
    if let Some(value) = object
        .get("pauseInvites")
        .and_then(serde_json::Value::as_bool)
    {
        policy.pause_invites = value;
    }
    policy
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntiRaidDecision {
    pub joins: u32,
    pub armed: bool,
    pub shadow_mode: bool,
    pub should_contain: bool,
    pub incident_minutes: u64,
    pub reason: String,
}

pub fn evaluate_anti_raid(
    policy: &AntiRaidPolicy,
    joins: u32,
    shadow_mode: bool,
) -> AntiRaidDecision {
    let joins = joins.min(100_000);
    let armed = joins >= policy.join_threshold;
    let shadow_mode = shadow_mode || policy.alert_only;
    let should_contain = armed && !shadow_mode && policy.pause_invites;
    AntiRaidDecision {
        joins,
        armed,
        shadow_mode,
        should_contain,
        incident_minutes: policy.incident_minutes,
        reason: if armed {
            if shadow_mode {
                "join_burst_detected_shadow".into()
            } else if policy.pause_invites {
                "join_burst_detected_contain".into()
            } else {
                "join_burst_detected_alert".into()
            }
        } else {
            "join_burst_below_threshold".into()
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinGatePolicy {
    pub minimum_account_days: i64,
    pub require_avatar: bool,
    pub blocked_name_patterns: Vec<String>,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinGateObservation {
    pub account_age_days: i64,
    pub has_avatar: bool,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinGateDecision {
    pub blocked: bool,
    pub reasons: Vec<String>,
    pub action: String,
}

/// Decide whether a member needs verification.  Name matching is deliberately
/// case-insensitive and bounded by the adapter's validation limits.
pub fn evaluate_join_gate(
    policy: &JoinGatePolicy,
    observation: &JoinGateObservation,
) -> JoinGateDecision {
    let account_age_days = observation.account_age_days.clamp(0, 3650);
    let minimum_account_days = policy.minimum_account_days.clamp(0, 365);
    let display_name = observation.display_name.to_ascii_lowercase();
    let mut reasons = Vec::new();
    if account_age_days < minimum_account_days {
        reasons.push(format!(
            "account age {account_age_days}d < {minimum_account_days}d"
        ));
    }
    if policy.require_avatar && !observation.has_avatar {
        reasons.push("profile avatar is required".into());
    }
    if let Some(pattern) = policy
        .blocked_name_patterns
        .iter()
        .map(|pattern| pattern.trim().to_ascii_lowercase())
        .filter(|pattern| !pattern.is_empty())
        .take(20)
        .find(|pattern| display_name.contains(pattern))
    {
        reasons.push(format!("display name matches `{pattern}`"));
    }
    JoinGateDecision {
        blocked: !reasons.is_empty(),
        reasons,
        action: if policy.action == "alert" {
            "alert".into()
        } else {
            "quarantine".into()
        },
    }
}

/// Configuration and bounded input for the XP leaderboard.  Keeping this
/// evaluator in core means the API preview and the Discord command cannot
/// disagree about visibility, opt-outs or ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderboardPolicy {
    pub max_entries: u32,
    pub public: bool,
}

impl Default for LeaderboardPolicy {
    fn default() -> Self {
        Self {
            max_entries: 10,
            public: true,
        }
    }
}

pub fn leaderboard_policy_from_json(value: &serde_json::Value) -> LeaderboardPolicy {
    let mut policy = LeaderboardPolicy::default();
    let Some(object) = value.as_object() else {
        return policy;
    };
    if let Some(value) = object.get("maxEntries").and_then(serde_json::Value::as_u64) {
        policy.max_entries = value.clamp(1, 100) as u32;
    }
    if let Some(value) = object.get("public").and_then(serde_json::Value::as_bool) {
        policy.public = value;
    }
    policy
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LeaderboardEntry {
    #[serde(rename = "userId", alias = "user_id")]
    pub user_id: String,
    pub xp: i64,
    #[serde(default, rename = "optedOut", alias = "opted_out")]
    pub opted_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderboardDecision {
    pub public: bool,
    pub entries: Vec<LeaderboardEntry>,
    pub excluded_opt_outs: u32,
    pub truncated: bool,
}

/// Sort and bound leaderboard rows while excluding members who opted out.
/// IDs and XP are clamped so a malformed preview or legacy row cannot create
/// an unbounded response.  The same function is used by the gateway and API.
pub fn evaluate_leaderboard(
    policy: &LeaderboardPolicy,
    entries: impl IntoIterator<Item = LeaderboardEntry>,
) -> LeaderboardDecision {
    let mut excluded_opt_outs = 0;
    let mut entries: Vec<_> = entries
        .into_iter()
        .filter_map(|mut entry| {
            entry.user_id = entry.user_id.trim().chars().take(64).collect();
            if entry.user_id.is_empty() {
                return None;
            }
            entry.xp = entry.xp.clamp(0, i64::MAX);
            if entry.opted_out {
                excluded_opt_outs += 1;
                None
            } else {
                Some(entry)
            }
        })
        .take(1_000)
        .collect();
    entries.sort_by(|left, right| {
        right
            .xp
            .cmp(&left.xp)
            .then_with(|| left.user_id.cmp(&right.user_id))
    });
    let max_entries = policy.max_entries.clamp(1, 100) as usize;
    let truncated = entries.len() > max_entries;
    entries.truncate(max_entries);
    LeaderboardDecision {
        public: policy.public,
        entries,
        excluded_opt_outs,
        truncated,
    }
}

/// Parse the bounded `leaderboardEntries` fixture accepted by the simulation
/// endpoint. Invalid rows are ignored just like rows without a Discord user
/// in the runtime store.
pub fn leaderboard_entries_from_json(value: &serde_json::Value) -> Vec<LeaderboardEntry> {
    value
        .get("leaderboardEntries")
        .or_else(|| value.get("leaderboard_entries"))
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| serde_json::from_value::<LeaderboardEntry>(row.clone()).ok())
                .take(1_000)
                .collect()
        })
        .unwrap_or_default()
}

/// Bounded policy shared by workflow simulation and the Discord message
/// handler.  Keeping this in the pure core crate prevents the panel from
/// previewing a different reply than the bot actually sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPolicy {
    pub max_reply_length: usize,
    pub allow_mentions: bool,
}

impl Default for WorkflowPolicy {
    fn default() -> Self {
        Self {
            max_reply_length: 1_000,
            allow_mentions: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowObservation {
    pub enabled: bool,
    pub trigger: String,
    pub condition: String,
    pub action: String,
    pub payload: String,
    pub message_content: String,
    pub user_mention: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDecision {
    pub matched: bool,
    pub should_run: bool,
    pub reply: Option<String>,
    pub reason: String,
}

/// Evaluate one bounded message workflow.  Only the existing allow-listed
/// `message` + `reply` path is executable; unsupported actions are reported
/// as a non-match rather than silently being treated as successful.
pub fn evaluate_workflow(
    policy: &WorkflowPolicy,
    observation: &WorkflowObservation,
) -> WorkflowDecision {
    let content = observation.message_content.trim();
    if !observation.enabled {
        return WorkflowDecision {
            matched: false,
            should_run: false,
            reply: None,
            reason: "disabled".into(),
        };
    }
    if observation.trigger != "message" {
        return WorkflowDecision {
            matched: false,
            should_run: false,
            reply: None,
            reason: "unsupported_trigger".into(),
        };
    }
    if !observation.condition.trim().is_empty()
        && !content
            .to_lowercase()
            .contains(&observation.condition.to_lowercase())
    {
        return WorkflowDecision {
            matched: false,
            should_run: false,
            reply: None,
            reason: "condition_not_met".into(),
        };
    }
    if observation.action != "reply" {
        return WorkflowDecision {
            matched: true,
            should_run: false,
            reply: None,
            reason: "unsupported_action".into(),
        };
    }
    let mut reply = observation
        .payload
        .replace("{user}", &observation.user_mention)
        .replace("{message}", &truncate_chars(content, 500));
    if !policy.allow_mentions {
        reply = reply
            .replace("@everyone", "@\u{200b}everyone")
            .replace("@here", "@\u{200b}here");
    }
    let reply = truncate_chars(&reply, policy.max_reply_length.clamp(1, 1_500));
    WorkflowDecision {
        matched: true,
        should_run: true,
        reply: Some(reply),
        reason: "matched".into(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Render the bounded member/server variables used by welcome and farewell
/// messages.  Legacy settings pass through this function as well, so a value
/// loaded from the compatibility projection cannot re-enable mass mentions or
/// exceed Discord's message limit.
pub fn render_member_message(template: &str, member: &str, server: &str) -> String {
    let rendered = template
        .replace("{member}", member)
        .replace("{server}", server)
        .replace("@everyone", "@\u{200b}everyone")
        .replace("@here", "@\u{200b}here");
    truncate_chars(
        &rendered
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>(),
        2_000,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementPolicy {
    pub first_threshold: i64,
    pub regular_threshold: i64,
    pub pillar_threshold: i64,
}

impl Default for AchievementPolicy {
    fn default() -> Self {
        Self {
            first_threshold: 100,
            regular_threshold: 1_000,
            pillar_threshold: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementUnlock {
    pub key: &'static str,
    pub label: &'static str,
    pub threshold: i64,
}

/// Return the milestones reached by the supplied XP balance.  The runtime
/// persists each returned key idempotently; the API uses the same pure result
/// for its preview.
pub fn evaluate_achievements(policy: &AchievementPolicy, xp: i64) -> Vec<AchievementUnlock> {
    let xp = xp.max(0);
    [
        (
            "first_steps",
            "First steps",
            policy.first_threshold.clamp(1, 1_000_000),
        ),
        (
            "regular",
            "Regular",
            policy.regular_threshold.clamp(1, 1_000_000),
        ),
        (
            "community_pillar",
            "Community pillar",
            policy.pillar_threshold.clamp(1, 1_000_000),
        ),
    ]
    .into_iter()
    .filter(|(_, _, threshold)| xp >= *threshold)
    .map(|(key, label, threshold)| AchievementUnlock {
        key,
        label,
        threshold,
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationPolicy {
    pub require_reason: bool,
    pub max_purge: u64,
}

impl Default for ModerationPolicy {
    fn default() -> Self {
        Self {
            require_reason: true,
            max_purge: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationObservation {
    pub action: String,
    pub reason: String,
    pub requested_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationDecision {
    pub allowed: bool,
    pub effective_count: Option<u64>,
    pub reason_code: &'static str,
    pub explanation: String,
}

/// Evaluate the bounded safety envelope used by manual moderation commands.
/// This function has no Discord or store dependency, so API simulations and
/// production command handlers cannot drift apart on reasons or purge limits.
pub fn evaluate_moderation(
    policy: &ModerationPolicy,
    observation: &ModerationObservation,
) -> ModerationDecision {
    let action = observation.action.trim().to_ascii_lowercase();
    let supported = [
        "warn", "kick", "ban", "timeout", "tempban", "softban", "purge",
    ];
    if !supported.contains(&action.as_str()) {
        return ModerationDecision {
            allowed: false,
            effective_count: None,
            reason_code: "unsupported_action",
            explanation: format!("The moderation action `{action}` is not supported."),
        };
    }

    if action == "purge" {
        let Some(requested) = observation.requested_count else {
            return ModerationDecision {
                allowed: false,
                effective_count: None,
                reason_code: "count_required",
                explanation: "Choose how many messages to purge.".into(),
            };
        };
        if requested <= 0 {
            return ModerationDecision {
                allowed: false,
                effective_count: None,
                reason_code: "invalid_count",
                explanation: "The purge count must be at least 1.".into(),
            };
        }
        let maximum = policy.max_purge.clamp(1, 100);
        let effective = (requested as u64).min(maximum);
        let explanation = if requested as u64 > maximum {
            format!("Purge is limited to {maximum} messages by this server's policy.")
        } else {
            format!("Purge up to {effective} message(s).")
        };
        return ModerationDecision {
            allowed: true,
            effective_count: Some(effective),
            reason_code: if requested as u64 > maximum {
                "count_clamped"
            } else {
                "allowed"
            },
            explanation,
        };
    }

    if policy.require_reason && observation.reason.trim().is_empty() {
        return ModerationDecision {
            allowed: false,
            effective_count: None,
            reason_code: "reason_required",
            explanation: "Provide a reason so the action can be audited.".into(),
        };
    }

    ModerationDecision {
        allowed: true,
        effective_count: None,
        reason_code: "allowed",
        explanation: format!("The `{action}` action is allowed by the server policy."),
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

    /// Produce a bounded, side-effect-free preview from the same projection
    /// that is published to the runtime.  The helper below is deliberately
    /// keyed by the adapter descriptor rather than by an HTTP route, so every
    /// feature gets a meaningful runtime action even when its configuration is
    /// currently just a feature gate.
    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let descriptor = self.descriptor();
        let projection = self.runtime_projection(config);
        simulate_feature_effect(&descriptor.key, config, fixture, &projection)
    }
}

fn fixture_string<'a>(
    fixture: &'a serde_json::Value,
    camel: &str,
    snake: &str,
    fallback: &'a str,
) -> &'a str {
    fixture
        .get(camel)
        .or_else(|| fixture.get(snake))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
}

fn fixture_u64(fixture: &serde_json::Value, camel: &str, snake: &str, fallback: u64) -> u64 {
    fixture
        .get(camel)
        .or_else(|| fixture.get(snake))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(fallback)
}

fn fixture_bool(fixture: &serde_json::Value, camel: &str, snake: &str, fallback: bool) -> bool {
    fixture
        .get(camel)
        .or_else(|| fixture.get(snake))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(fallback)
}

fn fixture_strings(fixture: &serde_json::Value, camel: &str, snake: &str) -> Vec<String> {
    fixture
        .get(camel)
        .or_else(|| fixture.get(snake))
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .take(100)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Describe the observable operation represented by a feature publication.
/// This is intentionally pure and bounded: it never calls Discord or an
/// external provider.  The Discord handlers consume the same projected
/// setting keys and the API uses this function for its preview, so the panel
/// cannot claim that it only "saved JSON".
fn simulate_feature_effect(
    key: &str,
    config: &serde_json::Value,
    fixture: &serde_json::Value,
    projection: &[(String, String)],
) -> Vec<String> {
    let channel = fixture
        .get("channelId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("the selected channel");
    let content = fixture
        .get("content")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("the preview event");
    let object = config.as_object();
    let effect = match key {
        "community.leaderboard" => format!(
            "Read the XP ledger and publish a {} leaderboard for {channel}.",
            if object
                .and_then(|values| values.get("public"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "public"
            } else {
                "private"
            }
        ),
        "community.achievements" =>
            "Evaluate the member's XP/message milestones and grant each eligible reward once.".into(),
        "community.birthdays" =>
            "Run the timezone-aware birthday job and send the configured message without exposing a birth year.".into(),
        "management.nickname" =>
            "Apply the Helper nickname after a fresh Manage Nicknames and hierarchy preflight.".into(),
        "management.moderation" =>
            "Route the moderation action through policy, hierarchy checks, an idempotency key and an audit record.".into(),
        "management.audit" =>
            "Route the selected Discord events to the configured log destination and enforce retention.".into(),
        "management.privacy" =>
            "Create a guild-scoped export or erasure receipt while preserving required moderation evidence.".into(),
        "management.templates" =>
            "Render the selected template with approved variables and Discord size/mention limits before publishing.".into(),
        "management.custom_commands" =>
            "Resolve the saved response with bounded variables; user-provided code is never executed.".into(),
        "utility.help" =>
            "Return the enabled modules, command examples and the dashboard link for this server.".into(),
        "utility.reminders" =>
            "Create an idempotent reminder job with the configured timezone, recurrence and delivery channel.".into(),
        "utility.emojis" =>
            "Read the server emoji inventory, apply the configured filters and return a bounded result set.".into(),
        "utility.embeds" =>
            "Render the bounded embed preview with mentions disabled, then publish only after preflight.".into(),
        "utility.search" =>
            "Query only the enabled approved providers and return a rate-limited result set; arbitrary URLs are not fetched.".into(),
        "utility.temp_channels" => {
            let template = object
                .and_then(|values| values.get("nameTemplate"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("{user}'s room");
            let maximum = object
                .and_then(|values| values.get("maxActive"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(10);
            format!("Create a temporary voice room named `{template}` (up to {maximum} active rooms) and clean it up after a full disconnect.")
        }
        "insights.stats" =>
            "Update the configured statistics channel from the latest guild counters and expose freshness/health.".into(),
        "community.economy" =>
            "Append an idempotent economy ledger entry and derive the member balance from the ledger.".into(),
        "community.role_panels" =>
            "Publish bounded role-panel components after checking every role's manageability and hierarchy.".into(),
        "community.events" =>
            "Create or update the scheduled event, persist registrations and enqueue the configured reminders.".into(),
        "community.suggestions" =>
            "Create the suggestion, apply the voting policy and keep every state transition auditable.".into(),
        "community.giveaways" =>
            "Create an idempotent giveaway job with bounded winners, role requirements and a replay-safe draw.".into(),
        "management.polls" =>
            "Publish the poll interaction and persist each vote with duplicate protection and a closing job.".into(),
        "support.tickets" =>
            "Create a private ticket channel with the configured team overwrites, claim/SLA state and transcript policy.".into(),
        "support.welcome" =>
            "Send the welcome message/DM and apply the configured auto-role on member join, with a safe fallback channel.".into(),
        "support.welcome_channel" =>
            "Publish the guided welcome panel with rules, first steps and bounded confirmation components.".into(),
        "studio.rank_card" =>
            "Render the XP card using only curated backgrounds, colours and the member's current XP snapshot.".into(),
        "social.youtube" | "social.rss" | "social.podcasts" | "social.twitch" | "social.bluesky"
        | "social.reddit" | "social.instagram" | "social.x" | "social.tiktok" | "social.kick" =>
            "Validate the subscription, enqueue a deduplicated provider job and deliver the rendered alert to the selected channel.".into(),
        "web3.crypto_stats" | "web3.crypto_queries" | "web3.gas_tracker" | "web3.nft_stats"
        | "web3.nft_queries" | "web3.nft_sales" =>
            "Query the configured read-only provider with bounded caching, freshness metadata and rate-limit handling.".into(),
        "growth.monetization" =>
            "Create or update the server's Connect capability only after onboarding, webhook and compliance checks.".into(),
        "web3.gating" =>
            "Verify a single-use SIWE nonce and reconcile only the configured ERC-20/721/1155 role rules.".into(),
        "protection.antispam" | "protection.antiscam" | "protection.anti_raid" | "protection.join_gate" =>
            format!("Evaluate `{content}` against the configured protection policy, then apply only the allow-listed action after preflight."),
        _ if projection.is_empty() =>
            "Enable the feature's Discord event/command handler and record the publication in the audit stream.".into(),
        _ => format!(
            "Publish the validated settings ({}) to the feature's Discord runtime consumer and record an audit event.",
            projection.len()
        ),
    };
    let mut effects = vec![effect];
    // Keep the exact projected keys visible in the preview.  This is useful
    // for operators and, more importantly, proves that the effect is tied to
    // the same persisted values consumed by the runtime.
    effects.extend(
        projection
            .iter()
            .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
    );
    effects
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

/// Adapter for provider subscriptions whose persistence and workers live in
/// the API/modules crates.  The subscription editor still belongs to the
/// canonical feature registry so production never falls back to a blank
/// toggle-only form.
#[derive(Debug, Clone, Copy)]
pub struct AlertSubscriptionAdapter {
    pub key: &'static str,
    pub source: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub source_key: &'static str,
    pub source_label: &'static str,
    pub source_help: &'static str,
    pub default_template: &'static str,
    pub dependencies: &'static [&'static str],
}

impl FeatureAdapter for AlertSubscriptionAdapter {
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
                        {"key": self.source_key, "label": self.source_label, "kind": "text", "help": self.source_help},
                        {"key": "targetChannelId", "label": "Discord channel", "kind": "channel"},
                        {"key": "intervalSeconds", "label": "Polling interval (seconds)", "kind": "number", "min": 900, "max": 86400},
                        {"key": "messageTemplate", "label": "Alert message", "kind": "textarea", "advanced": true},
                        {"key": "mention", "label": "Optional mention", "kind": "text", "advanced": true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                self.source_key: "",
                "targetChannelId": "",
                "intervalSeconds": 900,
                "messageTemplate": self.default_template,
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
                message: "Alert configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        let source = object
            .get(self.source_key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if source.is_empty() || source.chars().count() > 100 {
            issues.push(ValidationIssue {
                path: self.source_key.into(),
                code: "required".into(),
                message: format!(
                    "{} must be provided and at most 100 characters.",
                    self.source_label
                ),
                severity: "error".into(),
            });
        }
        let channel = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if channel.is_empty() || channel.parse::<u64>().is_err() {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "invalid_discord_id".into(),
                message: "Choose a real Discord channel for alerts.".into(),
                severity: "error".into(),
            });
        }
        if let Some(interval) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
            && !(900..=86_400).contains(&interval)
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "out_of_range".into(),
                message: "The polling interval must be between 900 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        if let Some(template) = object.get("messageTemplate")
            && (!template.is_string()
                || template
                    .as_str()
                    .is_some_and(|value| value.trim().is_empty() || value.chars().count() > 1_800))
        {
            issues.push(ValidationIssue {
                path: "messageTemplate".into(),
                code: "invalid_template".into(),
                message: "The alert message must be non-empty and at most 1800 characters.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let prefix = self.key.replace('.', "_");
        let mut projection = Vec::new();
        for field in [
            self.source_key,
            "targetChannelId",
            "intervalSeconds",
            "messageTemplate",
            "mention",
        ] {
            if let Some(value) = object.get(field) {
                let value = value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_i64().map(|number| number.to_string()));
                if let Some(value) = value {
                    projection.push((format!("{prefix}.{field}"), value));
                }
            }
        }
        projection
    }
}

/// Configuration for interaction-driven community modules.  Keeping these
/// schemas in the core registry prevents the panel from inventing controls
/// that the Discord handlers do not understand.
#[derive(Debug, Clone, Copy)]
pub struct CommunityInteractionAdapter {
    pub key: &'static str,
    pub source: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schema: &'static str,
    pub defaults: &'static str,
    pub dependencies: &'static [&'static str],
    pub projection: fn(&serde_json::Value) -> Vec<(String, String)>,
}

fn project_suggestions(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    if let Some(value) = object.get("channel").and_then(serde_json::Value::as_str) {
        pairs.push(("community.suggestions.channel_id".into(), value.into()));
    }
    if let Some(value) = object.get("anonymous").and_then(serde_json::Value::as_bool) {
        pairs.push(("community.suggestions.anonymous".into(), value.to_string()));
    }
    if let Some(value) = object
        .get("requiredRole")
        .and_then(serde_json::Value::as_str)
    {
        pairs.push(("community.suggestions.required_role".into(), value.into()));
    }
    pairs
}

fn project_giveaways(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    if let Some(value) = object
        .get("defaultDurationHours")
        .and_then(serde_json::Value::as_i64)
    {
        pairs.push((
            "community.giveaways.default_duration_hours".into(),
            value.to_string(),
        ));
    }
    if let Some(value) = object
        .get("defaultWinners")
        .and_then(serde_json::Value::as_i64)
    {
        pairs.push((
            "community.giveaways.default_winners".into(),
            value.to_string(),
        ));
    }
    if let Some(value) = object
        .get("requiredRole")
        .and_then(serde_json::Value::as_str)
    {
        pairs.push(("community.giveaways.required_role".into(), value.into()));
    }
    pairs
}

fn project_polls(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    if let Some(value) = object
        .get("defaultDurationHours")
        .and_then(serde_json::Value::as_i64)
    {
        pairs.push((
            "management.polls.default_duration_hours".into(),
            value.to_string(),
        ));
    }
    if let Some(value) = object.get("channel").and_then(serde_json::Value::as_str) {
        pairs.push(("management.polls.channel_id".into(), value.into()));
    }
    pairs
}

fn project_events(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    if let Some(value) = object
        .get("defaultCapacity")
        .and_then(serde_json::Value::as_i64)
    {
        pairs.push((
            "community.events.default_capacity".into(),
            value.to_string(),
        ));
    }
    if let Some(value) = object
        .get("announcementChannel")
        .and_then(serde_json::Value::as_str)
    {
        pairs.push((
            "community.events.announcement_channel_id".into(),
            value.into(),
        ));
    }
    pairs
}

fn project_role_panels(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for (field, key) in [
        ("channel", "community.role_panels.channel_id"),
        ("panelTitle", "community.role_panels.title"),
        ("panelDescription", "community.role_panels.description"),
    ] {
        if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
            pairs.push((key.into(), value.into()));
        }
    }
    if let Some(value) = object.get("maxRoles").and_then(serde_json::Value::as_i64) {
        pairs.push(("community.role_panels.max_roles".into(), value.to_string()));
    }
    if let Some(value) = object
        .get("removeOnUnselect")
        .and_then(serde_json::Value::as_bool)
    {
        pairs.push((
            "community.role_panels.remove_on_unselect".into(),
            value.to_string(),
        ));
    }
    if let Some(value) = object
        .get("selectionMode")
        .and_then(serde_json::Value::as_str)
        .filter(|mode| *mode == "multiple" || *mode == "unique")
    {
        pairs.push(("community.role_panels.selection_mode".into(), value.into()));
    }
    if let Some(values) = object.get("roleIds").and_then(serde_json::Value::as_array) {
        let ids = values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| value.parse::<u64>().is_ok())
            .take(5)
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            pairs.push(("community.role_panels.role_ids".into(), ids.join(",")));
        }
    }
    pairs
}

fn validate_interaction_config(config: &serde_json::Value, key: &str) -> Vec<ValidationIssue> {
    let Some(object) = config.as_object() else {
        return vec![ValidationIssue {
            path: "config".into(),
            code: "object_required".into(),
            message: "The configuration must be an object.".into(),
            severity: "error".into(),
        }];
    };
    let mut issues = Vec::new();
    for field in ["channel", "requiredRole", "announcementChannel"] {
        if object.get(field).is_some_and(|value| {
            value
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.parse::<u64>().is_err())
        }) {
            issues.push(ValidationIssue {
                path: field.into(),
                code: "invalid_discord_id".into(),
                message: "Choose a valid Discord channel or role.".into(),
                severity: "error".into(),
            });
        }
    }
    for field in ["anonymous", "removeOnUnselect"] {
        if object.get(field).is_some_and(|value| !value.is_boolean()) {
            issues.push(ValidationIssue {
                path: field.into(),
                code: "boolean_required".into(),
                message: "This option must be true or false.".into(),
                severity: "error".into(),
            });
        }
    }
    for (field, min, max) in [
        ("defaultDurationHours", 1_i64, 168_i64),
        ("defaultWinners", 1_i64, 20_i64),
        ("defaultCapacity", 0_i64, 100_000_i64),
        ("maxRoles", 1_i64, 5_i64),
    ] {
        if let Some(value) = object.get(field) {
            if value.as_i64().is_none() {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "integer_required".into(),
                    message: "This value must be an integer.".into(),
                    severity: "error".into(),
                });
            } else if !value
                .as_i64()
                .is_some_and(|number| (min..=max).contains(&number))
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "out_of_range".into(),
                    message: format!("Value must be between {min} and {max}."),
                    severity: "error".into(),
                });
            }
        }
    }
    if key == "community.role_panels" {
        for (field, max) in [("panelTitle", 80_usize), ("panelDescription", 1_000_usize)] {
            if let Some(value) = object.get(field)
                && (value.as_str().is_none()
                    || value
                        .as_str()
                        .is_some_and(|text| text.chars().count() > max))
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "invalid_text".into(),
                    message: format!("Text must be at most {max} characters."),
                    severity: "error".into(),
                });
            }
        }
        if let Some(values) = object.get("roleIds") {
            let valid = values.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.len() <= 5
                    && items
                        .iter()
                        .all(|value| value.as_str().is_some_and(|id| id.parse::<u64>().is_ok()))
            });
            if !valid {
                issues.push(ValidationIssue {
                    path: "roleIds".into(),
                    code: "invalid_role_ids".into(),
                    message: "Choose between one and five valid Discord roles.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("selectionMode") {
            let valid = value
                .as_str()
                .is_some_and(|mode| mode == "multiple" || mode == "unique");
            if !valid {
                issues.push(ValidationIssue {
                    path: "selectionMode".into(),
                    code: "invalid_selection_mode".into(),
                    message: "Choose multiple roles or one role at a time.".into(),
                    severity: "error".into(),
                });
            }
        }
    }
    issues
}

impl FeatureAdapter for CommunityInteractionAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: self.source.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::from_str(self.schema)
                .expect("community adapter schema is valid JSON"),
            defaults: serde_json::from_str(self.defaults)
                .expect("community adapter defaults are valid JSON"),
            dependencies: self
                .dependencies
                .iter()
                .map(|value| (*value).into())
                .collect(),
        }
    }
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        validate_interaction_config(config, self.key)
    }
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        (self.projection)(config)
    }
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
                        {"key":"shadowMode","label":"Shadow mode","kind":"toggle","help":"Record and alert without automatic containment."},
                        {"key":"logChannel","label":"Audit log channel (optional)","kind":"channel","advanced":true},
                        {"key":"includeContent","label":"Include cached message content","kind":"toggle","advanced":true,"help":"Off by default. Discord may not provide content for every edit/delete event."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"threshold": 3, "windowSeconds": 10, "shadowMode": false, "logChannel": "", "includeContent": false}),
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
        if object.get("logChannel").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|raw| raw.is_empty() || raw.parse::<u64>().is_ok())
        }) {
            issues.push(ValidationIssue {
                path: "logChannel".into(),
                code: "invalid_channel_id".into(),
                message: "Choose a valid Discord channel or leave it empty.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("includeContent")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "includeContent".into(),
                code: "boolean_required".into(),
                message: "Content logging must be true or false.".into(),
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
        if let Some(value) = object.get("logChannel").and_then(serde_json::Value::as_str) {
            projection.push(("management.audit.log_channel".into(), value.into()));
        }
        if let Some(value) = object
            .get("includeContent")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("management.audit.include_content".into(), value.to_string()));
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
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        // Templates are consumed by the import/export endpoints rather than a
        // Discord event handler.  Still publish an explicit, guild-scoped
        // policy so the adapter cannot appear configured while the module is
        // silently disabled.
        let enabled = config.is_object();
        vec![("management.templates.enabled".into(), enabled.to_string())]
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
            // Provider integrations have their own subscription builders in
            // the API.  Do not expose an empty generic section that suggests
            // a toggle is enough to configure delivery.
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": self.source,
                "sections": [],
                "notes": [{"title": self.title, "description": self.description}]
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

/// The XP card has a dedicated editor and renderer, but it still participates
/// in the common adapter contract.  Keeping its schema and projection here
/// prevents the specialised endpoint from becoming a second, invisible
/// configuration source.
#[derive(Debug, Clone, Copy, Default)]
pub struct RankCardAdapter;

impl RankCardAdapter {
    pub const KEY: &'static str = "studio.rank_card";
    pub const SOURCE: &'static str = "rank_card_adapter_v2";

    fn valid_hex(value: &str) -> bool {
        value.len() == 7
            && value.starts_with('#')
            && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    }

    fn valid_config(config: &RankCardConfig) -> bool {
        matches!(
            config.font.as_str(),
            "system" | "inter" | "roboto" | "poppins" | "space_grotesk" | "lexend"
        ) && Self::valid_hex(&config.primary_color)
            && Self::valid_hex(&config.text_color)
            && Self::valid_hex(&config.background_color)
            && Self::valid_hex(&config.avatar_ring_color)
            && config.overlay_opacity.is_finite()
            && (0.0..=0.85).contains(&config.overlay_opacity)
            && config.avatar_ring_width <= 8
            && config.background_preset.as_deref().is_none_or(|preset| {
                RANK_CARD_BACKGROUND_PRESETS
                    .iter()
                    .any(|(id, _)| *id == preset)
            })
            && config.background_url.is_none()
            && config.background_data.is_none()
    }
}

impl FeatureAdapter for RankCardAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "XP card appearance",
                    "description": "Choose curated backgrounds and colours for the XP card rendered by /rank.",
                    "fields": [
                        {"key":"font","label":"Font","kind":"select","options":["system","inter","roboto","poppins","space_grotesk","lexend"]},
                        {"key":"primaryColor","label":"Primary colour","kind":"color"},
                        {"key":"textColor","label":"Text colour","kind":"color"},
                        {"key":"backgroundColor","label":"Background colour","kind":"color"},
                        {"key":"overlayOpacity","label":"Overlay opacity","kind":"number","min":0,"max":0.85},
                        {"key":"backgroundPreset","label":"Curated background","kind":"select","advanced":true},
                        {"key":"avatarRingColor","label":"Avatar ring colour","kind":"color","advanced":true},
                        {"key":"avatarRingWidth","label":"Avatar ring width","kind":"number","min":0,"max":8,"advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::to_value(RankCardConfig::default())
                .expect("rank card defaults serialize"),
            dependencies: vec!["attach_files".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        match serde_json::from_value::<RankCardConfig>(config.clone()) {
            Ok(parsed) if Self::valid_config(&parsed) => Vec::new(),
            Ok(parsed) => vec![ValidationIssue {
                path: "config".into(),
                code: "invalid_rank_card".into(),
                message: if parsed.background_url.is_some() || parsed.background_data.is_some() {
                    "XP card backgrounds must use one of the curated presets; custom images are not accepted.".into()
                } else {
                    "Use a supported font, hexadecimal colours, an opacity between 0 and 0.85, and a curated background.".into()
                },
                severity: "error".into(),
            }],
            Err(_) => vec![ValidationIssue {
                path: "config".into(),
                code: "invalid_rank_card".into(),
                message: "XP card configuration must match the supported appearance fields.".into(),
                severity: "error".into(),
            }],
        }
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        serde_json::to_string(config)
            .ok()
            .map(|value| vec![("community.rank_card".into(), value)])
            .unwrap_or_default()
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
        let feed_url = object
            .get("feedUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if feed_url.is_empty() {
            issues.push(ValidationIssue {
                path: "feedUrl".into(),
                code: "required".into(),
                message: "Choose a public RSS or Atom feed URL.".into(),
                severity: "error".into(),
            });
        } else if feed_url.chars().count() > 2_000
            || !feed_url.starts_with("https://") && !feed_url.starts_with("http://")
        {
            issues.push(ValidationIssue {
                path: "feedUrl".into(),
                code: "invalid_url".into(),
                message:
                    "The feed URL must use http:// or https:// and be at most 2000 characters."
                        .into(),
                severity: "error".into(),
            });
        }
        let target_channel = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if target_channel.is_empty() || target_channel.parse::<u64>().is_err() {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "invalid_discord_id".into(),
                message: "Choose a real Discord channel for alerts.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("messageTemplate")
            && (!value.is_string()
                || value
                    .as_str()
                    .is_some_and(|value| value.chars().count() > 2_000))
        {
            issues.push(ValidationIssue {
                path: "messageTemplate".into(),
                code: "too_long".into(),
                message: "Alert messages must be text and at most 2000 characters.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("mention")
            && (!value.is_string()
                || value.as_str().is_some_and(|mention| {
                    !mention.trim().is_empty()
                        && mention.trim() != "@here"
                        && mention.trim() != "@everyone"
                        && !mention.trim().starts_with("<@&")
                }))
        {
            issues.push(ValidationIssue {
                path: "mention".into(),
                code: "invalid_mention".into(),
                message: "Mention must be empty, @here, @everyone or a role mention.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("intervalSeconds")
            .is_some_and(|value| value.as_i64().is_none())
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "integer_required".into(),
                message: "Polling interval must be an integer in seconds.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("feedUrl")
            .is_some_and(|value| value.as_str().is_none())
        {
            issues.push(ValidationIssue {
                path: "feedUrl".into(),
                code: "string_required".into(),
                message: "Feed URL must be text.".into(),
                severity: "error".into(),
            });
        }
        if object
            .get("targetChannelId")
            .is_some_and(|value| value.as_str().is_none())
        {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "string_required".into(),
                message: "Discord channel ID must be text.".into(),
                severity: "error".into(),
            });
        }
        /*
         * Keep provider validation bounded here as well as in the provider
         * client. This means a saved beta feature cannot look healthy when
         * its subscription is only a half-filled JSON object.
         */
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

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let mut projection = Vec::new();
        for (field, key) in [
            ("feedUrl", "social.feed.url"),
            ("targetChannelId", "social.feed.target_channel_id"),
            ("messageTemplate", "social.feed.message_template"),
            ("mention", "social.feed.mention"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                projection.push((key.into(), value.into()));
            }
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push(("social.feed.interval_seconds".into(), value.to_string()));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlueskyAdapter;

impl BlueskyAdapter {
    pub const KEY: &'static str = "social.bluesky";
    pub const SOURCE: &'static str = "bluesky_public_feed_v1";
}

impl FeatureAdapter for BlueskyAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Bluesky alerts",
                    "description": "Poll a public profile through the official Bluesky AppView API and post new updates in Discord.",
                    "fields": [
                        {"key":"sourceHandle","label":"Bluesky handle","kind":"text","help":"For example: vozen.org or @vozen.org."},
                        {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                        {"key":"intervalSeconds","label":"Polling interval (seconds)","kind":"number","min":300,"max":86400},
                        {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                        {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "sourceHandle": "",
                "targetChannelId": "",
                "intervalSeconds": 900,
                "messageTemplate": "New Bluesky post from {handle}: **{text}**\\n{url}",
                "mention": ""
            }),
            dependencies: vec!["Bluesky public AppView API".into(), "Send Messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Bluesky alert configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        let handle = object
            .get("sourceHandle")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if handle.is_empty()
            || handle.len() > 253
            || !handle.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'@')
            })
        {
            issues.push(ValidationIssue {
                path: "sourceHandle".into(),
                code: "invalid_handle".into(),
                message: "Use a valid public Bluesky handle, such as vozen.org.".into(),
                severity: "error".into(),
            });
        }
        let target = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if target.is_empty() || target.parse::<u64>().is_err() {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "invalid_discord_id".into(),
                message: "Choose a real Discord channel for alerts.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
            && !(300..=86_400).contains(&value)
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "out_of_range".into(),
                message: "Polling interval must be between 300 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        if object.get("messageTemplate").is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|text| text.trim().is_empty() || text.chars().count() > 1_800)
        }) {
            issues.push(ValidationIssue {
                path: "messageTemplate".into(),
                code: "invalid_template".into(),
                message: "Alert messages must be non-empty and at most 1800 characters.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("mention").and_then(serde_json::Value::as_str)
            && !value.trim().is_empty()
            && value.trim() != "@here"
            && value.trim() != "@everyone"
            && !(value.trim().starts_with("<@&") && value.trim().ends_with('>'))
        {
            issues.push(ValidationIssue {
                path: "mention".into(),
                code: "invalid_mention".into(),
                message: "Mention must be empty, @here, @everyone or a role mention.".into(),
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
        for (field, key) in [
            ("sourceHandle", "social.bluesky.handle"),
            ("targetChannelId", "social.bluesky.target_channel_id"),
            ("messageTemplate", "social.bluesky.message_template"),
            ("mention", "social.bluesky.mention"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                projection.push((key.into(), value.into()));
            }
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push(("social.bluesky.interval_seconds".into(), value.to_string()));
        }
        projection
    }
}

/// Read-only Reddit alert contract.  The adapter is intentionally present
/// even while the catalogue remains blocked: the panel can explain the exact
/// fields and the API can validate drafts without ever implying that Reddit
/// commercial access has been approved.
#[derive(Debug, Clone, Copy, Default)]
pub struct RedditAdapter;

impl RedditAdapter {
    pub const KEY: &'static str = "social.reddit";
    pub const SOURCE: &'static str = "reddit_oauth_readonly_v1";
}

impl FeatureAdapter for RedditAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Reddit alerts",
                    "description": "Read public subreddit posts through Reddit's official OAuth API. Commercial approval is required before enabling delivery.",
                    "fields": [
                        {"key":"sourceSubreddit","label":"Subreddit","kind":"text","help":"For example: vozen or r/vozen."},
                        {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                        {"key":"intervalSeconds","label":"Polling interval (seconds)","kind":"number","min":300,"max":86400},
                        {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                        {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "sourceSubreddit": "",
                "targetChannelId": "",
                "intervalSeconds": 900,
                "messageTemplate": "New post in r/{subreddit}: **{title}**\\n{permalink}",
                "mention": ""
            }),
            dependencies: vec![
                "Reddit OAuth application".into(),
                "Commercial API approval".into(),
                "Send Messages".into(),
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Reddit alert configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let source = object
            .get("sourceSubreddit")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        let valid_source = (2..=50).contains(&source.len())
            && source
                .trim_start_matches('/')
                .trim_start_matches("r/")
                .trim_start_matches("subreddit/")
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        let target = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        let mut issues = Vec::new();
        if !valid_source {
            issues.push(ValidationIssue {
                path: "sourceSubreddit".into(),
                code: "invalid_subreddit".into(),
                message: "Use a subreddit name such as vozen or r/vozen; URLs are not accepted."
                    .into(),
                severity: "error".into(),
            });
        }
        if target.is_empty() || target.parse::<u64>().is_err() {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "invalid_discord_id".into(),
                message: "Choose a real Discord channel for alerts.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
            && !(300..=86_400).contains(&value)
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "out_of_range".into(),
                message: "Polling interval must be between 300 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        if object.get("messageTemplate").is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|text| text.trim().is_empty() || text.chars().count() > 1_800)
        }) {
            issues.push(ValidationIssue {
                path: "messageTemplate".into(),
                code: "invalid_template".into(),
                message: "Alert messages must be non-empty and at most 1800 characters.".into(),
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
        for (field, key) in [
            ("sourceSubreddit", "social.reddit.subreddit"),
            ("targetChannelId", "social.reddit.target_channel_id"),
            ("messageTemplate", "social.reddit.message_template"),
            ("mention", "social.reddit.mention"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                projection.push((key.into(), value.into()));
            }
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push(("social.reddit.interval_seconds".into(), value.to_string()));
        }
        projection
    }
}

/// Contract for integrations whose runtime is deliberately gated by an
/// external provider account, app review, commercial agreement or operator
/// secret.  Keeping these adapters in the registry means the panel can show
/// a real schema, validation and exact dependency checklist instead of a
/// misleading empty form.  The maturity remains `Blocked` until the
/// provider-specific gate is satisfied.
#[derive(Debug, Clone, Copy)]
pub struct ExternalProviderAdapter {
    pub key: &'static str,
    pub source: &'static str,
    pub schema: &'static str,
    pub defaults: &'static str,
    pub dependencies: &'static [&'static str],
}

fn is_eth_address(value: &str) -> bool {
    let body = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"));
    body.is_some_and(|text| text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

impl FeatureAdapter for ExternalProviderAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: self.source.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::from_str(self.schema).expect("external schema is valid JSON"),
            defaults: serde_json::from_str(self.defaults)
                .expect("external defaults are valid JSON"),
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
                message: "The integration configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        if let Some(value) = object.get("targetChannelId")
            && !value
                .as_str()
                .is_some_and(|text| text.trim().is_empty() || text.trim().parse::<u64>().is_ok())
        {
            issues.push(ValidationIssue {
                path: "targetChannelId".into(),
                code: "invalid_discord_id".into(),
                message: "Choose a real Discord channel.".into(),
                severity: "error".into(),
            });
        }
        for field in ["sourceHandle", "username", "account", "collectionSlug"] {
            if let Some(value) = object.get(field)
                && let Some(text) = value.as_str()
                && !text.trim().is_empty()
                && (!(2..=100).contains(&text.trim().chars().count())
                    || text
                        .trim()
                        .chars()
                        .any(|character| character.is_control() || character.is_whitespace()))
            {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "invalid_source_identifier".into(),
                    message: "Use the provider's identifier, not a URL or arbitrary text.".into(),
                    severity: "error".into(),
                });
            }
        }
        if let Some(value) = object.get("intervalSeconds")
            && (!value.is_i64()
                || !value
                    .as_i64()
                    .is_some_and(|seconds| (300..=86_400).contains(&seconds)))
        {
            issues.push(ValidationIssue {
                path: "intervalSeconds".into(),
                code: "out_of_range".into(),
                message: "Polling intervals must be between 300 and 86400 seconds.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("messageTemplate")
            && !value
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= 1_800)
        {
            issues.push(ValidationIssue {
                path: "messageTemplate".into(),
                code: "invalid_template".into(),
                message: "Alert messages must be at most 1800 characters.".into(),
                severity: "error".into(),
            });
        }
        if self.key == "growth.monetization" {
            if let Some(value) = object.get("productName")
                && (!value.is_string()
                    || value
                        .as_str()
                        .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 80))
            {
                issues.push(ValidationIssue {
                    path: "productName".into(),
                    code: "invalid_product_name".into(),
                    message: "Product name must contain 1 to 80 characters.".into(),
                    severity: "error".into(),
                });
            }
            if let Some(value) = object.get("priceCents")
                && (!value.is_i64()
                    || !value
                        .as_i64()
                        .is_some_and(|cents| (0..=10_000_000).contains(&cents)))
            {
                issues.push(ValidationIssue {
                    path: "priceCents".into(),
                    code: "invalid_price".into(),
                    message: "Prices must be integer cents between 0 and 10000000.".into(),
                    severity: "error".into(),
                });
            }
        }
        if self.key == "web3.gating"
            && let Some(value) = object.get("chain")
            && !value
                .as_str()
                .is_some_and(|chain| matches!(chain, "ethereum" | "polygon" | "arbitrum" | "base"))
        {
            issues.push(ValidationIssue {
                path: "chain".into(),
                code: "unsupported_chain".into(),
                message: "Choose one of the supported read-only chains.".into(),
                severity: "error".into(),
            });
        }
        if self.key == "web3.gating" {
            if let Some(value) = object.get("contractAddress")
                && let Some(address) = value.as_str()
                && !address.trim().is_empty()
                && !is_eth_address(address.trim())
            {
                issues.push(ValidationIssue {
                    path: "contractAddress".into(),
                    code: "invalid_contract_address".into(),
                    message: "Use a 0x-prefixed Ethereum contract address.".into(),
                    severity: "error".into(),
                });
            }
            if let Some(value) = object.get("targetRoleId")
                && let Some(role) = value.as_str()
                && !role.trim().is_empty()
                && role.parse::<u64>().is_err()
            {
                issues.push(ValidationIssue {
                    path: "targetRoleId".into(),
                    code: "invalid_discord_id".into(),
                    message: "Choose a real Discord role.".into(),
                    severity: "error".into(),
                });
            }
            let asset_type = object
                .get("assetType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("erc721");
            if !matches!(asset_type, "erc20" | "erc721" | "erc1155") {
                issues.push(ValidationIssue {
                    path: "assetType".into(),
                    code: "invalid_asset_type".into(),
                    message: "Choose ERC-20, ERC-721 or ERC-1155.".into(),
                    severity: "error".into(),
                });
            }
            let minimum = object
                .get("minimumBalance")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if minimum.is_empty()
                || minimum.len() > 39
                || !minimum.bytes().all(|byte| byte.is_ascii_digit())
            {
                issues.push(ValidationIssue {
                    path: "minimumBalance".into(),
                    code: "invalid_uint".into(),
                    message: "Minimum balance must be a non-negative decimal integer.".into(),
                    severity: "error".into(),
                });
            }
            if asset_type == "erc1155" {
                let token_id = object
                    .get("tokenId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if token_id.is_empty() || token_id.len() > 78 {
                    issues.push(ValidationIssue {
                        path: "tokenId".into(),
                        code: "token_id_required".into(),
                        message: "ERC-1155 gating requires a bounded token ID.".into(),
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
        let prefix = self.key.replace('.', "_");
        let mut projection = Vec::new();
        for (field, suffix) in [
            ("productName", "product_name"),
            ("sourceHandle", "source_handle"),
            ("username", "username"),
            ("account", "account"),
            ("collectionSlug", "collection_slug"),
            ("targetChannelId", "target_channel_id"),
            ("targetRoleId", "target_role_id"),
            ("messageTemplate", "message_template"),
            ("mention", "mention"),
            ("chain", "chain"),
            ("contractAddress", "contract_address"),
            ("minimumBalance", "minimum_balance"),
            ("assetType", "asset_type"),
            ("tokenId", "token_id"),
            ("currency", "currency"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                projection.push((format!("{prefix}.{suffix}"), value.into()));
            }
        }
        for (field, suffix) in [
            ("intervalSeconds", "interval_seconds"),
            ("priceCents", "price_cents"),
            ("trialDays", "trial_days"),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_i64) {
                projection.push((format!("{prefix}.{suffix}"), value.to_string()));
            }
        }
        projection
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CryptoAdapter {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub stats: bool,
}

impl FeatureAdapter for CryptoAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        let mut fields = vec![
            serde_json::json!({
                "key": "coinIds",
                "label": "CoinGecko IDs",
                "kind": "text",
                "help": "Comma-separated IDs such as bitcoin, ethereum or solana."
            }),
            serde_json::json!({
                "key": "currency",
                "label": "Currency",
                "kind": "text",
                "help": "Three to ten lowercase letters, for example usd or eur."
            }),
        ];
        if self.stats {
            fields.extend([
                serde_json::json!({"key":"targetChannelId","label":"Discord channel","kind":"channel"}),
                serde_json::json!({"key":"intervalSeconds","label":"Update interval (seconds)","kind":"number","min":300,"max":86400}),
                serde_json::json!({"key":"messageTemplate","label":"Statistics message","kind":"textarea","advanced":true}),
            ]);
        } else {
            fields.push(serde_json::json!({
                "key": "maxResults",
                "label": "Maximum results",
                "kind": "number",
                "min": 1,
                "max": 10,
                "advanced": true
            }));
        }
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: "coingecko_simple_price_v1".into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": "coingecko_simple_price_v1",
                "sections": [{
                    "title": self.title,
                    "description": self.description,
                    "fields": fields
                }]
            }),
            defaults: if self.stats {
                serde_json::json!({
                    "coinIds": "bitcoin",
                    "currency": "usd",
                    "targetChannelId": "",
                    "intervalSeconds": 900,
                    "messageTemplate": "Crypto update: {coins}"
                })
            } else {
                serde_json::json!({
                    "coinIds": "bitcoin",
                    "currency": "usd",
                    "maxResults": 5
                })
            },
            dependencies: vec![
                "CoinGecko public API".into(),
                if self.stats {
                    "Send Messages".into()
                } else {
                    "Use Application Commands".into()
                },
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Crypto configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        let coins = object
            .get("coinIds")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let coin_list = coins
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if coin_list.is_empty() || coin_list.len() > 20 {
            issues.push(ValidationIssue {
                path: "coinIds".into(),
                code: "invalid_coin_ids".into(),
                message: "Choose between one and twenty CoinGecko IDs.".into(),
                severity: "error".into(),
            });
        } else if coin_list.iter().any(|coin| {
            !(1..=64).contains(&coin.len())
                || !coin
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || coin.starts_with('-')
                || coin.ends_with('-')
        }) {
            issues.push(ValidationIssue {
                path: "coinIds".into(),
                code: "invalid_coin_id".into(),
                message: "Coin IDs may contain only letters, numbers and hyphens.".into(),
                severity: "error".into(),
            });
        }
        let currency = object
            .get("currency")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if !(3..=10).contains(&currency.len())
            || !currency.bytes().all(|byte| byte.is_ascii_lowercase())
        {
            issues.push(ValidationIssue {
                path: "currency".into(),
                code: "invalid_currency".into(),
                message: "Currency must use three to ten lowercase letters, such as usd.".into(),
                severity: "error".into(),
            });
        }
        if self.stats {
            let target = object
                .get("targetChannelId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            if target.is_empty() || target.parse::<u64>().is_err() {
                issues.push(ValidationIssue {
                    path: "targetChannelId".into(),
                    code: "invalid_discord_id".into(),
                    message: "Choose a real Discord channel for statistics.".into(),
                    severity: "error".into(),
                });
            }
            if let Some(interval) = object
                .get("intervalSeconds")
                .and_then(serde_json::Value::as_i64)
                && !(300..=86_400).contains(&interval)
            {
                issues.push(ValidationIssue {
                    path: "intervalSeconds".into(),
                    code: "out_of_range".into(),
                    message: "Update interval must be between 300 and 86400 seconds.".into(),
                    severity: "error".into(),
                });
            }
            if object.get("messageTemplate").is_some_and(|value| {
                value.as_str().is_none_or(|template| {
                    template.trim().is_empty() || template.chars().count() > 1_800
                })
            }) {
                issues.push(ValidationIssue {
                    path: "messageTemplate".into(),
                    code: "invalid_template".into(),
                    message: "Statistics messages must be non-empty and at most 1800 characters."
                        .into(),
                    severity: "error".into(),
                });
            }
        } else if let Some(max_results) =
            object.get("maxResults").and_then(serde_json::Value::as_i64)
            && !(1..=10).contains(&max_results)
        {
            issues.push(ValidationIssue {
                path: "maxResults".into(),
                code: "out_of_range".into(),
                message: "Maximum results must be between 1 and 10.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let prefix = self.key.replace('.', "_");
        let mut projection = Vec::new();
        if let Some(coins) = object.get("coinIds").and_then(serde_json::Value::as_str) {
            projection.push((
                format!("{prefix}.coin_ids"),
                coins.trim().to_ascii_lowercase(),
            ));
        }
        if let Some(currency) = object.get("currency").and_then(serde_json::Value::as_str) {
            projection.push((
                format!("{prefix}.currency"),
                currency.trim().to_ascii_lowercase(),
            ));
        }
        if let Some(max_results) = object.get("maxResults").and_then(serde_json::Value::as_i64) {
            projection.push((format!("{prefix}.max_results"), max_results.to_string()));
        }
        if let Some(channel) = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((format!("{prefix}.target_channel_id"), channel.to_string()));
        }
        if let Some(interval) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((format!("{prefix}.interval_seconds"), interval.to_string()));
        }
        if let Some(template) = object
            .get("messageTemplate")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((format!("{prefix}.message_template"), template.to_string()));
        }
        projection
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GasAdapter;

impl FeatureAdapter for GasAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: "web3.gas_tracker".into(),
            source: "operator_rpc_gas_v1".into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": "operator_rpc_gas_v1",
                "sections": [{
                    "title": "Gas tracker",
                    "description": "Publish read-only gas prices from an operator-approved RPC endpoint.",
                    "fields": [
                        {"key":"network","label":"Network","kind":"select","options":["ethereum","polygon","arbitrum","base"]},
                        {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                        {"key":"intervalSeconds","label":"Update interval (seconds)","kind":"number","min":300,"max":86400},
                        {"key":"messageTemplate","label":"Statistics message","kind":"textarea","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "network": "ethereum",
                "targetChannelId": "",
                "intervalSeconds": 900,
                "messageTemplate": "{network} gas: {gasPriceGwei} Gwei (block {blockNumber})"
            }),
            dependencies: vec!["Operator-approved HTTPS RPC".into(), "Send Messages".into()],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "Gas tracker configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        let network = object
            .get("network")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !matches!(
            network.as_str(),
            "ethereum" | "polygon" | "arbitrum" | "base"
        ) {
            issues.push(ValidationIssue {
                path: "network".into(),
                code: "unsupported_network".into(),
                message: "Choose ethereum, polygon, arbitrum or base.".into(),
                severity: "error".into(),
            });
        }
        validate_channel_and_schedule(object, &mut issues, "gas");
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        project_gas(config)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NftAdapter {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub sales: bool,
    pub alerts: bool,
}

impl FeatureAdapter for NftAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        let mut fields = vec![
            serde_json::json!({"key":"collectionSlug","label":"OpenSea collection slug","kind":"text"}),
        ];
        if self.sales && !self.alerts {
            // Collection queries return one bounded collection document; a
            // result-count control would be decorative and is intentionally
            // not exposed. Sales alerts use maxResults below because they
            // fetch an event list.
        } else {
            fields.extend([
                serde_json::json!({"key":"targetChannelId","label":"Discord channel","kind":"channel"}),
                serde_json::json!({"key":"intervalSeconds","label":"Update interval (seconds)","kind":"number","min":300,"max":86400}),
                serde_json::json!({"key":"messageTemplate","label":"Statistics message","kind":"textarea","advanced":true}),
            ]);
            if self.sales {
                fields.push(serde_json::json!({"key":"maxResults","label":"Maximum events","kind":"number","min":1,"max":10,"advanced":true}));
            }
        }
        FeatureAdapterDescriptor {
            key: self.key.into(),
            source: "opensea_read_only_v1".into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({"version":FEATURE_SCHEMA_VERSION,"source":"opensea_read_only_v1","sections":[{"title":self.title,"description":self.description,"fields":fields}]}),
            defaults: if self.sales && !self.alerts {
                serde_json::json!({"collectionSlug":""})
            } else {
                serde_json::json!({"collectionSlug":"","targetChannelId":"","intervalSeconds":900,"messageTemplate":"OpenSea update: {collection}","maxResults":5})
            },
            dependencies: vec![
                "OpenSea API key".into(),
                if self.sales {
                    "Use Application Commands".into()
                } else {
                    "Send Messages".into()
                },
            ],
        }
    }

    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue> {
        let Some(object) = config.as_object() else {
            return vec![ValidationIssue {
                path: "config".into(),
                code: "object_required".into(),
                message: "NFT configuration must be an object.".into(),
                severity: "error".into(),
            }];
        };
        let mut issues = Vec::new();
        let slug = object
            .get("collectionSlug")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if slug.is_empty()
            || slug.len() > 128
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || slug.starts_with('-')
            || slug.ends_with('-')
        {
            issues.push(ValidationIssue {
                path: "collectionSlug".into(),
                code: "invalid_collection_slug".into(),
                message: "Use the OpenSea collection slug, not a URL.".into(),
                severity: "error".into(),
            });
        }
        if self.sales
            && let Some(max) = object.get("maxResults").and_then(serde_json::Value::as_i64)
            && !(1..=10).contains(&max)
        {
            issues.push(ValidationIssue {
                path: "maxResults".into(),
                code: "out_of_range".into(),
                message: "Maximum events must be between 1 and 10.".into(),
                severity: "error".into(),
            });
        }
        if !self.sales || self.alerts {
            validate_channel_and_schedule(object, &mut issues, "NFT");
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let Some(object) = config.as_object() else {
            return Vec::new();
        };
        let prefix = self.key.replace('.', "_");
        let mut projection = Vec::new();
        if let Some(value) = object
            .get("collectionSlug")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((
                format!("{prefix}.collection_slug"),
                value.trim().to_ascii_lowercase(),
            ));
        }
        if let Some(value) = object.get("maxResults").and_then(serde_json::Value::as_i64) {
            projection.push((format!("{prefix}.max_results"), value.to_string()));
        }
        if let Some(value) = object
            .get("targetChannelId")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((format!("{prefix}.target_channel_id"), value.to_string()));
        }
        if let Some(value) = object
            .get("intervalSeconds")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((format!("{prefix}.interval_seconds"), value.to_string()));
        }
        if let Some(value) = object
            .get("messageTemplate")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((format!("{prefix}.message_template"), value.to_string()));
        }
        projection
    }
}

fn validate_channel_and_schedule(
    object: &serde_json::Map<String, serde_json::Value>,
    issues: &mut Vec<ValidationIssue>,
    label: &str,
) {
    let target = object
        .get("targetChannelId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    if target.is_empty() || target.parse::<u64>().is_err() {
        issues.push(ValidationIssue {
            path: "targetChannelId".into(),
            code: "invalid_discord_id".into(),
            message: format!("Choose a real Discord channel for {label} updates."),
            severity: "error".into(),
        });
    }
    if let Some(interval) = object
        .get("intervalSeconds")
        .and_then(serde_json::Value::as_i64)
        && !(300..=86_400).contains(&interval)
    {
        issues.push(ValidationIssue {
            path: "intervalSeconds".into(),
            code: "out_of_range".into(),
            message: "Update interval must be between 300 and 86400 seconds.".into(),
            severity: "error".into(),
        });
    }
    if object.get("messageTemplate").is_some_and(|value| {
        value
            .as_str()
            .is_none_or(|template| template.trim().is_empty() || template.chars().count() > 1_800)
    }) {
        issues.push(ValidationIssue {
            path: "messageTemplate".into(),
            code: "invalid_template".into(),
            message: "Messages must be non-empty and at most 1800 characters.".into(),
            severity: "error".into(),
        });
    }
}

fn project_gas(config: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = config.as_object() else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for (field, key) in [
        ("network", "web3.gas_tracker.network"),
        ("targetChannelId", "web3.gas_tracker.target_channel_id"),
        ("messageTemplate", "web3.gas_tracker.message_template"),
    ] {
        if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
            pairs.push((key.into(), value.to_string()));
        }
    }
    if let Some(value) = object
        .get("intervalSeconds")
        .and_then(serde_json::Value::as_i64)
    {
        pairs.push((
            "web3.gas_tracker.interval_seconds".into(),
            value.to_string(),
        ));
    }
    pairs
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
                        {"key":"farewellMessage","label":"Farewell message","kind":"textarea","maxLength":2000,"advanced":true},
                        {"key":"templateId","label":"Reusable message template","kind":"select","options":[["","No template"]],"help":"Optional: choose a template created in Models and importação.","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({"channel":"","message":"Welcome {member} to {server}!","delaySeconds":0,"sendDm":false,"dmMessage":"Hello {member}, welcome to {server}!","autoRole":"","farewellChannel":"","farewellMessage":"Goodbye {member}. We hope to see you again!","templateId":""}),
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
        if object.get("templateId").is_some_and(|value| {
            !value.as_str().is_some_and(|raw| {
                raw.is_empty()
                    || ((1..=64).contains(&raw.len())
                        && raw.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
                        }))
            })
        }) {
            issues.push(ValidationIssue {
                path: "templateId".into(),
                code: "invalid_template_id".into(),
                message: "Choose a template from this guild.".into(),
                severity: "error".into(),
            });
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
            ("templateId", "support.welcome.template_id"),
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let member = fixture_string(fixture, "member", "member", "<@member>");
        let server = fixture_string(fixture, "server", "server", "this server");
        let message = config
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Welcome {member} to {server}!");
        let rendered = render_member_message(message, member, server);
        let delay = config
            .get("delaySeconds")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            .clamp(0, 300);
        let mut effects = vec![format!(
            "Send the welcome message `{rendered}` after {delay} second(s)."
        )];
        if config
            .get("sendDm")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            let dm = config
                .get("dmMessage")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Hello {member}, welcome to {server}!");
            effects.push(format!(
                "Send a direct message `{}`.",
                render_member_message(dm, member, server)
            ));
        }
        if let Some(role) = config
            .get("autoRole")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            effects.push(format!("Assign the configured automatic role `{role}`."));
        }
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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

/// Parse the fixed UTC offsets supported by the reminder scheduler.
///
/// The panel deliberately offers a bounded list instead of arbitrary IANA
/// names: this keeps scheduling deterministic across the API, gateway and
/// tests without pulling a timezone database into the bot process.  Values
/// such as `UTC+01`, `UTC+01:30`, `UTC-05:00` and `UTC` are accepted.
pub fn parse_utc_offset_minutes(raw: &str) -> Option<i32> {
    let value = raw.trim();
    if value.eq_ignore_ascii_case("UTC") || value.eq_ignore_ascii_case("GMT") {
        return Some(0);
    }
    let suffix = value
        .strip_prefix("UTC")
        .or_else(|| value.strip_prefix("GMT"))?;
    let (sign, digits) = match suffix.as_bytes().first().copied()? {
        b'+' => (1_i32, &suffix[1..]),
        b'-' => (-1_i32, &suffix[1..]),
        _ => return None,
    };
    let (hours, minutes) = if let Some((hours, minutes)) = digits.split_once(':') {
        (hours.parse::<i32>().ok()?, minutes.parse::<i32>().ok()?)
    } else {
        (digits.parse::<i32>().ok()?, 0)
    };
    if !(0..=14).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    if hours == 14 && minutes != 0 {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

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
                    {"key":"timezone","label":"Reminder timezone","kind":"select","options":[["UTC","UTC"],["UTC-05:00","UTC-05:00"],["UTC+01:00","UTC+01:00"],["UTC+02:00","UTC+02:00"],["UTC+05:30","UTC+05:30"],["UTC+08:00","UTC+08:00"]],"help":"Used when a reminder time is written as HH:MM, such as 09:30."},
                    {"key":"notifyUser","label":"Mention the member when it fires","kind":"toggle","help":"Turn off to post a quiet reminder without a mention."},
                    {"key":"allowRecurring","label":"Allow recurring reminders","kind":"toggle","help":"Members can choose a bounded daily or weekly reminder."},
                    {"key":"maxRecurrences","label":"Maximum repeats","kind":"number","min":1,"max":52,"help":"Limits how many times a recurring reminder is re-created."}
                ]
            }]
        })
    }

    fn defaults() -> serde_json::Value {
        serde_json::json!({"maxDelayHours": 168, "maxTextLength": 500, "timezone": "UTC", "notifyUser": true, "allowRecurring": false, "maxRecurrences": 12})
    }
}

/// Runtime policy shared by the reminder command and the dashboard
/// simulation. Keeping the bounds here prevents a saved setting from being
/// interpreted differently by the scheduler and by the preview endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderPolicy {
    pub max_delay_hours: u64,
    pub max_text_length: usize,
    pub timezone: String,
    pub notify_user: bool,
    pub allow_recurring: bool,
    pub max_recurrences: u64,
}

impl Default for ReminderPolicy {
    fn default() -> Self {
        Self {
            max_delay_hours: 168,
            max_text_length: 500,
            timezone: "UTC".into(),
            notify_user: true,
            allow_recurring: false,
            max_recurrences: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderObservation {
    pub delay_ms: i64,
    pub text: String,
    pub repeat: Option<String>,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReminderDecision {
    pub allowed: bool,
    pub reason_code: &'static str,
    pub explanation: String,
    pub delay_ms: i64,
    pub repeat: Option<String>,
    pub remaining: u64,
}

pub fn reminder_policy_from_json(config: &serde_json::Value) -> ReminderPolicy {
    let object = config.as_object();
    ReminderPolicy {
        max_delay_hours: object
            .and_then(|value| value.get("maxDelayHours"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(168)
            .clamp(1, 8_760),
        max_text_length: object
            .and_then(|value| value.get("maxTextLength"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(500)
            .clamp(50, 500) as usize,
        timezone: object
            .and_then(|value| value.get("timezone"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("UTC")
            .to_owned(),
        notify_user: object
            .and_then(|value| value.get("notifyUser"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        allow_recurring: object
            .and_then(|value| value.get("allowRecurring"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        max_recurrences: object
            .and_then(|value| value.get("maxRecurrences"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(12)
            .clamp(1, 52),
    }
}

/// Evaluate one reminder request. This is deliberately pure and bounded so
/// that `POST .../simulate` and the Discord slash command cannot drift apart.
pub fn evaluate_reminder(
    policy: &ReminderPolicy,
    observation: &ReminderObservation,
) -> ReminderDecision {
    let max_delay_ms = policy
        .max_delay_hours
        .saturating_mul(3_600_000)
        .min(i64::MAX as u64) as i64;
    let reject = |reason_code: &'static str, explanation: String| ReminderDecision {
        allowed: false,
        reason_code,
        explanation,
        delay_ms: observation.delay_ms,
        repeat: observation.repeat.clone(),
        remaining: policy.max_recurrences,
    };
    if parse_utc_offset_minutes(&observation.timezone).is_none() {
        return reject(
            "invalid_timezone",
            "The reminder timezone must be UTC or a fixed offset between -14:00 and +14:00.".into(),
        );
    }
    if observation.delay_ms <= 0 {
        return reject(
            "invalid_delay",
            "The reminder time must be in the future.".into(),
        );
    }
    if observation.delay_ms > max_delay_ms {
        return reject(
            "delay_exceeds_limit",
            format!(
                "The reminder is beyond the configured {} hour limit.",
                policy.max_delay_hours
            ),
        );
    }
    if observation.text.len() > policy.max_text_length {
        return reject(
            "text_exceeds_limit",
            format!(
                "The reminder is longer than the configured {} character limit.",
                policy.max_text_length
            ),
        );
    }
    if let Some(repeat) = observation.repeat.as_deref() {
        if !policy.allow_recurring {
            return reject(
                "recurring_disabled",
                "Recurring reminders are disabled in this server's dashboard.".into(),
            );
        }
        if !matches!(repeat, "daily" | "weekly") {
            return reject(
                "invalid_repeat",
                "Choose daily or weekly for a recurring reminder.".into(),
            );
        }
    }
    ReminderDecision {
        allowed: true,
        reason_code: "accepted",
        explanation: "The reminder is within the server limits and can be scheduled.".into(),
        delay_ms: observation.delay_ms,
        repeat: observation.repeat.clone(),
        remaining: if observation.repeat.is_some() {
            policy.max_recurrences
        } else {
            0
        },
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
        if let Some(value) = object.get("timezone") {
            let valid = value.as_str().and_then(parse_utc_offset_minutes).is_some();
            if !valid {
                issues.push(ValidationIssue {
                    path: "timezone".into(),
                    code: "invalid_timezone".into(),
                    message: "Use UTC or a fixed offset from UTC between -14:00 and +14:00.".into(),
                    severity: "error".into(),
                });
            }
        }
        if object
            .get("allowRecurring")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "allowRecurring".into(),
                code: "boolean_required".into(),
                message: "Recurring reminder preference must be true or false.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("maxRecurrences") {
            if let Some(value) = value.as_i64() {
                if !(1..=52).contains(&value) {
                    issues.push(ValidationIssue {
                        path: "maxRecurrences".into(),
                        code: "out_of_range".into(),
                        message: "The value must be between 1 and 52.".into(),
                        severity: "error".into(),
                    });
                }
            } else {
                issues.push(ValidationIssue {
                    path: "maxRecurrences".into(),
                    code: "integer_required".into(),
                    message: "Maximum repeats must be an integer.".into(),
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
        if let Some(value) = object.get("timezone").and_then(serde_json::Value::as_str)
            && parse_utc_offset_minutes(value).is_some()
        {
            projection.push(("utility.reminders.timezone".into(), value.to_owned()));
        }
        if let Some(value) = object
            .get("notifyUser")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("utility.reminders.notify_user".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("allowRecurring")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push((
                "utility.reminders.allow_recurring".into(),
                value.to_string(),
            ));
        }
        if let Some(value) = object
            .get("maxRecurrences")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "utility.reminders.max_recurrences".into(),
                value.to_string(),
            ));
        }
        projection
    }

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = reminder_policy_from_json(config);
        let observation = ReminderObservation {
            delay_ms: fixture
                .get("delayMs")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(600_000),
            text: fixture
                .get("reminderText")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Preview reminder")
                .to_owned(),
            repeat: fixture
                .get("repeat")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            timezone: fixture
                .get("timezone")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(policy.timezone.as_str())
                .to_owned(),
        };
        let decision = evaluate_reminder(&policy, &observation);
        if decision.allowed {
            let mention = if policy.notify_user {
                " with a member mention"
            } else {
                " quietly"
            };
            let recurrence = decision
                .repeat
                .as_deref()
                .map(|repeat| format!(" and repeat {repeat} up to {} time(s)", decision.remaining))
                .unwrap_or_default();
            vec![format!(
                "Schedule the bounded reminder{}{} (delay: {} ms).",
                mention, recurrence, decision.delay_ms
            )]
        } else {
            vec![format!(
                "Reminder rejected ({}): {}",
                decision.reason_code, decision.explanation
            )]
        }
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = leaderboard_policy_from_json(config);
        let entries = leaderboard_entries_from_json(fixture);
        let decision = evaluate_leaderboard(&policy, entries);
        let visibility = if decision.public { "public" } else { "private" };
        let listed = decision
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| format!("{}. {} — {} XP", index + 1, entry.user_id, entry.xp))
            .collect::<Vec<_>>();
        let mut effect = format!(
            "Render a {visibility} XP leaderboard with up to {} members.",
            policy.max_entries
        );
        if !listed.is_empty() {
            effect.push_str(&format!(" Preview: {}.", listed.join("; ")));
        } else {
            effect.push_str(" Preview contains no eligible members.");
        }
        if decision.excluded_opt_outs > 0 {
            effect.push_str(&format!(
                " Excluded {} member(s) who opted out.",
                decision.excluded_opt_outs
            ));
        }
        if decision.truncated {
            effect.push_str(" Additional eligible members are hidden by the configured limit.");
        }
        let mut effects = vec![effect];
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = WorkflowPolicy {
            max_reply_length: config
                .get("maxReplyLength")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1_000)
                .clamp(1, 1_500) as usize,
            allow_mentions: config
                .get("allowMentions")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        };
        let workflow = fixture.get("workflow").unwrap_or(fixture);
        let decision = evaluate_workflow(
            &policy,
            &WorkflowObservation {
                enabled: fixture_bool(workflow, "enabled", "enabled", true),
                trigger: fixture_string(workflow, "trigger", "trigger", "message").into(),
                condition: fixture_string(workflow, "condition", "condition", "").into(),
                action: fixture_string(workflow, "action", "action", "reply").into(),
                payload: fixture_string(workflow, "payload", "payload", "Thanks {user}: {message}")
                    .into(),
                message_content: fixture_string(
                    fixture,
                    "messageContent",
                    "message_content",
                    "hello preview",
                )
                .into(),
                user_mention: fixture_string(
                    fixture,
                    "userMention",
                    "user_mention",
                    "<@preview-user>",
                )
                .into(),
            },
        );
        let mut effects = if let Some(reply) = decision.reply {
            vec![format!(
                "Workflow matched and would send this bounded reply: {reply}"
            )]
        } else {
            vec![format!(
                "Workflow would not send a reply ({}).",
                decision.reason
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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
                    "description": "Control the period, visibility and optional live counter channel for server statistics.",
                    "fields": [
                        {"key":"windowDays","label":"Reporting window (days)","kind":"number","min":1,"max":30,"help":"Number of recent daily snapshots included."},
                        {"key":"public","label":"Show publicly","kind":"toggle","help":"When disabled, only the requesting member sees the summary."},
                        {"key":"channelId","label":"Live counter channel (optional)","kind":"channel","help":"Rename one existing voice or text channel with the latest message count."},
                        {"key":"intervalMinutes","label":"Counter refresh (minutes)","kind":"number","min":5,"max":1440,"advanced":true},
                        {"key":"nameTemplate","label":"Channel name template","kind":"text","max":100,"help":"Use {messages}, {joins}, {leaves} and {days}."}
                    ]
                }]
            }),
            defaults: serde_json::json!({"windowDays": 7, "public": false, "channelId": "", "intervalMinutes": 15, "nameTemplate": "messages-{messages}"}),
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
        if let Some(value) = object.get("channelId")
            && (!value.is_string()
                || value
                    .as_str()
                    .is_some_and(|value| !value.is_empty() && value.parse::<u64>().is_err()))
        {
            issues.push(ValidationIssue {
                path: "channelId".into(),
                code: "channel_id_required".into(),
                message: "Live counter channel must be a Discord channel ID or empty.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("intervalMinutes")
            && !value
                .as_i64()
                .is_some_and(|value| (5..=1_440).contains(&value))
        {
            issues.push(ValidationIssue {
                path: "intervalMinutes".into(),
                code: "out_of_range".into(),
                message: "Counter refresh must be between 5 and 1440 minutes.".into(),
                severity: "error".into(),
            });
        }
        if let Some(value) = object.get("nameTemplate")
            && (!value.is_string()
                || value.as_str().is_some_and(|value| {
                    value.trim().is_empty() || value.chars().count() > 100 || !value.contains('{')
                }))
        {
            issues.push(ValidationIssue {
                path: "nameTemplate".into(),
                code: "invalid_template".into(),
                message: "Channel name template must be non-empty, at most 100 characters and contain a placeholder.".into(),
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
        if let Some(value) = object.get("channelId").and_then(serde_json::Value::as_str) {
            projection.push(("insights.stats.channel_id".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("intervalMinutes")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "insights.stats.interval_minutes".into(),
                value.clamp(5, 1_440).to_string(),
            ));
        }
        if let Some(value) = object
            .get("nameTemplate")
            .and_then(serde_json::Value::as_str)
        {
            projection.push(("insights.stats.name_template".into(), value.to_string()));
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = ModerationPolicy {
            require_reason: config
                .get("requireReason")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            max_purge: config
                .get("maxPurge")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(100)
                .clamp(1, 100) as u64,
        };
        let action = fixture_string(fixture, "action", "action", "purge");
        let reason = fixture_string(fixture, "reason", "reason", "");
        let requested_count = fixture
            .get("requestedCount")
            .or_else(|| fixture.get("requested_count"))
            .and_then(serde_json::Value::as_i64);
        let decision = evaluate_moderation(
            &policy,
            &ModerationObservation {
                action: action.to_owned(),
                reason: reason.to_owned(),
                requested_count,
            },
        );
        let mut effects = if decision.allowed {
            if let Some(count) = decision.effective_count {
                vec![format!(
                    "Allow `{action}` with an effective limit of {count} message(s)."
                )]
            } else {
                vec![format!(
                    "Allow `{action}` after the configured safety checks."
                )]
            }
        } else {
            vec![format!("Block `{action}`: {}", decision.explanation)]
        };
        effects.push(format!("Decision code: `{}`.", decision.reason_code));
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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
                        {"key":"ignoredRoles","label":"Ignored roles","kind":"roles","max":100,"advanced":true},
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
                "ignoredRoles": [],
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
            ("ignoredRoles", 100, false),
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
        for field in ["ignoredChannels", "ignoredRoles"] {
            if let Some(value) = object.get(field) {
                let valid_ids = value.as_array().is_some_and(|items| {
                    items.iter().all(|item| {
                        item.as_str()
                            .is_some_and(|id| id.trim().parse::<u64>().is_ok())
                    })
                });
                if !valid_ids {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "invalid_discord_ids".into(),
                        message: "Choose valid Discord channel or role IDs.".into(),
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
            ("ignoredRoles", "security.antiscam.ignored_roles"),
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let channel_id = fixture_string(fixture, "channelId", "channel_id", "preview-channel");
        let role_ids = fixture
            .get("roleIds")
            .or_else(|| fixture.get("role_ids"))
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .take(100)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let content = fixture_string(
            fixture,
            "content",
            "content",
            "Claim your free Nitro at https://discord.gg/example",
        );
        let decision = evaluate_scam_with_roles(
            &scam_policy_from_json(config),
            channel_id,
            &role_ids,
            content,
        );
        let mut effects = if decision.ignored {
            vec!["Ignore the message because this channel is exempt.".into()]
        } else if decision.matched.is_empty() {
            vec!["No scam pattern matched; keep the message.".into()]
        } else if decision.should_act {
            vec![format!(
                "Match {} and apply the configured action (timeout: {}s).",
                decision.matched.join(", "),
                decision.timeout_seconds
            )]
        } else {
            vec![format!(
                "Match {} in monitor-only mode; do not modify the message.",
                decision.matched.join(", ")
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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
                        {"key":"message","label":"Guide message","kind":"textarea","min":1,"max":2000,"help":"Use {member} and {server} as placeholders."},
                        {"key":"steps","label":"Guided steps","kind":"tags","max":4,"help":"Choose from rules, introductions, channels and help."},
                        {"key":"rulesChannel","label":"Rules channel (optional)","kind":"channel","advanced":true},
                        {"key":"introductionsChannel","label":"Introductions channel (optional)","kind":"channel","advanced":true},
                        {"key":"channelsChannel","label":"Channels guide (optional)","kind":"channel","advanced":true},
                        {"key":"templateId","label":"Reusable guide template","kind":"select","options":[["","No template"]],"help":"Optional: choose a template created in Models and importação.","advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "channelId": "",
                "message": "Welcome {member}! Start with the rules, introduce yourself and check the server channels.",
                "steps": ["rules", "introductions", "channels"],
                "rulesChannel": "",
                "introductionsChannel": "",
                "channelsChannel": "",
                "templateId": ""
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
        if object.get("templateId").is_some_and(|value| {
            !value.as_str().is_some_and(|raw| {
                raw.is_empty()
                    || ((1..=64).contains(&raw.len())
                        && raw.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
                        }))
            })
        }) {
            issues.push(ValidationIssue {
                path: "templateId".into(),
                code: "invalid_template_id".into(),
                message: "Choose a template from this guild.".into(),
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
        if let Some(value) = object.get("steps") {
            let valid = value.as_array().is_some_and(|steps| {
                (1..=4).contains(&steps.len())
                    && steps.iter().all(|step| {
                        step.as_str().is_some_and(|step| {
                            matches!(step, "rules" | "introductions" | "channels" | "help")
                        })
                    })
                    && steps
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == steps.len()
            });
            if !valid {
                issues.push(ValidationIssue {
                    path: "steps".into(),
                    code: "invalid_steps".into(),
                    message: "Choose one to four unique steps from rules, introductions, channels and help.".into(),
                    severity: "error".into(),
                });
            }
        }
        for field in ["rulesChannel", "introductionsChannel", "channelsChannel"] {
            if object.get(field).is_some_and(|value| {
                !value
                    .as_str()
                    .is_some_and(|raw| raw.is_empty() || raw.parse::<u64>().is_ok())
            }) {
                issues.push(ValidationIssue {
                    path: field.into(),
                    code: "invalid_channel_id".into(),
                    message: "Choose a valid Discord channel or leave it empty.".into(),
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
        if let Some(value) = object.get("channelId").and_then(serde_json::Value::as_str) {
            pairs.push(("support.welcome_channel.channel_id".into(), value.into()));
        }
        if let Some(value) = object.get("message").and_then(serde_json::Value::as_str) {
            pairs.push(("support.welcome_channel.message".into(), value.into()));
        }
        if let Some(values) = object.get("steps").and_then(serde_json::Value::as_array) {
            pairs.push((
                "support.welcome_channel.steps".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        for (field, setting) in [
            ("rulesChannel", "support.welcome_channel.rules_channel"),
            (
                "introductionsChannel",
                "support.welcome_channel.introductions_channel",
            ),
            (
                "channelsChannel",
                "support.welcome_channel.channels_channel",
            ),
        ] {
            if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                pairs.push((setting.into(), value.into()));
            }
        }
        if let Some(value) = object.get("templateId").and_then(serde_json::Value::as_str) {
            pairs.push(("support.welcome_channel.template_id".into(), value.into()));
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
                        {"key":"ignoredChannels","label":"Ignored channels","kind":"channels","max":100,"advanced":true},
                        {"key":"ignoredRoles","label":"Ignored member roles","kind":"roles","max":100,"advanced":true}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "channel": "",
                "threshold": 3,
                "emoji": "⭐",
                "allowSelfStar": false,
                "includeImages": true,
                "ignoredChannels": [],
                "ignoredRoles": []
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
        for field in ["ignoredChannels", "ignoredRoles"] {
            if let Some(value) = object.get(field) {
                let valid = value.as_array().is_some_and(|items| {
                    items.len() <= 100
                        && items.iter().all(|item| {
                            item.as_str().is_some_and(|text| {
                                !text.trim().is_empty() && text.parse::<u64>().is_ok()
                            })
                        })
                });
                if !valid {
                    issues.push(ValidationIssue {
                        path: field.into(),
                        code: "invalid_discord_ids".into(),
                        message: "Choose up to 100 valid Discord IDs.".into(),
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
        if let Some(values) = object
            .get("ignoredRoles")
            .and_then(serde_json::Value::as_array)
        {
            pairs.push((
                "community.starboard.ignored_roles".into(),
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        pairs
    }

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = starboard_policy_from_json(config);
        let reactor_ids = {
            let values = fixture_strings(fixture, "reactorIds", "reactor_ids");
            if values.is_empty() {
                (0..fixture_u64(fixture, "reactionCount", "reaction_count", 5))
                    .take(100)
                    .map(|index| format!("preview-reactor-{index}"))
                    .collect()
            } else {
                values
            }
        };
        let decision = evaluate_starboard(
            &policy,
            &StarboardObservation {
                source_channel_id: fixture_string(
                    fixture,
                    "channelId",
                    "channel_id",
                    "preview-channel",
                )
                .to_owned(),
                author_id: fixture_string(fixture, "authorId", "author_id", "preview-author")
                    .to_owned(),
                reactor_ids,
                author_role_ids: fixture_strings(fixture, "authorRoleIds", "author_role_ids"),
                has_attachments: fixture_bool(fixture, "hasAttachments", "has_attachments", false),
            },
        );
        let mut effects = if decision.should_publish {
            vec![format!(
                "Create or update the starboard mirror ({}/{} reactions). {}",
                decision.count, decision.threshold, decision.reason
            )]
        } else if decision.ignored {
            vec![format!("Do not mirror this message. {}", decision.reason)]
        } else {
            vec![format!(
                "Keep the original message below the board threshold ({}/{} reactions). {}",
                decision.count, decision.threshold, decision.reason
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
    }
}

/// Runtime policy for the starboard.  Both the Discord event handler and the
/// API simulator consume this type so a preview cannot disagree with the
/// reaction reconciliation performed by the bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarboardPolicy {
    pub threshold: u64,
    pub allow_self_star: bool,
    pub include_images: bool,
    pub ignored_channels: Vec<String>,
    pub ignored_roles: Vec<String>,
}

impl Default for StarboardPolicy {
    fn default() -> Self {
        Self {
            threshold: 3,
            allow_self_star: false,
            include_images: true,
            ignored_channels: Vec::new(),
            ignored_roles: Vec::new(),
        }
    }
}

pub fn starboard_policy_from_json(config: &serde_json::Value) -> StarboardPolicy {
    let object = config.as_object();
    let ids = |field: &str| {
        object
            .and_then(|values| values.get(field))
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    StarboardPolicy {
        threshold: object
            .and_then(|values| values.get("threshold"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(3)
            .clamp(1, 100),
        allow_self_star: object
            .and_then(|values| values.get("allowSelfStar"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        include_images: object
            .and_then(|values| values.get("includeImages"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        ignored_channels: ids("ignoredChannels"),
        ignored_roles: ids("ignoredRoles"),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StarboardObservation {
    pub source_channel_id: String,
    pub author_id: String,
    pub reactor_ids: Vec<String>,
    pub author_role_ids: Vec<String>,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarboardDecision {
    pub ignored: bool,
    pub should_publish: bool,
    pub count: u64,
    pub threshold: u64,
    pub reason: String,
}

pub fn evaluate_starboard(
    policy: &StarboardPolicy,
    observation: &StarboardObservation,
) -> StarboardDecision {
    let ignored_channel = policy
        .ignored_channels
        .iter()
        .any(|value| value == &observation.source_channel_id);
    let ignored_role = policy.ignored_roles.iter().any(|ignored| {
        observation
            .author_role_ids
            .iter()
            .any(|role| role == ignored)
    });
    if ignored_channel || ignored_role {
        return StarboardDecision {
            ignored: true,
            should_publish: false,
            count: 0,
            threshold: policy.threshold,
            reason: if ignored_channel {
                "The source channel is ignored by the starboard policy.".into()
            } else {
                "The message author has an ignored role.".into()
            },
        };
    }
    if observation.has_attachments && !policy.include_images {
        return StarboardDecision {
            ignored: true,
            should_publish: false,
            count: 0,
            threshold: policy.threshold,
            reason: "Image attachments are disabled by the starboard policy.".into(),
        };
    }
    let mut reactors = HashSet::new();
    for reactor_id in &observation.reactor_ids {
        if policy.allow_self_star || reactor_id != &observation.author_id {
            reactors.insert(reactor_id);
        }
    }
    let count = reactors.len() as u64;
    StarboardDecision {
        ignored: false,
        should_publish: count >= policy.threshold,
        count,
        threshold: policy.threshold,
        reason: if count >= policy.threshold {
            "The message reached the configured star threshold.".into()
        } else {
            "The message is below the configured star threshold.".into()
        },
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
                        {"key":"pauseInvites","label":"Pause invite entry during an incident","kind":"toggle","help":"Uses the join gate temporarily and restores it when containment expires."},
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
                "pauseInvites": true,
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
        if object
            .get("pauseInvites")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "pauseInvites".into(),
                code: "boolean_required".into(),
                message: "Pause invite entry must be true or false.".into(),
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
            .get("pauseInvites")
            .and_then(serde_json::Value::as_bool)
        {
            pairs.push(("security.anti_raid.pause_invites".into(), value.to_string()));
        }
        if let Some(value) = object
            .get("verification")
            .and_then(serde_json::Value::as_str)
        {
            pairs.push(("security.anti_raid.verification".into(), value.into()));
        }
        pairs
    }

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = anti_raid_policy_from_json(config);
        let joins = fixture_u64(
            fixture,
            "joinCount",
            "join_count",
            policy.join_threshold as u64,
        );
        let decision = evaluate_anti_raid(&policy, joins as u32, policy.alert_only);
        let mut effects = if decision.armed {
            vec![format!(
                "{} ({}/{} joins in {}s; containment {} for {} minutes).",
                if decision.shadow_mode || !decision.should_contain {
                    "Monitor the burst without automatic containment"
                } else {
                    "Contain the join burst"
                },
                decision.joins,
                policy.join_threshold,
                policy.window_seconds,
                if decision.shadow_mode || !decision.should_contain {
                    "disabled"
                } else {
                    "enabled"
                },
                decision.incident_minutes
            )]
        } else {
            vec![format!(
                "Keep monitoring: {}/{} joins in the current {}s window.",
                decision.joins, policy.join_threshold, policy.window_seconds
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let object = config.as_object();
        let policy = JoinGatePolicy {
            minimum_account_days: object
                .and_then(|values| values.get("minimumAccountDays"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
            require_avatar: object
                .and_then(|values| values.get("requireAvatar"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            blocked_name_patterns: object
                .and_then(|values| values.get("blockedNamePatterns"))
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .take(20)
                        .collect()
                })
                .unwrap_or_default(),
            action: object
                .and_then(|values| values.get("action"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("quarantine")
                .to_owned(),
        };
        let decision = evaluate_join_gate(
            &policy,
            &JoinGateObservation {
                account_age_days: fixture_u64(fixture, "accountAgeDays", "account_age_days", 30)
                    as i64,
                has_avatar: fixture_bool(fixture, "hasAvatar", "has_avatar", true),
                display_name: fixture_string(
                    fixture,
                    "displayName",
                    "display_name",
                    "Preview member",
                )
                .to_owned(),
            },
        );
        let mut effects = if !decision.blocked {
            vec!["Allow the member and apply the configured auto-role, if any.".into()]
        } else {
            vec![format!(
                "Apply the {} join-gate action because {}.",
                decision.action,
                decision.reasons.join("; ")
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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
        if config
            .get("alertOnly")
            .is_some_and(|value| !value.is_boolean())
        {
            issues.push(ValidationIssue {
                path: "alertOnly".into(),
                code: "boolean_required".into(),
                message: "Apenas alertar tem de ser verdadeiro ou falso.".into(),
                severity: "error".into(),
            });
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = anti_spam_policy_from_json(config);
        let observation = anti_spam_observation_from_json(fixture);
        let decision = evaluate_anti_spam(&policy, &observation);
        if decision.ignored {
            return vec!["Ignore the message because its channel or member role is exempt.".into()];
        }
        if decision.matched.is_empty() {
            return vec!["Record no match and leave the message unchanged.".into()];
        }
        let mode = if decision.should_act {
            "action"
        } else {
            "monitor-only"
        };
        let action = if decision.should_act && decision.timeout_seconds > 0 {
            format!(" and apply a {} second timeout", decision.timeout_seconds)
        } else {
            String::new()
        };
        let mut effects = vec![format!(
            "Record {} in {mode} mode{action} ({} messages, {} duplicate(s), {} mention(s)).",
            decision.matched.join(", "),
            observation.message_count,
            observation.duplicate_count,
            observation.mention_count
        )];
        if let Some(channel) = config
            .get("logChannel")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            effects.push(format!("Publish the decision to log channel {channel}."));
        }
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(key, value)| format!("Runtime setting {key}={value}.")),
        );
        effects
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

    fn simulate(&self, config: &serde_json::Value, fixture: &serde_json::Value) -> Vec<String> {
        let policy = AchievementPolicy {
            first_threshold: config
                .get("firstThreshold")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(100),
            regular_threshold: config
                .get("regularThreshold")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(1_000),
            pillar_threshold: config
                .get("pillarThreshold")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(10_000),
        };
        let xp = fixture
            .get("xp")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(1_250);
        let unlocked = evaluate_achievements(&policy, xp);
        let mut effects = if unlocked.is_empty() {
            vec![format!("No achievement is unlocked at {xp} XP.")]
        } else {
            vec![format!(
                "Unlock {} at {xp} XP: {}.",
                unlocked.len(),
                unlocked
                    .iter()
                    .map(|item| format!("{} ({} XP)", item.label, item.threshold))
                    .collect::<Vec<_>>()
                    .join(", ")
            )]
        };
        effects.extend(
            self.runtime_projection(config)
                .into_iter()
                .map(|(setting, value)| format!("Runtime setting `{setting}` = `{value}`.")),
        );
        effects
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
static SUGGESTIONS_ADAPTER: CommunityInteractionAdapter = CommunityInteractionAdapter {
    key: "community.suggestions",
    source: "suggestions_adapter_v2",
    title: "Suggestion workflow",
    description: "Route suggestions, control anonymous submissions and require an optional member role.",
    schema: r#"{"version":1,"source":"suggestions_adapter_v2","sections":[{"title":"Suggestion intake","description":"These options are applied to every new suggestion.","fields":[{"key":"channel","label":"Suggestion channel","kind":"channel"},{"key":"anonymous","label":"Hide author in the public message","kind":"toggle"},{"key":"requiredRole","label":"Required role (optional)","kind":"role"}]}]}"#,
    defaults: r#"{"channel":"","anonymous":false,"requiredRole":""}"#,
    dependencies: &["send_messages", "interactions"],
    projection: project_suggestions,
};
static GIVEAWAYS_ADAPTER: CommunityInteractionAdapter = CommunityInteractionAdapter {
    key: "community.giveaways",
    source: "giveaways_adapter_v2",
    title: "Giveaway workflow",
    description: "Set safe defaults for giveaway duration, winners and eligibility.",
    schema: r#"{"version":1,"source":"giveaways_adapter_v2","sections":[{"title":"Giveaway defaults","description":"Commands can still override these defaults when needed.","fields":[{"key":"defaultDurationHours","label":"Default duration (hours)","kind":"number","min":1,"max":168},{"key":"defaultWinners","label":"Default winners","kind":"number","min":1,"max":20},{"key":"requiredRole","label":"Required role (optional)","kind":"role"}]}]}"#,
    defaults: r#"{"defaultDurationHours":24,"defaultWinners":1,"requiredRole":""}"#,
    dependencies: &["send_messages", "scheduler", "interactions"],
    projection: project_giveaways,
};
static POLLS_ADAPTER: CommunityInteractionAdapter = CommunityInteractionAdapter {
    key: "management.polls",
    source: "polls_adapter_v2",
    title: "Poll workflow",
    description: "Set the default poll lifetime and route new polls to a real channel.",
    schema: r#"{"version":1,"source":"polls_adapter_v2","sections":[{"title":"Poll defaults","description":"The command uses these values when no override is provided.","fields":[{"key":"defaultDurationHours","label":"Default duration (hours)","kind":"number","min":1,"max":168},{"key":"channel","label":"Poll channel (optional)","kind":"channel"}]}]}"#,
    defaults: r#"{"defaultDurationHours":24,"channel":""}"#,
    dependencies: &["send_messages", "scheduler", "interactions"],
    projection: project_polls,
};
static EVENTS_ADAPTER: CommunityInteractionAdapter = CommunityInteractionAdapter {
    key: "community.events",
    source: "events_adapter_v2",
    title: "Discord events",
    description: "Set event capacity and an optional announcement channel for native events.",
    schema: r#"{"version":1,"source":"events_adapter_v2","sections":[{"title":"Event defaults","description":"Capacity is stored with each event; announcements use the selected channel.","fields":[{"key":"defaultCapacity","label":"Default capacity (0 = unlimited)","kind":"number","min":0,"max":100000},{"key":"announcementChannel","label":"Announcement channel (optional)","kind":"channel"}]}]}"#,
    defaults: r#"{"defaultCapacity":0,"announcementChannel":""}"#,
    dependencies: &["manage_events", "scheduler"],
    projection: project_events,
};
static ROLE_PANELS_ADAPTER: CommunityInteractionAdapter = CommunityInteractionAdapter {
    key: "community.role_panels",
    source: "role_panels_adapter_v2",
    title: "Role panel workflow",
    description: "Set safe defaults for role panels and enforce a bounded number of roles.",
    schema: r#"{"version":1,"source":"role_panels_adapter_v2","sections":[{"title":"Panel defaults","description":"The role panel command applies these values to newly published panels.","fields":[{"key":"channel","label":"Panel channel (optional)","kind":"channel"},{"key":"roleIds","label":"Panel roles","kind":"roles","max":5},{"key":"panelTitle","label":"Panel title","kind":"text","min":1,"max":80},{"key":"panelDescription","label":"Panel description","kind":"textarea","max":1000},{"key":"maxRoles","label":"Maximum roles","kind":"number","min":1,"max":5},{"key":"selectionMode","label":"Selection mode","kind":"select","options":[["multiple","Members can choose several roles"],["unique","Members can choose one role"]]},{"key":"removeOnUnselect","label":"Remove the role when toggled off","kind":"toggle"}]}]}"#,
    defaults: r#"{"channel":"","panelTitle":"Choose your roles","panelDescription":"Select the roles that fit you.","maxRoles":5,"selectionMode":"multiple","removeOnUnselect":true}"#,
    dependencies: &["manage_roles", "interactions"],
    projection: project_role_panels,
};
static CUSTOM_COMMANDS_ADAPTER: CustomCommandsAdapter = CustomCommandsAdapter;
static AUDIT_ADAPTER: AuditAdapter = AuditAdapter;
static TEMPLATES_ADAPTER: TemplatesAdapter = TemplatesAdapter;
static BIRTHDAYS_ADAPTER: BirthdaysAdapter = BirthdaysAdapter;
static EMOJIS_ADAPTER: EmojisAdapter = EmojisAdapter;
static ACHIEVEMENTS_ADAPTER: AchievementsAdapter = AchievementsAdapter;
static INVITE_TRACKER_ADAPTER: InviteTrackerAdapter = InviteTrackerAdapter;
static SEARCH_ADAPTER: SearchAdapter = SearchAdapter;

static TEMP_CHANNELS_ADAPTER: TempChannelsAdapter = TempChannelsAdapter;

static YOUTUBE_ADAPTER: AlertSubscriptionAdapter = AlertSubscriptionAdapter {
    key: "social.youtube",
    source: "youtube_data_api_v3_adapter_v1",
    title: "YouTube alerts",
    description: "Polls the official YouTube Data API and delivers deduplicated new-video alerts.",
    source_key: "sourceChannelId",
    source_label: "YouTube channel ID",
    source_help: "Use the channel ID, not a watch URL.",
    default_template: "New video from {channel}: **{title}**\\n{url}",
    dependencies: &["YOUTUBE_API_KEY", "send_messages", "embed_links"],
};
// XP card uses a dedicated endpoint and renderer, while its schema and
// projection remain owned by the same registry as every other feature.
static RANK_CARD_ADAPTER: RankCardAdapter = RankCardAdapter;
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
static TWITCH_ADAPTER: AlertSubscriptionAdapter = AlertSubscriptionAdapter {
    key: "social.twitch",
    source: "twitch_eventsub_adapter_v1",
    title: "Twitch alerts",
    description: "Uses the official Helix/EventSub API with signed webhook verification and deduplication.",
    source_key: "sourceLogin",
    source_label: "Twitch channel login",
    source_help: "Use the channel login, without a URL or @ prefix.",
    default_template: "{handle} is live: **{title}**\\n{url}",
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
            schema: serde_json::json!({"version": FEATURE_SCHEMA_VERSION, "source": Self::SOURCE, "sections":[{"title":"Rewards", "description":"Bounded community rewards backed by an auditable balance ledger.", "fields":[{"key":"currencyName","label":"Currency name","kind":"text","maxLength":24},{"key":"dailyReward","label":"Daily reward","kind":"number","min":1,"max":10000},{"key":"workReward","label":"Work reward","kind":"number","min":1,"max":10000},{"key":"workCooldownMinutes","label":"Work cooldown (minutes)","kind":"number","min":5,"max":1440}]}]}),
            defaults: serde_json::json!({"currencyName":"credits","dailyReward":100,"workReward":50,"workCooldownMinutes":60}),
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
        let mut issues = Vec::new();
        if let Some(value) = object.get("currencyName") {
            let valid = value.as_str().is_some_and(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty() && trimmed.chars().count() <= 24
            });
            if !valid {
                issues.push(ValidationIssue {
                    path: "currencyName".into(),
                    code: "invalid_name".into(),
                    message: "Currency name must contain 1 to 24 characters.".into(),
                    severity: "error".into(),
                });
            }
        }
        for (path, min, max) in [
            ("dailyReward", 1_i64, 10_000_i64),
            ("workReward", 1_i64, 10_000_i64),
            ("workCooldownMinutes", 5_i64, 1_440_i64),
        ] {
            match object.get(path) {
                Some(value)
                    if value
                        .as_i64()
                        .is_some_and(|number| (min..=max).contains(&number)) => {}
                Some(value) if value.as_i64().is_some() => issues.push(ValidationIssue {
                    path: path.into(),
                    code: "out_of_range".into(),
                    message: format!("{path} must be between {min} and {max}."),
                    severity: "error".into(),
                }),
                Some(_) => issues.push(ValidationIssue {
                    path: path.into(),
                    code: "integer_required".into(),
                    message: format!("{path} must be an integer."),
                    severity: "error".into(),
                }),
                None => {}
            }
        }
        issues
    }
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let mut projection = Vec::new();
        if let Some(value) = config
            .get("currencyName")
            .and_then(serde_json::Value::as_str)
        {
            projection.push((
                "community.economy.currency_name".into(),
                value.trim().to_string(),
            ));
        }
        if let Some(value) = config
            .get("dailyReward")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push(("community.economy.daily_reward".into(), value.to_string()));
        }
        if let Some(value) = config.get("workReward").and_then(serde_json::Value::as_i64) {
            projection.push(("community.economy.work_reward".into(), value.to_string()));
        }
        if let Some(value) = config
            .get("workCooldownMinutes")
            .and_then(serde_json::Value::as_i64)
        {
            projection.push((
                "community.economy.work_cooldown_ms".into(),
                (value.saturating_mul(60_000)).to_string(),
            ));
        }
        projection
    }
}

/// Bounded search uses only providers with public, documented APIs.  The
/// adapter deliberately exposes the allow-list so the dashboard cannot turn
/// the feature into an arbitrary web proxy.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchAdapter;

impl SearchAdapter {
    pub const KEY: &'static str = "utility.search";
    pub const SOURCE: &'static str = "bounded_search_adapter_v1";
}

impl FeatureAdapter for SearchAdapter {
    fn descriptor(&self) -> FeatureAdapterDescriptor {
        FeatureAdapterDescriptor {
            key: Self::KEY.into(),
            source: Self::SOURCE.into(),
            schema_version: FEATURE_SCHEMA_VERSION,
            schema: serde_json::json!({
                "version": FEATURE_SCHEMA_VERSION,
                "source": Self::SOURCE,
                "sections": [{
                    "title": "Approved search sources",
                    "description": "Search is limited to providers with documented APIs; arbitrary URLs are never fetched.",
                    "fields": [
                        {"key":"maxResults","label":"Results per search","kind":"number","min":1,"max":5},
                        {"key":"allowWikipedia","label":"Wikipedia","kind":"toggle"},
                        {"key":"allowAniList","label":"AniList","kind":"toggle"},
                        {"key":"allowBluesky","label":"Bluesky","kind":"toggle"}
                    ]
                }]
            }),
            defaults: serde_json::json!({
                "maxResults": 5,
                "allowWikipedia": true,
                "allowAniList": true,
                "allowBluesky": true
            }),
            dependencies: vec!["outbound_https".into(), "send_messages".into()],
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
        if let Some(value) = object.get("maxResults")
            && !value.as_i64().is_some_and(|value| (1..=5).contains(&value))
        {
            issues.push(ValidationIssue {
                path: "maxResults".into(),
                code: "out_of_range".into(),
                message: "Results per search must be between 1 and 5.".into(),
                severity: "error".into(),
            });
        }
        for key in ["allowWikipedia", "allowAniList", "allowBluesky"] {
            if object.get(key).is_some_and(|value| !value.is_boolean()) {
                issues.push(ValidationIssue {
                    path: key.into(),
                    code: "boolean_required".into(),
                    message: "Search provider switches must be true or false.".into(),
                    severity: "error".into(),
                });
            }
        }
        if ["allowWikipedia", "allowAniList", "allowBluesky"]
            .iter()
            .all(|key| object.get(*key).and_then(serde_json::Value::as_bool) == Some(false))
        {
            issues.push(ValidationIssue {
                path: "providers".into(),
                code: "provider_required".into(),
                message: "Enable at least one approved search provider.".into(),
                severity: "error".into(),
            });
        }
        issues
    }

    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)> {
        let mut projection = Vec::new();
        if let Some(value) = config.get("maxResults").and_then(serde_json::Value::as_i64) {
            projection.push((
                "utility.search.max_results".into(),
                value.clamp(1, 5).to_string(),
            ));
        }
        if let Some(value) = config
            .get("allowWikipedia")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("utility.search.allow_wikipedia".into(), value.to_string()));
        }
        if let Some(value) = config
            .get("allowAniList")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("utility.search.allow_anilist".into(), value.to_string()));
        }
        if let Some(value) = config
            .get("allowBluesky")
            .and_then(serde_json::Value::as_bool)
        {
            projection.push(("utility.search.allow_bluesky".into(), value.to_string()));
        }
        projection
    }
}
static EMBEDS_ADAPTER: EmbedsAdapter = EmbedsAdapter;
static ECONOMY_ADAPTER: EconomyAdapter = EconomyAdapter;
static BLUESKY_ADAPTER: BlueskyAdapter = BlueskyAdapter;
static REDDIT_ADAPTER: RedditAdapter = RedditAdapter;
static INSTAGRAM_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "social.instagram",
    source: "meta_instagram_official_v1",
    schema: r#"{
        "version": 1,
        "source": "meta_instagram_official_v1",
        "sections": [{
            "title": "Instagram alerts",
            "description": "Notify a Discord channel about posts from a professional Instagram account authorised through Meta OAuth.",
            "fields": [
                {"key":"username","label":"Professional account","kind":"text","help":"The account must explicitly authorise Vozen through Meta OAuth."},
                {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                {"key":"intervalSeconds","label":"Check interval (seconds)","kind":"number","min":300,"max":86400},
                {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
            ]
        }]
    }"#,
    defaults: r#"{
        "username":"",
        "targetChannelId":"",
        "intervalSeconds":900,
        "messageTemplate":"New Instagram post from @{username}: {url}",
        "mention":""
    }"#,
    dependencies: &[
        "Meta Instagram API app",
        "Professional account OAuth grant",
        "Meta App Review and deletion callbacks",
        "Send Messages",
    ],
};
static X_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "social.x",
    source: "x_api_official_v1",
    schema: r#"{
        "version": 1,
        "source": "x_api_official_v1",
        "sections": [{
            "title": "X alerts",
            "description": "Notify a Discord channel about posts from an X account through the official API.",
            "fields": [
                {"key":"sourceHandle","label":"X handle","kind":"text","help":"Enter the handle without an @ or URL."},
                {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                {"key":"intervalSeconds","label":"Check interval (seconds)","kind":"number","min":300,"max":86400},
                {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
            ]
        }]
    }"#,
    defaults: r#"{
        "sourceHandle":"",
        "targetChannelId":"",
        "intervalSeconds":900,
        "messageTemplate":"New post from @{sourceHandle}: {url}",
        "mention":""
    }"#,
    dependencies: &[
        "X developer application",
        "Official OAuth access",
        "Approved API budget",
        "Send Messages",
    ],
};
static TIKTOK_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "social.tiktok",
    source: "tiktok_display_api_v1",
    schema: r#"{
        "version": 1,
        "source": "tiktok_display_api_v1",
        "sections": [{
            "title": "TikTok alerts",
            "description": "Notify a Discord channel about videos from a creator who authorises the Display API scopes.",
            "fields": [
                {"key":"username","label":"Creator username","kind":"text","help":"The creator must authorise the Vozen application."},
                {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                {"key":"intervalSeconds","label":"Check interval (seconds)","kind":"number","min":300,"max":86400},
                {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
            ]
        }]
    }"#,
    defaults: r#"{
        "username":"",
        "targetChannelId":"",
        "intervalSeconds":900,
        "messageTemplate":"New TikTok video from @{username}: {url}",
        "mention":""
    }"#,
    dependencies: &[
        "TikTok Display API app",
        "TikTok App Review",
        "Creator OAuth grant",
        "Send Messages",
    ],
};
static KICK_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "social.kick",
    source: "kick_official_api_v1",
    schema: r#"{
        "version": 1,
        "source": "kick_official_api_v1",
        "sections": [{
            "title": "Kick alerts",
            "description": "Notify a Discord channel about streams through an approved Kick API application.",
            "fields": [
                {"key":"sourceHandle","label":"Kick channel","kind":"text","help":"Enter the channel slug, not a web URL."},
                {"key":"targetChannelId","label":"Discord channel","kind":"channel"},
                {"key":"intervalSeconds","label":"Check interval (seconds)","kind":"number","min":300,"max":86400},
                {"key":"messageTemplate","label":"Alert message","kind":"textarea","advanced":true},
                {"key":"mention","label":"Optional mention","kind":"text","advanced":true}
            ]
        }]
    }"#,
    defaults: r#"{
        "sourceHandle":"",
        "targetChannelId":"",
        "intervalSeconds":900,
        "messageTemplate":"{sourceHandle} is live on Kick: {url}",
        "mention":""
    }"#,
    dependencies: &[
        "Approved Kick developer application",
        "Official Kick API/webhook access",
        "Send Messages",
    ],
};
static MONETIZATION_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "growth.monetization",
    source: "stripe_connect_server_sales_v1",
    schema: r#"{
        "version": 1,
        "source": "stripe_connect_server_sales_v1",
        "sections": [{
            "title": "Server monetization",
            "description": "Offer a server subscription through Stripe Connect. Production requires legal, tax and support approval.",
            "fields": [
                {"key":"productName","label":"Product name","kind":"text","max":80},
                {"key":"targetRoleId","label":"Subscriber role","kind":"role"},
                {"key":"priceCents","label":"Monthly price (cents)","kind":"number","min":0,"max":10000000},
                {"key":"currency","label":"Currency","kind":"text","help":"ISO 4217 lowercase code, for example eur."},
                {"key":"trialDays","label":"Trial days","kind":"number","min":0,"max":30}
            ]
        }]
    }"#,
    defaults: r#"{
        "productName":"",
        "targetRoleId":"",
        "priceCents":0,
        "currency":"eur",
        "trialDays":0
    }"#,
    dependencies: &[
        "Stripe Connect account",
        "KYC, tax and refund policy",
        "Signed Stripe webhooks",
        "Support and chargeback process",
    ],
};
static WALLET_GATING_ADAPTER: ExternalProviderAdapter = ExternalProviderAdapter {
    key: "web3.gating",
    source: "siwe_read_only_role_gate_v1",
    schema: r#"{
        "version": 1,
        "source": "siwe_read_only_role_gate_v1",
        "sections": [{
            "title": "Wallet gating",
            "description": "Grant a Discord role after a one-time SIWE signature and read-only contract check.",
            "fields": [
                {"key":"chain","label":"Network","kind":"select","options":["ethereum","polygon","arbitrum","base"]},
                {"key":"contractAddress","label":"Contract address","kind":"text","help":"Only an approved contract is accepted; never enter a private key."},
                {"key":"assetType","label":"Token standard","kind":"select","options":["erc20","erc721","erc1155"]},
                {"key":"tokenId","label":"Token ID (ERC-1155)","kind":"text","advanced":true},
                {"key":"targetRoleId","label":"Holder role","kind":"role"},
                {"key":"minimumBalance","label":"Minimum balance","kind":"text"},
                {"key":"intervalSeconds","label":"Recheck interval (seconds)","kind":"number","min":900,"max":86400}
            ]
        }]
    }"#,
    defaults: r#"{
        "chain":"ethereum",
        "contractAddress":"",
        "assetType":"erc721",
        "tokenId":"",
        "targetRoleId":"",
        "minimumBalance":"1",
        "intervalSeconds":3600
    }"#,
    dependencies: &[
        "SIWE domain and session secret",
        "Operator-approved RPC endpoint",
        "Approved contract allow-list",
        "Manage Roles",
    ],
};
static CRYPTO_STATS_ADAPTER: CryptoAdapter = CryptoAdapter {
    key: "web3.crypto_stats",
    title: "Crypto statistics",
    description: "Publish bounded CoinGecko price updates to a Discord channel.",
    stats: true,
};
static CRYPTO_QUERIES_ADAPTER: CryptoAdapter = CryptoAdapter {
    key: "web3.crypto_queries",
    title: "Crypto queries",
    description: "Query read-only CoinGecko prices from an application command.",
    stats: false,
};
static GAS_TRACKER_ADAPTER: GasAdapter = GasAdapter;
static NFT_STATS_ADAPTER: NftAdapter = NftAdapter {
    key: "web3.nft_stats",
    title: "NFT collection statistics",
    description: "Read-only floor, volume and sales statistics from OpenSea.",
    sales: false,
    alerts: true,
};
static NFT_QUERIES_ADAPTER: NftAdapter = NftAdapter {
    key: "web3.nft_queries",
    title: "NFT collection queries",
    description: "Query one approved OpenSea collection from an application command.",
    sales: true,
    alerts: false,
};
static NFT_SALES_ADAPTER: NftAdapter = NftAdapter {
    key: "web3.nft_sales",
    title: "NFT sales and listings",
    description: "Read-only recent sales from an approved OpenSea collection.",
    sales: true,
    alerts: true,
};

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
        SearchAdapter::KEY => Some(&SEARCH_ADAPTER as &dyn FeatureAdapter),
        "utility.temp_channels" => Some(&TEMP_CHANNELS_ADAPTER as &dyn FeatureAdapter),
        "social.youtube" => Some(&YOUTUBE_ADAPTER as &dyn FeatureAdapter),
        "social.rss" => Some(&RSS_ADAPTER as &dyn FeatureAdapter),
        "social.podcasts" => Some(&PODCASTS_ADAPTER as &dyn FeatureAdapter),
        "social.twitch" => Some(&TWITCH_ADAPTER as &dyn FeatureAdapter),
        BlueskyAdapter::KEY => Some(&BLUESKY_ADAPTER as &dyn FeatureAdapter),
        RedditAdapter::KEY => Some(&REDDIT_ADAPTER as &dyn FeatureAdapter),
        "social.instagram" => Some(&INSTAGRAM_ADAPTER as &dyn FeatureAdapter),
        "social.x" => Some(&X_ADAPTER as &dyn FeatureAdapter),
        "social.tiktok" => Some(&TIKTOK_ADAPTER as &dyn FeatureAdapter),
        "social.kick" => Some(&KICK_ADAPTER as &dyn FeatureAdapter),
        "growth.monetization" => Some(&MONETIZATION_ADAPTER as &dyn FeatureAdapter),
        "web3.gating" => Some(&WALLET_GATING_ADAPTER as &dyn FeatureAdapter),
        "studio.rank_card" => Some(&RANK_CARD_ADAPTER as &dyn FeatureAdapter),
        EmbedsAdapter::KEY => Some(&EMBEDS_ADAPTER as &dyn FeatureAdapter),
        EconomyAdapter::KEY => Some(&ECONOMY_ADAPTER as &dyn FeatureAdapter),
        "web3.crypto_stats" => Some(&CRYPTO_STATS_ADAPTER as &dyn FeatureAdapter),
        "web3.crypto_queries" => Some(&CRYPTO_QUERIES_ADAPTER as &dyn FeatureAdapter),
        "web3.gas_tracker" => Some(&GAS_TRACKER_ADAPTER as &dyn FeatureAdapter),
        "web3.nft_stats" => Some(&NFT_STATS_ADAPTER as &dyn FeatureAdapter),
        "web3.nft_queries" => Some(&NFT_QUERIES_ADAPTER as &dyn FeatureAdapter),
        "web3.nft_sales" => Some(&NFT_SALES_ADAPTER as &dyn FeatureAdapter),
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
        | "utility.search"
        | "utility.temp_channels"
        | "community.economy"
        | "studio.rank_card"
        | "social.bluesky"
        | "web3.crypto_stats"
        | "web3.crypto_queries" => FeatureMaturity::Operational,
        "social.youtube" | "social.rss" | "social.twitch" | "web3.gas_tracker"
        | "web3.nft_stats" | "web3.nft_queries" | "web3.nft_sales" => FeatureMaturity::Beta,
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
        | "growth.monetization"
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
    fn reminder_evaluator_applies_delay_length_and_repeat_policy() {
        let policy = ReminderPolicy {
            max_delay_hours: 1,
            max_text_length: 20,
            timezone: "UTC".into(),
            notify_user: true,
            allow_recurring: true,
            max_recurrences: 3,
        };
        let accepted = evaluate_reminder(
            &policy,
            &ReminderObservation {
                delay_ms: 30 * 60 * 1_000,
                text: "check the queue".into(),
                repeat: Some("daily".into()),
                timezone: "UTC".into(),
            },
        );
        assert!(accepted.allowed);
        assert_eq!(accepted.remaining, 3);

        let too_long = evaluate_reminder(
            &policy,
            &ReminderObservation {
                delay_ms: 1_000,
                text: "this reminder is deliberately too long".into(),
                repeat: None,
                timezone: "UTC".into(),
            },
        );
        assert_eq!(too_long.reason_code, "text_exceeds_limit");

        let invalid_repeat = evaluate_reminder(
            &policy,
            &ReminderObservation {
                delay_ms: 1_000,
                text: "short".into(),
                repeat: Some("hourly".into()),
                timezone: "UTC".into(),
            },
        );
        assert_eq!(invalid_repeat.reason_code, "invalid_repeat");
    }

    #[test]
    fn reminder_adapter_simulation_uses_runtime_evaluator() {
        let adapter = ReminderAdapter;
        let effects = adapter.simulate(
            &serde_json::json!({
                "maxDelayHours": 1,
                "maxTextLength": 20,
                "timezone": "UTC",
                "allowRecurring": false,
                "maxRecurrences": 3,
            }),
            &serde_json::json!({
                "delayMs": 120_000,
                "reminderText": "short",
                "repeat": "daily",
                "timezone": "UTC",
            }),
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.contains("recurring_disabled"))
        );
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
    fn leaderboard_evaluator_sorts_bounds_and_respects_opt_outs() {
        let policy = LeaderboardPolicy {
            max_entries: 2,
            public: false,
        };
        let decision = evaluate_leaderboard(
            &policy,
            vec![
                LeaderboardEntry {
                    user_id: "low".into(),
                    xp: 10,
                    opted_out: false,
                },
                LeaderboardEntry {
                    user_id: "private".into(),
                    xp: 999,
                    opted_out: true,
                },
                LeaderboardEntry {
                    user_id: "top".into(),
                    xp: 100,
                    opted_out: false,
                },
                LeaderboardEntry {
                    user_id: "middle".into(),
                    xp: 50,
                    opted_out: false,
                },
            ],
        );
        assert!(!decision.public);
        assert_eq!(decision.excluded_opt_outs, 1);
        assert!(decision.truncated);
        assert_eq!(
            decision
                .entries
                .iter()
                .map(|entry| entry.user_id.as_str())
                .collect::<Vec<_>>(),
            vec!["top", "middle"]
        );
    }

    #[test]
    fn workflow_evaluator_matches_condition_and_sanitizes_mentions() {
        let decision = evaluate_workflow(
            &WorkflowPolicy {
                max_reply_length: 80,
                allow_mentions: false,
            },
            &WorkflowObservation {
                enabled: true,
                trigger: "message".into(),
                condition: "hello".into(),
                action: "reply".into(),
                payload: "Hi {user}: {message} @everyone".into(),
                message_content: "HELLO from the community".into(),
                user_mention: "<@42>".into(),
            },
        );
        assert!(decision.matched);
        assert!(decision.should_run);
        let reply = decision.reply.expect("matched workflow reply");
        assert!(reply.contains("<@42>"));
        assert!(reply.contains("@\u{200b}everyone"));
        assert!(!reply.contains("@everyone"));
    }

    #[test]
    fn workflow_adapter_simulation_uses_the_runtime_evaluator() {
        let adapter = feature_adapter("management.workflows").expect("workflow adapter");
        let effects = adapter.simulate(
            &serde_json::json!({"maxReplyLength": 20, "allowMentions": false}),
            &serde_json::json!({
                "workflow": {"condition":"ping", "action":"reply", "payload":"{user} ping @here"},
                "messageContent":"please PING",
                "userMention":"<@99>"
            }),
        );
        assert!(effects[0].contains("<@99> ping @\u{200b}here"));
        assert!(effects.iter().any(|effect| {
            effect.contains("management.workflows.max_reply_length") && effect.contains("20")
        }));
    }

    #[test]
    fn achievements_evaluator_returns_ordered_milestones() {
        let unlocked = evaluate_achievements(
            &AchievementPolicy {
                first_threshold: 100,
                regular_threshold: 500,
                pillar_threshold: 1_000,
            },
            600,
        );
        assert_eq!(
            unlocked.iter().map(|item| item.key).collect::<Vec<_>>(),
            vec!["first_steps", "regular"]
        );
    }

    #[test]
    fn achievements_adapter_simulation_uses_the_same_milestone_evaluator() {
        let adapter = feature_adapter("community.achievements").expect("achievements adapter");
        let effects = adapter.simulate(
            &serde_json::json!({
                "firstThreshold": 100,
                "regularThreshold": 500,
                "pillarThreshold": 1000
            }),
            &serde_json::json!({"xp": 600}),
        );
        assert!(effects[0].contains("First steps"));
        assert!(effects[0].contains("Regular"));
        assert!(!effects[0].contains("Community pillar"));
    }

    #[test]
    fn leaderboard_adapter_simulation_uses_the_same_decision_contract() {
        let adapter = feature_adapter("community.leaderboard").expect("leaderboard adapter");
        let effects = adapter.simulate(
            &serde_json::json!({"maxEntries": 1, "public": true}),
            &serde_json::json!({
                "leaderboardEntries": [
                    {"userId": "opted-out", "xp": 999, "optedOut": true},
                    {"userId": "alice", "xp": 42}
                ]
            }),
        );
        assert!(effects[0].contains("public XP leaderboard"));
        assert!(effects[0].contains("alice"));
        assert!(effects[0].contains("Excluded 1 member"));
        assert!(effects.iter().any(|effect| {
            effect.contains("community.leaderboard.max_entries") && effect.contains("1")
        }));
    }

    #[test]
    fn every_internal_feature_has_an_adapter_and_external_features_stay_blocked() {
        let internal = [
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
            "community.birthdays",
            "community.economy",
            "social.podcasts",
            "social.bluesky",
            "web3.crypto_stats",
            "web3.crypto_queries",
        ];
        for key in internal {
            assert!(feature_adapter(key).is_some(), "missing adapter for {key}");
            assert!(matches!(
                feature_maturity(key),
                FeatureMaturity::Operational | FeatureMaturity::Beta
            ));
        }
        for key in [
            "social.instagram",
            "social.reddit",
            "social.x",
            "social.tiktok",
            "social.kick",
            "growth.monetization",
            "web3.gating",
        ] {
            assert_eq!(feature_maturity(key), FeatureMaturity::Blocked);
            let adapter = feature_adapter(key).expect("blocked feature still has a contract");
            assert!(!adapter.descriptor().dependencies.is_empty());
            assert!(!adapter.descriptor().schema.as_object().unwrap().is_empty());
        }
        for key in [
            "web3.nft_stats",
            "web3.nft_queries",
            "web3.nft_sales",
            "web3.gas_tracker",
        ] {
            // The adapters and Discord commands are real, but the providers
            // remain Beta until their production credentials/endpoints are
            // configured and health checks succeed.
            assert_eq!(feature_maturity(key), FeatureMaturity::Beta);
        }
    }

    #[test]
    fn every_catalogue_key_has_a_runtime_adapter_or_an_explicit_blocker() {
        for key in FEATURE_KEYS {
            let maturity = feature_maturity(key);
            assert!(
                feature_adapter(key).is_some() || maturity == FeatureMaturity::Blocked,
                "{key} would otherwise be a silent no-op in the panel"
            );
        }
    }

    #[test]
    fn every_catalogue_adapter_has_a_bounded_preview_and_runtime_projection() {
        let fixture = serde_json::json!({
            "content": "preview message",
            "channelId": "123456789012345678",
            "reactionCount": 5,
            "joinCount": 10,
            "accountAgeDays": 30,
            "hasAvatar": true,
            "displayName": "Preview member"
        });

        for key in FEATURE_KEYS {
            let adapter = feature_adapter(key).expect("catalogue key must have an adapter");
            let descriptor = adapter.descriptor();
            assert_eq!(descriptor.key, *key, "adapter key drifted for {key}");
            assert_eq!(descriptor.schema_version, FEATURE_SCHEMA_VERSION);
            assert!(
                descriptor.schema.is_object(),
                "{key} must expose an object schema"
            );
            assert!(
                descriptor.defaults.is_object(),
                "{key} must expose object defaults"
            );

            // A preview is deliberately side-effect free, but it must still
            // explain what the runtime would do.  This catches adapters that
            // only persist JSON and would otherwise render a misleading
            // Configure button in the panel.
            let effects = adapter.simulate(&descriptor.defaults, &fixture);
            assert!(
                !effects.is_empty() && effects.iter().all(|effect| !effect.trim().is_empty()),
                "{key} has no usable runtime preview"
            );

            // Projection can be empty only for a toggle-only adapter.  Those
            // adapters are still covered by the non-empty simulate contract
            // above; configurable schemas must project at least one setting.
            if descriptor
                .schema
                .get("sections")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|sections| !sections.is_empty())
            {
                assert!(
                    !adapter.runtime_projection(&descriptor.defaults).is_empty(),
                    "{key} exposes fields but publishes no runtime projection"
                );
            }
        }
    }

    #[test]
    fn every_catalogue_preview_describes_an_actionable_runtime_decision() {
        let fixture = serde_json::json!({
            "content": "preview message",
            "channelId": "123456789012345678",
            "reactionCount": 5,
            "joinCount": 10,
            "accountAgeDays": 30,
            "hasAvatar": true,
            "displayName": "Preview member"
        });

        for key in FEATURE_KEYS {
            let adapter = feature_adapter(key).expect("catalogue key must have an adapter");
            let descriptor = adapter.descriptor();
            let effects = adapter.simulate(&descriptor.defaults, &fixture);
            assert!(
                effects.iter().all(|effect| {
                    !effect.contains("Validate and apply")
                        && !effect.contains("only persist")
                        && !effect.contains("guardar JSON")
                }),
                "{key} still exposes a generic persistence-only preview"
            );
            let projection = adapter.runtime_projection(&descriptor.defaults);
            if !projection.is_empty() {
                assert!(
                    effects
                        .iter()
                        .any(|effect| effect.contains("Runtime setting")),
                    "{key} preview does not expose the values consumed by the runtime"
                );
            }
        }
    }

    #[test]
    fn every_schema_field_changes_the_published_runtime_projection() {
        fn probe_value(
            kind: &str,
            current: Option<&serde_json::Value>,
            min: Option<i64>,
            max: Option<i64>,
        ) -> serde_json::Value {
            match current {
                Some(value) if value.is_boolean() => serde_json::json!(!value.as_bool().unwrap()),
                Some(value) if value.is_number() => {
                    let current = value.as_i64().unwrap_or_default();
                    let candidate = if min.is_none_or(|bound| current > bound) {
                        current.saturating_sub(1)
                    } else {
                        current.saturating_add(1)
                    };
                    let candidate = max.map_or(candidate, |bound| candidate.min(bound));
                    serde_json::json!(min.map_or(candidate, |bound| candidate.max(bound)))
                }
                Some(value) if value.is_array() => serde_json::json!(["123456789012345678"]),
                Some(value) if value.is_string() => {
                    serde_json::json!(format!("{}-probe", value.as_str().unwrap_or_default()))
                }
                Some(_) | None if kind == "toggle" => serde_json::json!(true),
                Some(_) | None if matches!(kind, "number" | "slider") => serde_json::json!(1),
                Some(_) | None if matches!(kind, "tags" | "channels" | "roles") => {
                    serde_json::json!(["123456789012345678"])
                }
                _ => serde_json::json!("probe"),
            }
        }

        for key in FEATURE_KEYS {
            let adapter = feature_adapter(key).expect("catalogue key must have an adapter");
            let descriptor = adapter.descriptor();
            let baseline = adapter.runtime_projection(&descriptor.defaults);
            let sections = descriptor.schema["sections"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for section in sections {
                for field in section["fields"].as_array().cloned().unwrap_or_default() {
                    let Some(field_key) = field["key"].as_str() else {
                        continue;
                    };
                    let kind = field["kind"].as_str().unwrap_or("text");
                    let min = field["min"].as_i64();
                    let max = field["max"].as_i64();
                    let mut candidate = descriptor.defaults.clone();
                    let object = candidate
                        .as_object_mut()
                        .expect("feature defaults must be a JSON object");
                    let current = object.get(field_key);
                    object.insert(field_key.into(), probe_value(kind, current, min, max));
                    let projected = adapter.runtime_projection(&candidate);
                    assert_ne!(
                        projected, baseline,
                        "{key}.{field_key} is exposed but does not change runtime projection"
                    );
                }
            }
        }
    }

    #[test]
    fn every_known_key_has_a_declared_lifecycle() {
        for key in FEATURE_KEYS {
            assert_ne!(
                feature_maturity(key),
                FeatureMaturity::Planned,
                "{key} must be operational, beta, or explicitly blocked"
            );
        }
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
            "logChannel": "not-a-discord-id",
            "alertOnly": "true"
        }));
        assert!(issues.iter().any(|issue| issue.path == "floodCount"));
        assert!(issues.iter().any(|issue| issue.path == "ignoredChannels"));
        assert!(issues.iter().any(|issue| issue.path == "logChannel"));
        assert!(issues.iter().any(|issue| issue.path == "alertOnly"));
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
    fn anti_spam_adapter_preview_uses_camel_and_snake_case_fixtures() {
        let adapter = feature_adapter("protection.antispam").expect("adapter registered");
        let config = serde_json::json!({
            "floodCount": 4,
            "duplicateLimit": 2,
            "mentionLimit": 3,
            "timeoutSeconds": 90,
            "logChannel": "987654321098765432"
        });
        for fixture in [
            serde_json::json!({
                "channelId": "general",
                "roleIds": [],
                "messageCount": 4,
                "duplicateCount": 2,
                "mentionCount": 3
            }),
            serde_json::json!({
                "channel_id": "general",
                "role_ids": [],
                "message_count": 4,
                "duplicate_count": 2,
                "mention_count": 3
            }),
        ] {
            let effects = adapter.simulate(&config, &fixture);
            assert!(effects[0].contains("90 second timeout"));
            assert!(effects.iter().any(|effect| effect.contains("log channel")));
            assert!(
                effects
                    .iter()
                    .any(|effect| effect.contains("security.antispam.flood_count=4"))
            );
        }
    }

    #[test]
    fn protection_adapters_preview_the_same_policies_as_the_gateway() {
        let scam = feature_adapter("protection.antiscam").unwrap();
        let scam_effects = scam.simulate(
            &serde_json::json!({"blockedKeywords": ["free nitro"], "timeoutSeconds": 120}),
            &serde_json::json!({"channel_id": "general", "content": "free nitro"}),
        );
        assert!(scam_effects[0].contains("timeout: 120s"));

        let raid = feature_adapter("protection.anti_raid").unwrap();
        let raid_effects = raid.simulate(
            &serde_json::json!({"joinThreshold": 4, "windowSeconds": 12, "alertOnly": true}),
            &serde_json::json!({"join_count": 4}),
        );
        assert!(raid_effects[0].contains("Monitor the burst"));
        assert!(raid_effects[0].contains("4/4 joins in 12s"));

        let gate = feature_adapter("protection.join_gate").unwrap();
        let gate_effects = gate.simulate(
            &serde_json::json!({"minimumAccountDays": 7, "requireAvatar": true, "blockedNamePatterns": ["raid"]}),
            &serde_json::json!({"account_age_days": 1, "has_avatar": false, "display_name": "raid account"}),
        );
        assert!(gate_effects[0].contains("join-gate action"));

        let starboard = feature_adapter("community.starboard").unwrap();
        let starboard_effects = starboard.simulate(
            &serde_json::json!({"threshold": 5}),
            &serde_json::json!({"reaction_count": 3}),
        );
        assert!(starboard_effects[0].contains("below the board threshold (3/5"));
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
                    "autoRole": "456",
                    "templateId": "welcome-1"
                }))
                .contains(&("support.welcome.send_dm".into(), "true".into()))
        );
        assert!(
            welcome
                .runtime_projection(&serde_json::json!({"templateId": "welcome-1"}))
                .contains(&("support.welcome.template_id".into(), "welcome-1".into()))
        );

        for key in [
            "community.suggestions",
            "community.giveaways",
            "management.polls",
            "community.events",
            "community.role_panels",
        ] {
            let adapter = feature_adapter(key).expect("community adapter registered");
            assert!(
                adapter.descriptor().schema["sections"][0]["fields"]
                    .as_array()
                    .is_some_and(|fields| !fields.is_empty())
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
        let rss = feature_adapter("social.rss").expect("rss adapter");
        let feed_projection = rss.runtime_projection(&serde_json::json!({
            "feedUrl": "https://example.com/feed.xml",
            "targetChannelId": "123",
            "intervalSeconds": 600,
            "messageTemplate": "New: {title}",
            "mention": ""
        }));
        assert!(feed_projection.contains(&("social.feed.target_channel_id".into(), "123".into())));
        assert!(feed_projection.contains(&("social.feed.interval_seconds".into(), "600".into())));

        let youtube = feature_adapter("social.youtube").expect("youtube adapter");
        let youtube_descriptor = youtube.descriptor();
        let youtube_fields = youtube_descriptor.schema["sections"][0]["fields"]
            .as_array()
            .expect("youtube schema fields");
        assert!(
            youtube_fields
                .iter()
                .any(|field| field["key"] == "sourceChannelId")
        );
        assert!(
            youtube_fields
                .iter()
                .any(|field| field["key"] == "targetChannelId")
        );
        assert!(
            youtube
                .validate(&serde_json::json!({}))
                .iter()
                .any(|issue| issue.path == "sourceChannelId")
        );
        assert!(
            youtube
                .validate(&serde_json::json!({
                    "sourceChannelId": "UC123",
                    "targetChannelId": "123456789012345678",
                    "intervalSeconds": 900,
                    "messageTemplate": "New video: {title}"
                }))
                .is_empty()
        );

        let twitch = feature_adapter("social.twitch").expect("twitch adapter");
        let twitch_descriptor = twitch.descriptor();
        let twitch_fields = twitch_descriptor.schema["sections"][0]["fields"]
            .as_array()
            .expect("twitch schema fields");
        assert!(
            twitch_fields
                .iter()
                .any(|field| field["key"] == "sourceLogin")
        );
        assert!(
            twitch
                .runtime_projection(&serde_json::json!({
                    "sourceLogin": "vozen",
                    "targetChannelId": "987654321098765432",
                    "intervalSeconds": 1800
                }))
                .contains(&("social_twitch.sourceLogin".into(), "vozen".into()))
        );
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
            "shadowMode": true,
            "logChannel": "123",
            "includeContent": true
        }));
        assert!(audit_projection.contains(&("security.anti_nuke.actions".into(), "5".into())));
        assert!(audit_projection.contains(&("security.shadow_mode".into(), "true".into())));
        assert!(audit_projection.contains(&("management.audit.log_channel".into(), "123".into())));
        assert!(
            audit_projection.contains(&("management.audit.include_content".into(), "true".into()))
        );
        assert!(
            audit
                .validate(&serde_json::json!({"logChannel": "not-a-channel"}))
                .iter()
                .any(|issue| issue.path == "logChannel")
        );

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
        assert_eq!(descriptor.defaults["timezone"], "UTC");
        assert!(adapter
            .validate(&serde_json::json!({"maxDelayHours": 0, "maxTextLength": 500, "notifyUser": true}))
            .iter()
            .any(|issue| issue.path == "maxDelayHours"));
        assert!(
            adapter
                .validate(&serde_json::json!({"timezone": "Europe/Lisbon"}))
                .iter()
                .any(|issue| issue.code == "invalid_timezone")
        );
        let projection = adapter.runtime_projection(&serde_json::json!({
            "maxDelayHours": 24,
            "maxTextLength": 240,
            "timezone": "UTC+01:00",
            "notifyUser": false,
            "allowRecurring": true,
            "maxRecurrences": 6
        }));
        assert!(projection.contains(&("utility.reminders.max_delay_hours".into(), "24".into())));
        assert!(projection.contains(&("utility.reminders.max_text_length".into(), "240".into())));
        assert!(projection.contains(&("utility.reminders.timezone".into(), "UTC+01:00".into())));
        assert!(projection.contains(&("utility.reminders.notify_user".into(), "false".into())));
        assert!(projection.contains(&("utility.reminders.allow_recurring".into(), "true".into())));
        assert!(projection.contains(&("utility.reminders.max_recurrences".into(), "6".into())));
        assert_eq!(
            feature_maturity("utility.reminders"),
            FeatureMaturity::Operational
        );
    }

    #[test]
    fn reminder_timezones_are_bounded_fixed_offsets() {
        assert_eq!(parse_utc_offset_minutes("UTC"), Some(0));
        assert_eq!(parse_utc_offset_minutes("UTC+05:30"), Some(330));
        assert_eq!(parse_utc_offset_minutes("GMT-05"), Some(-300));
        assert_eq!(parse_utc_offset_minutes("UTC+14:30"), None);
        assert_eq!(parse_utc_offset_minutes("Europe/Lisbon"), None);
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
    fn moderation_evaluator_requires_reasons_and_clamps_purge() {
        let policy = ModerationPolicy {
            require_reason: true,
            max_purge: 12,
        };
        let missing_reason = evaluate_moderation(
            &policy,
            &ModerationObservation {
                action: "ban".into(),
                reason: " ".into(),
                requested_count: None,
            },
        );
        assert!(!missing_reason.allowed);
        assert_eq!(missing_reason.reason_code, "reason_required");

        let purge = evaluate_moderation(
            &policy,
            &ModerationObservation {
                action: "purge".into(),
                reason: String::new(),
                requested_count: Some(50),
            },
        );
        assert!(purge.allowed);
        assert_eq!(purge.effective_count, Some(12));
        assert_eq!(purge.reason_code, "count_clamped");
    }

    #[test]
    fn moderation_adapter_simulation_uses_runtime_evaluator() {
        let adapter = feature_adapter("management.moderation").expect("moderation adapter");
        let blocked = adapter.simulate(
            &serde_json::json!({"requireReason": true, "maxPurge": 12}),
            &serde_json::json!({"action": "ban", "reason": ""}),
        );
        assert!(
            blocked
                .iter()
                .any(|effect| effect.contains("reason_required"))
        );

        let clamped = adapter.simulate(
            &serde_json::json!({"requireReason": true, "maxPurge": 12}),
            &serde_json::json!({"action": "purge", "requestedCount": 50}),
        );
        assert!(
            clamped
                .iter()
                .any(|effect| effect.contains("effective limit of 12"))
        );
        assert!(
            clamped
                .iter()
                .any(|effect| effect.contains("count_clamped"))
        );
    }

    #[test]
    fn welcome_renderer_bounds_templates_and_mentions() {
        let rendered =
            render_member_message("Hello {member} in {server}: @everyone", "<@42>", "Vozen");
        assert_eq!(rendered, "Hello <@42> in Vozen: @\u{200b}everyone");
        assert!(
            render_member_message(&"x".repeat(2_500), "m", "s")
                .chars()
                .count()
                <= 2_000
        );
    }

    #[test]
    fn welcome_adapter_simulation_uses_rendered_runtime_message() {
        let adapter = feature_adapter("support.welcome").expect("welcome adapter");
        let effects = adapter.simulate(
            &serde_json::json!({
                "message": "Welcome {member} to {server}! @everyone",
                "sendDm": true,
                "dmMessage": "Read the rules, {member}.",
                "delaySeconds": 5,
                "autoRole": "123"
            }),
            &serde_json::json!({"member": "<@42>", "server": "Vozen"}),
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.contains("@\u{200b}everyone"))
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.contains("after 5 second(s)"))
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.contains("automatic role `123`"))
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
        assert!(
            adapter
                .validate(&serde_json::json!({"ignoredRoles": ["staff"]}))
                .iter()
                .any(|issue| issue.path == "ignoredRoles")
        );
        let policy = scam_policy_from_json(&serde_json::json!({
            "blockInvites": true,
            "blockedDomains": ["example.invalid"],
            "blockedKeywords": ["free coins"],
            "ignoredRoles": ["777"],
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
        assert!(
            evaluate_scam(&policy, "123", "https://example.invalid.evil.test/landing")
                .matched
                .iter()
                .all(|value| !value.starts_with("domain:"))
        );
        assert!(
            evaluate_scam_with_roles(&policy, "123", &["777".into()], "https://example.invalid")
                .ignored
        );
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
        assert!(
            adapter
                .validate(&serde_json::json!({
                    "channelId": "123",
                    "message": "Hi",
                    "steps": ["rules", "rules"]
                }))
                .iter()
                .any(|issue| issue.path == "steps")
        );
        let projection = adapter.runtime_projection(&serde_json::json!({
            "channelId": "123",
            "message": "Hi",
            "steps": ["rules", "help"],
            "rulesChannel": "456"
        }));
        assert!(
            projection.contains(&("support.welcome_channel.steps".into(), "rules,help".into()))
        );
        assert!(
            projection.contains(&("support.welcome_channel.rules_channel".into(), "456".into()))
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
        assert!(
            starboard
                .runtime_projection(&serde_json::json!({"ignoredRoles": ["456"]}))
                .contains(&("community.starboard.ignored_roles".into(), "456".into()))
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
                "pauseInvites": false,
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
        assert!(
            raid.runtime_projection(&serde_json::json!({"pauseInvites": false}))
                .contains(&("security.anti_raid.pause_invites".into(), "false".into()))
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
                .validate(&serde_json::json!({
                    "windowDays": 31,
                    "public": false,
                    "channelId": "not-a-channel",
                    "intervalMinutes": 2,
                    "nameTemplate": ""
                }))
                .iter()
                .any(|issue| issue.path == "windowDays")
        );
        assert!(stats
            .validate(&serde_json::json!({"channelId": "123", "intervalMinutes": 15, "nameTemplate": "messages-{messages}"}))
            .is_empty());
        assert!(
            stats
                .runtime_projection(&serde_json::json!({
                    "channelId": "123",
                    "intervalMinutes": 30,
                    "nameTemplate": "msgs-{messages}"
                }))
                .contains(&("insights.stats.channel_id".into(), "123".into()))
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

    #[test]
    fn bounded_search_adapter_rejects_disabled_provider_set() {
        let search = feature_adapter("utility.search").expect("search adapter");
        assert_eq!(search.descriptor().source, "bounded_search_adapter_v1");
        assert!(
            search
                .validate(&serde_json::json!({
                    "maxResults": 5,
                    "allowWikipedia": false,
                    "allowAniList": false,
                    "allowBluesky": false
                }))
                .iter()
                .any(|issue| issue.code == "provider_required")
        );
        assert!(
            search
                .runtime_projection(&serde_json::json!({
                    "maxResults": 3,
                    "allowWikipedia": true,
                    "allowAniList": false,
                    "allowBluesky": true
                }))
                .contains(&("utility.search.max_results".into(), "3".into()))
        );
        assert!(
            search
                .runtime_projection(&serde_json::json!({
                    "maxResults": 3,
                    "allowWikipedia": false,
                    "allowAniList": false,
                    "allowBluesky": true
                }))
                .contains(&("utility.search.allow_bluesky".into(), "true".into()))
        );
    }

    #[test]
    fn reddit_adapter_keeps_oauth_and_commercial_gate_explicit() {
        let reddit = feature_adapter("social.reddit").expect("reddit adapter");
        let descriptor = reddit.descriptor();
        assert_eq!(descriptor.source, "reddit_oauth_readonly_v1");
        assert!(
            descriptor
                .dependencies
                .iter()
                .any(|item| item == "Reddit OAuth application")
        );
        assert!(
            descriptor
                .dependencies
                .iter()
                .any(|item| item == "Commercial API approval")
        );
        assert!(
            reddit
                .validate(&serde_json::json!({
                    "sourceSubreddit": "https://reddit.com/r/vozen",
                    "targetChannelId": "123456789012345678",
                    "intervalSeconds": 900,
                    "messageTemplate": "{title}"
                }))
                .iter()
                .any(|issue| issue.code == "invalid_subreddit")
        );
        assert!(
            reddit
                .runtime_projection(&serde_json::json!({
                    "sourceSubreddit": "vozen",
                    "targetChannelId": "123456789012345678",
                    "intervalSeconds": 900,
                    "messageTemplate": "{title}",
                    "mention": ""
                }))
                .contains(&("social.reddit.interval_seconds".into(), "900".into()))
        );
        assert_eq!(feature_maturity("social.reddit"), FeatureMaturity::Blocked);
    }

    #[test]
    fn gas_and_nft_adapters_keep_provider_boundaries_explicit() {
        let gas = feature_adapter("web3.gas_tracker").expect("gas adapter");
        assert_eq!(gas.descriptor().source, "operator_rpc_gas_v1");
        assert!(
            gas.validate(&serde_json::json!({
                "network": "mainnet",
                "targetChannelId": "123",
                "intervalSeconds": 900
            }))
            .iter()
            .any(|issue| issue.path == "network")
        );
        assert!(
            gas.runtime_projection(&serde_json::json!({
                "network": "ethereum",
                "targetChannelId": "123",
                "intervalSeconds": 600
            }))
            .contains(&("web3.gas_tracker.network".into(), "ethereum".into()))
        );

        for key in ["web3.nft_stats", "web3.nft_queries", "web3.nft_sales"] {
            let nft = feature_adapter(key).expect("NFT adapter");
            assert_eq!(nft.descriptor().source, "opensea_read_only_v1");
            assert!(
                nft.validate(
                    &serde_json::json!({"collectionSlug": "https://opensea.io/collection/x"})
                )
                .iter()
                .any(|issue| issue.path == "collectionSlug")
            );
            // The adapter is implemented and read-only; availability is
            // dependency-gated by the OpenSea key, so the catalogue exposes
            // it as beta rather than silently pretending it is blocked.
            assert_eq!(feature_maturity(key), FeatureMaturity::Beta);
        }
    }

    #[test]
    fn anti_raid_evaluator_is_shared_and_respects_shadow_mode() {
        let policy = anti_raid_policy_from_json(&serde_json::json!({
            "joinThreshold": 5,
            "windowSeconds": 12,
            "incidentMinutes": 20,
            "alertOnly": false,
            "pauseInvites": true
        }));
        let below = evaluate_anti_raid(&policy, 4, false);
        assert!(!below.armed);
        assert_eq!(below.reason, "join_burst_below_threshold");

        let contained = evaluate_anti_raid(&policy, 5, false);
        assert!(contained.armed);
        assert!(!contained.shadow_mode);
        assert!(contained.should_contain);
        assert_eq!(contained.incident_minutes, 20);

        let shadow = evaluate_anti_raid(&policy, 5, true);
        assert!(shadow.armed);
        assert!(shadow.shadow_mode);
        assert!(!shadow.should_contain);
        assert_eq!(shadow.reason, "join_burst_detected_shadow");

        let alert_only = anti_raid_policy_from_json(&serde_json::json!({
            "joinThreshold": 5,
            "pauseInvites": false
        }));
        let alert = evaluate_anti_raid(&alert_only, 5, false);
        assert!(alert.armed);
        assert!(!alert.shadow_mode);
        assert!(!alert.should_contain);
        assert_eq!(alert.reason, "join_burst_detected_alert");
    }

    #[test]
    fn join_gate_evaluator_explains_all_blocking_reasons() {
        let policy = JoinGatePolicy {
            minimum_account_days: 7,
            require_avatar: true,
            blocked_name_patterns: vec!["free nitro".into()],
            action: "quarantine".into(),
        };
        let decision = evaluate_join_gate(
            &policy,
            &JoinGateObservation {
                account_age_days: 1,
                has_avatar: false,
                display_name: "Free Nitro winner".into(),
            },
        );
        assert!(decision.blocked);
        assert_eq!(decision.action, "quarantine");
        assert_eq!(decision.reasons.len(), 3);

        let allowed = evaluate_join_gate(
            &policy,
            &JoinGateObservation {
                account_age_days: 30,
                has_avatar: true,
                display_name: "Community member".into(),
            },
        );
        assert!(!allowed.blocked);
        assert!(allowed.reasons.is_empty());
    }

    #[test]
    fn starboard_evaluator_matches_threshold_and_exemptions() {
        let policy = starboard_policy_from_json(&serde_json::json!({
            "threshold": 3,
            "allowSelfStar": false,
            "includeImages": false,
            "ignoredChannels": ["999"],
            "ignoredRoles": ["777"]
        }));
        let decision = evaluate_starboard(
            &policy,
            &StarboardObservation {
                source_channel_id: "123".into(),
                author_id: "1".into(),
                reactor_ids: vec!["1".into(), "2".into(), "2".into(), "3".into(), "4".into()],
                author_role_ids: vec![],
                has_attachments: false,
            },
        );
        assert_eq!(decision.count, 3);
        assert!(decision.should_publish);
        assert!(!decision.ignored);

        let ignored = evaluate_starboard(
            &policy,
            &StarboardObservation {
                source_channel_id: "999".into(),
                author_id: "1".into(),
                reactor_ids: vec!["2".into(), "3".into(), "4".into()],
                author_role_ids: vec![],
                has_attachments: false,
            },
        );
        assert!(ignored.ignored);
        assert!(!ignored.should_publish);

        let image = evaluate_starboard(
            &policy,
            &StarboardObservation {
                source_channel_id: "123".into(),
                author_id: "1".into(),
                reactor_ids: vec!["2".into(), "3".into(), "4".into()],
                author_role_ids: vec![],
                has_attachments: true,
            },
        );
        assert!(image.ignored);
        assert!(image.reason.contains("attachments"));
    }

    #[test]
    fn role_panel_adapter_projects_web_selected_roles() {
        let adapter = feature_adapter("community.role_panels").expect("role panel adapter");
        let config = serde_json::json!({
            "channel": "123456789012345678",
            "roleIds": ["111111111111111111", "222222222222222222"],
            "panelTitle": "Pick your alerts",
            "panelDescription": "Choose what you want to receive.",
            "maxRoles": 2,
            "selectionMode": "unique",
            "removeOnUnselect": true
        });
        assert!(adapter.validate(&config).is_empty());
        assert!(adapter.runtime_projection(&config).contains(&(
            "community.role_panels.role_ids".into(),
            "111111111111111111,222222222222222222".into()
        )));
        assert!(adapter.runtime_projection(&config).contains(&(
            "community.role_panels.selection_mode".into(),
            "unique".into()
        )));
        assert!(
            adapter
                .validate(&serde_json::json!({"roleIds": ["not-a-discord-id"]}))
                .iter()
                .any(|issue| issue.code == "invalid_role_ids")
        );
        assert!(
            adapter
                .validate(&serde_json::json!({"selectionMode": "unbounded"}))
                .iter()
                .any(|issue| issue.code == "invalid_selection_mode")
        );
    }
}
