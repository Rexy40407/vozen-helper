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

/// Every configurable feature will eventually implement this contract. The
/// first adapter is intentionally small: it proves that the API and gateway
/// can consume one canonical schema/defaults/validator without pulling UI
/// concerns into the Discord crate.
pub trait FeatureAdapter: Sync {
    fn descriptor(&self) -> FeatureAdapterDescriptor;
    fn validate(&self, config: &serde_json::Value) -> Vec<ValidationIssue>;
    fn runtime_projection(&self, config: &serde_json::Value) -> Vec<(String, String)>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AntiSpamAdapter;

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

static ANTI_SPAM_ADAPTER: AntiSpamAdapter = AntiSpamAdapter;

pub fn feature_adapter(key: &str) -> Option<&'static dyn FeatureAdapter> {
    (key == AntiSpamAdapter::KEY).then_some(&ANTI_SPAM_ADAPTER as &dyn FeatureAdapter)
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
        assert!(feature_adapter("community.levels").is_none());
    }
}
