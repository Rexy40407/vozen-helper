//! Versioned API and compatibility routes for the existing panel.

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use helper_contracts::Plan;
use helper_contracts::{ApiError, SessionClaims};
use helper_core::{Capability, quota_limit};
use helper_modules::EntitlementClient;
use helper_store::Store;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const COOKIE: &str = "vh_session";
const SESSION_MAX_HOURS: i64 = 8;
const IDLE_MINUTES: i64 = 30;

#[derive(Clone)]
pub struct ApiState {
    pub store: Store,
    pub session_secret: String,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub oauth_redirect_uri: String,
    pub allow_legacy_session: bool,
    pub allowed_origin: Option<String>,
    pub entitlements: Option<EntitlementClient>,
}

#[derive(Debug, Serialize)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub timestamp: String,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/health", get(health))
        .route("/api/session", post(create_session))
        .route("/api/oauth/start", get(oauth_start))
        .route("/api/oauth/callback", get(oauth_callback))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/guilds", get(guilds))
        .route("/api/cases", get(cases))
        .route("/api/tickets", get(tickets))
        .route("/api/stats", get(stats))
        .route("/api/quotas", get(quotas))
        .route("/api/modules", get(modules))
        .route("/api/permissions", get(permissions))
        .route("/api/security/health", get(security_health))
        .route("/api/analytics", get(analytics))
        .route("/api/privacy/export", get(privacy_export))
        .route("/api/privacy/receipt", get(privacy_receipt))
        .route("/api/privacy/delete", post(privacy_delete))
        .route("/api/config/import", post(import_config))
        .route("/api/workflows", get(workflows).post(create_workflow))
        .route("/api/workflows/{id}", delete(delete_workflow))
        .with_state(Arc::new(state))
}

#[derive(Debug, Deserialize)]
struct OAuthStartQuery {
    guild_id: String,
    code_challenge: String,
}

#[derive(Debug, Serialize)]
struct OAuthStartResponse {
    authorization_url: String,
    state: String,
}

async fn oauth_start(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<OAuthStartQuery>,
) -> Result<Json<OAuthStartResponse>, (StatusCode, Json<ApiError>)> {
    if query.guild_id.trim().is_empty() || query.code_challenge.trim().len() < 43 {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "invalid_oauth_request",
        ));
    }
    let expires = (Utc::now() + Duration::minutes(10)).timestamp();
    let payload = format!("{}.{}.{}", query.guild_id, expires, query.code_challenge);
    let state_token = sign_oauth_state(&payload, &state.session_secret);
    let state_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(state_token.as_bytes()));
    state
        .store
        .register_oauth_state(&state_hash, expires)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    let mut url = url::Url::parse("https://discord.com/oauth2/authorize").expect("static URL");
    url.query_pairs_mut()
        .append_pair("client_id", &state.oauth_client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &state.oauth_redirect_uri)
        .append_pair("scope", "identify guilds")
        .append_pair("state", &state_token)
        .append_pair("code_challenge", &query.code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(Json(OAuthStartResponse {
        authorization_url: url.into(),
        state: state_token,
    }))
}

#[derive(Debug, Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    state: String,
    code_verifier: String,
}

async fn oauth_callback(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let payload = verify_oauth_state(&query.state, &state.session_secret)
        .ok_or_else(|| client_error(StatusCode::BAD_REQUEST, "invalid_oauth_state"))?;
    let parts: Vec<&str> = payload.splitn(3, '.').collect();
    if parts.len() != 3
        || parts[1]
            .parse::<i64>()
            .ok()
            .is_none_or(|exp| exp < Utc::now().timestamp())
    {
        return Err(client_error(StatusCode::BAD_REQUEST, "expired_oauth_state"));
    }
    let state_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(query.state.as_bytes()));
    let state_is_fresh = state
        .store
        .consume_oauth_state(&state_hash, Utc::now().timestamp())
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    if !state_is_fresh {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "oauth_state_replayed",
        ));
    }
    let guild_id = parts[0].to_string();
    let client = Client::new();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(query.code_verifier.as_bytes()));
    if challenge != parts[2] {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "pkce_verifier_mismatch",
        ));
    }
    let token = client
        .post("https://discord.com/api/v10/oauth2/token")
        .form(&[
            ("client_id", state.oauth_client_id.as_str()),
            ("client_secret", state.oauth_client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", query.code.as_str()),
            ("redirect_uri", state.oauth_redirect_uri.as_str()),
            ("code_verifier", query.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "discord_unreachable"))?;
    if !token.status().is_success() {
        return Err(client_error(
            StatusCode::UNAUTHORIZED,
            "oauth_exchange_failed",
        ));
    }
    let token: serde_json::Value = token
        .json()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "invalid_discord_response"))?;
    let access_token = token
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if access_token.is_empty() {
        return Err(client_error(
            StatusCode::UNAUTHORIZED,
            "oauth_token_missing",
        ));
    }
    create_session_inner(
        State(state),
        headers,
        Json(SessionRequest {
            token: access_token.to_string(),
            guild_id,
        }),
    )
    .await
}

async fn health() -> impl IntoResponse {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        timestamp: Utc::now().to_rfc3339(),
    })
}

#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    pub token: String,
    pub guild_id: String,
}
#[derive(Debug, Deserialize, Serialize)]
struct DiscordUser {
    id: String,
    username: String,
    global_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct DiscordGuild {
    id: String,
    name: String,
    permissions: Option<String>,
}
#[derive(Debug, Serialize)]
struct SessionResponse {
    ok: bool,
    user: DiscordUser,
    token: String,
    expires_at: String,
}

async fn create_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(req): Json<SessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    if !state.allow_legacy_session {
        return Err(client_error(StatusCode::GONE, "oauth_required"));
    }
    create_session_inner(State(state), headers, Json(req)).await
}

async fn create_session_inner(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(req): Json<SessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    if req.token.trim().is_empty() || req.guild_id.trim().is_empty() {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "missing_token_or_guild",
        ));
    }
    let client = Client::new();
    let user = client
        .get("https://discord.com/api/v10/users/@me")
        .bearer_auth(&req.token)
        .send()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "discord_unreachable"))?;
    if !user.status().is_success() {
        return Err(client_error(StatusCode::UNAUTHORIZED, "invalid_token"));
    }
    let discord_user: DiscordUser = user
        .json()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "invalid_discord_response"))?;
    let guilds = client
        .get("https://discord.com/api/v10/users/@me/guilds")
        .bearer_auth(&req.token)
        .send()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "discord_unreachable"))?;
    let guilds: Vec<DiscordGuild> = guilds
        .json()
        .await
        .map_err(|_| client_error(StatusCode::BAD_GATEWAY, "invalid_discord_response"))?;
    let can_manage = guilds
        .iter()
        .find(|guild| guild.id == req.guild_id)
        .map(|guild| {
            guild
                .permissions
                .as_deref()
                .is_some_and(has_manage_permission)
        })
        .unwrap_or(false);
    if !can_manage {
        return Err(client_error(StatusCode::FORBIDDEN, "guild_not_managed"));
    }
    let now = Utc::now();
    let claims = SessionClaims {
        session_id: Uuid::new_v4(),
        user_id: discord_user.id.clone(),
        guild_id: req.guild_id,
        issued_at: now,
        expires_at: now + Duration::hours(SESSION_MAX_HOURS),
        last_seen_at: now,
    };
    state
        .store
        .save_session(&claims)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    let managed_guilds = guilds
        .iter()
        .filter(|guild| {
            guild
                .permissions
                .as_deref()
                .is_some_and(has_manage_permission)
        })
        .map(|guild| {
            (
                guild.id.clone(),
                guild.name.clone(),
                guild.permissions.clone(),
            )
        })
        .collect::<Vec<_>>();
    state
        .store
        .replace_session_guilds(claims.session_id, &managed_guilds)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    let token = sign_session(&claims, &state.session_secret);
    let mut response = Json(SessionResponse {
        ok: true,
        user: discord_user,
        token: token.clone(),
        expires_at: claims.expires_at.to_rfc3339(),
    })
    .into_response();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && state
            .allowed_origin
            .as_deref()
            .is_some_and(|allowed| allowed == origin)
    {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.parse().unwrap());
    }
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!(
            "{COOKIE}={token}; HttpOnly; Secure; SameSite=None; Path=/; Max-Age={}",
            SESSION_MAX_HOURS * 3600
        )
        .parse()
        .unwrap(),
    );
    Ok(response)
}

async fn logout(State(state): State<Arc<ApiState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(claims) = authenticate(&state, &headers) {
        let _ = state.store.revoke_session(claims.session_id);
    }
    (
        [(
            header::SET_COOKIE,
            format!("{COOKIE}=; HttpOnly; Secure; SameSite=None; Path=/; Max-Age=0"),
        )],
        Json(serde_json::json!({"ok":true})),
    )
}

async fn me(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    Ok(Json(
        serde_json::json!({"id":claims.user_id,"guildId":claims.guild_id,"expiresAt":claims.expires_at,"dbOk":true}),
    ))
}

async fn guilds(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let guilds = state
        .store
        .session_guilds(claims.session_id)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(serde_json::json!({
        "guilds": guilds
            .into_iter()
            .map(|guild| serde_json::json!({"id": guild.guild_id, "name": guild.name, "canManage": true}))
            .collect::<Vec<_>>()
    })))
}

#[derive(Debug, Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}
async fn cases(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let list = state
        .store
        .recent_cases(&claims.guild_id, query.limit.unwrap_or(50).min(200))
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(serde_json::json!({"cases":list})))
}
async fn stats(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let cases = state
        .store
        .recent_cases(&claims.guild_id, 200)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(
        serde_json::json!({"totalCases":cases.len(),"guildId":claims.guild_id}),
    ))
}

async fn tickets(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let list = state
        .store
        .tickets_for_guild(&claims.guild_id, query.limit.unwrap_or(50).min(200))
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(
        serde_json::json!({"guildId": claims.guild_id, "tickets": list}),
    ))
}

async fn quotas(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let plan = effective_plan(&state, &claims).await;
    let keys = [
        "panels",
        "forms",
        "role_panels",
        "workflows",
        "workflow_runs",
        "feeds",
        "templates",
        "analytics_days",
        "audit_days",
        "transcript_days",
        "personal_drafts",
        "personal_views",
        "personal_templates",
    ];
    let limits = keys
        .into_iter()
        .map(|key| (key, quota_limit(&plan, key)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let usage = std::collections::BTreeMap::from([
        (
            "panels",
            state
                .store
                .count_settings_prefix(&claims.guild_id, "support.panel.")
                .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?,
        ),
        (
            "role_panels",
            state
                .store
                .count_settings_prefix(&claims.guild_id, "community.role_panel.")
                .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?,
        ),
        (
            "workflows",
            state
                .store
                .workflows(&claims.guild_id, u32::MAX)
                .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?
                .len() as u64,
        ),
    ]);
    Ok(Json(serde_json::json!({
        "plan": plan,
        "guildLimit": plan.guild_limit(),
        "limits": limits,
        "usage": usage,
    })))
}

async fn effective_plan(state: &ApiState, claims: &SessionClaims) -> Plan {
    if let Some(client) = &state.entitlements
        && let Ok(snapshot) = client
            .resolve(&claims.user_id, Some(&claims.guild_id))
            .await
        && snapshot.active
        && snapshot
            .expires_at
            .is_none_or(|expires_at| expires_at > Utc::now())
    {
        return snapshot.plan;
    }
    state
        .store
        .load_entitlement(&claims.user_id)
        .ok()
        .flatten()
        .filter(|snapshot| {
            snapshot.active
                && snapshot
                    .expires_at
                    .is_none_or(|expires_at| expires_at > Utc::now())
        })
        .map(|snapshot| snapshot.plan)
        .unwrap_or(Plan::Free)
}

async fn modules(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let _ = require_auth(&state, &headers)?;
    let modules = [
        ("core", Capability::Core),
        ("studio", Capability::Studio),
        ("security", Capability::Security),
        ("support", Capability::Support),
        ("events", Capability::Events),
        ("community", Capability::Community),
        ("automate", Capability::Automate),
        ("insights", Capability::Insights),
    ];
    Ok(Json(serde_json::json!({
        "modules": modules.into_iter().map(|(name, _)| name).collect::<Vec<_>>()
    })))
}

async fn permissions(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let _ = require_auth(&state, &headers)?;
    Ok(Json(permission_passport_document()))
}

async fn security_health(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let guild_id = &claims.guild_id;
    let setting = |key: &str| {
        state
            .store
            .get_setting(guild_id, key)
            .ok()
            .flatten()
            .unwrap_or_else(|| "false".into())
    };
    let mut findings = Vec::new();
    let mut penalty = 0_u64;
    for (key, label, severity, points, remediation) in [
        (
            "security.anti_raid.enabled",
            "Anti-raid desativado",
            "high",
            25,
            "Ativa anti-raid com /anti-raid e testa primeiro em shadow mode.",
        ),
        (
            "security.anti_nuke.enabled",
            "Anti-nuke desativado",
            "high",
            25,
            "Ativa anti-nuke com limiar conservador e mantém revisão humana.",
        ),
        (
            "security.join_gate.enabled",
            "Join gate desativado",
            "medium",
            15,
            "Configura um cargo de verificação e ativa o join gate.",
        ),
    ] {
        let value = setting(key);
        if value != "true" {
            penalty += points;
            findings.push(serde_json::json!({
                "id": key,
                "label": label,
                "severity": severity,
                "points": points,
                "evidence": {"setting": key, "value": value},
                "remediation": remediation
            }));
        }
    }
    let shadow_mode = setting("security.shadow_mode") == "true";
    if shadow_mode {
        findings.push(serde_json::json!({
            "id": "security.shadow_mode",
            "label": "Shadow mode ativo",
            "severity": "info",
            "points": 0,
            "evidence": {"setting": "security.shadow_mode", "value": "true"},
            "remediation": "Desativa depois de rever falsos positivos e confirmar a política."
        }));
    }
    Ok(Json(serde_json::json!({
        "version": 1,
        "guildId": guild_id,
        "score": 100_u64.saturating_sub(penalty),
        "findings": findings,
        "method": "deterministic_settings_v1",
        "limitations": ["Discord role hierarchy, channel overwrites and backup restore require a live diagnostic check"]
    })))
}

fn permission_passport_document() -> serde_json::Value {
    serde_json::json!({
        "permissions": [
            {
                "key": "VIEW_CHANNEL",
                "scope": "base",
                "modules": ["Core", "Support", "Events", "Community"],
                "risk": "low",
                "data": "channel visibility and metadata",
                "missingConsequence": "the Helper cannot inspect or respond in that channel"
            },
            {
                "key": "SEND_MESSAGES",
                "scope": "base",
                "modules": ["Core", "Support", "Events", "Community"],
                "risk": "low",
                "data": "bot responses and operational notices",
                "missingConsequence": "the Helper can detect a flow but cannot notify members"
            },
            {
                "key": "READ_MESSAGE_HISTORY",
                "scope": "base",
                "modules": ["Core", "Support", "Insights"],
                "risk": "low",
                "data": "message history needed for transcripts and context",
                "missingConsequence": "transcripts and historical context are incomplete"
            },
            {
                "key": "EMBED_LINKS",
                "scope": "base",
                "modules": ["Studio", "Support", "Events", "Community"],
                "risk": "low",
                "data": "formatted embeds and previews",
                "missingConsequence": "messages fall back to plain text"
            },
            {
                "key": "MANAGE_MESSAGES",
                "scope": "optional",
                "modules": ["Security"],
                "risk": "medium",
                "data": "message deletion for configured moderation actions",
                "missingConsequence": "purge and delete-based enforcement remain unavailable"
            },
            {
                "key": "MODERATE_MEMBERS",
                "scope": "optional",
                "modules": ["Security"],
                "risk": "high",
                "data": "timeouts and moderation cases",
                "missingConsequence": "timeout enforcement is unavailable; cases remain auditable"
            },
            {
                "key": "MANAGE_ROLES",
                "scope": "optional",
                "modules": ["Security", "Community", "Events"],
                "risk": "high",
                "data": "join-gate, self-roles and temporary event roles",
                "missingConsequence": "role assignment and quarantine restoration are unavailable"
            },
            {
                "key": "MANAGE_CHANNELS",
                "scope": "optional",
                "modules": ["Support", "Events"],
                "risk": "high",
                "data": "private ticket channels and temporary event channels",
                "missingConsequence": "ticket and temporary-channel workflows are unavailable"
            },
            {
                "key": "MESSAGE_CONTENT_INTENT",
                "scope": "gateway",
                "modules": ["Security", "Automate", "Insights"],
                "risk": "high",
                "data": "message content for bounded rules and workflows",
                "missingConsequence": "content-based detection runs in degraded mode"
            },
            {
                "key": "GUILD_MEMBERS_INTENT",
                "scope": "gateway",
                "modules": ["Security", "Community"],
                "risk": "high",
                "data": "member joins, leaves and account-age checks",
                "missingConsequence": "join-gate and member lifecycle features are incomplete"
            }
        ]
    })
}

async fn analytics(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let days = query.limit.unwrap_or(30).clamp(1, 365);
    let rows = state
        .store
        .stats_for(&claims.guild_id, days)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    let cases = state
        .store
        .recent_cases(&claims.guild_id, 200)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(serde_json::json!({
        "guildId": claims.guild_id,
        "days": rows,
        "totalCases": cases.len(),
    })))
}

async fn privacy_export(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let export = state
        .store
        .export_guild(&claims.guild_id)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(export))
}

async fn privacy_receipt(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "version": 1,
        "source": "helper_runtime_metadata_v1",
        "guildId": claims.guild_id,
        "intents": [
            {"key": "GUILDS", "purpose": "guild and channel metadata"},
            {"key": "GUILD_MEMBERS", "purpose": "joins, leaves, join-gate and anti-raid"},
            {"key": "MESSAGE_CONTENT", "purpose": "bounded moderation and message workflows"},
            {"key": "AUTO_MODERATION_EXECUTION", "purpose": "audit Discord AutoMod outcomes"},
            {"key": "GUILD_MODERATION", "purpose": "destructive-action Audit Log guard"}
        ],
        "persistedFields": [
            "guild-scoped settings", "moderation cases", "aggregate daily stats",
            "ticket metadata", "workflow definitions and run metadata", "quota counters",
            "managed guild metadata scoped to the session"
        ],
        "notPersistedByDefault": ["message bodies", "Discord tokens", "OAuth client secrets", "session cookies"],
        "retention": {
            "analyticsDays": 30,
            "auditDays": 30,
            "transcriptDays": 30,
            "configuration": "until changed or guild purge"
        },
        "subprocessors": [],
        "lastPurgeAt": null,
        "exportEndpoint": "/api/privacy/export",
        "deleteEndpoint": "/api/privacy/delete"
    })))
}

#[derive(Debug, Deserialize)]
struct PrivacyDeleteRequest {
    confirmation: String,
}

async fn privacy_delete(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(request): Json<PrivacyDeleteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    // Destructive privacy operations must not be authorisable by the ambient
    // SameSite=None session cookie; require an explicit bearer token to limit
    // cross-site request-forgery impact.
    let bearer_present = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| !value.trim().is_empty());
    if !bearer_present {
        return Err(client_error(StatusCode::UNAUTHORIZED, "bearer_required"));
    }
    let claims = require_auth(&state, &headers)?;
    if request.confirmation.trim() != claims.guild_id {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "confirmation_required",
        ));
    }
    state
        .store
        .purge_guild(&claims.guild_id)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "guildId": claims.guild_id,
        "deleted": "guild_operational_data"
    })))
}

async fn import_config(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(document): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let summary = state
        .store
        .import_guild_config(&claims.guild_id, &document)
        .map_err(|error| {
            let code = error.to_string();
            let invalid = [
                "invalid_target_guild",
                "config_too_large",
                "invalid_config_document",
                "unsupported_config_version",
                "invalid_settings",
                "invalid_tags",
                "invalid_workflows",
                "config_limits_exceeded",
                "invalid_setting_key",
                "invalid_setting_value",
                "invalid_setting_bounds",
                "secret_setting_rejected",
                "invalid_tag_name",
                "invalid_tag_content",
                "invalid_tag_author",
                "invalid_tag_bounds",
                "invalid_workflow_name",
                "invalid_workflow_trigger",
                "invalid_workflow_action",
                "invalid_workflow_payload",
                "unsupported_workflow",
            ];
            if invalid.contains(&code.as_str()) {
                client_error(StatusCode::BAD_REQUEST, "invalid_config_import")
            } else {
                client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error")
            }
        })?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "guildId": claims.guild_id,
        "imported": summary,
    })))
}

#[derive(Debug, Deserialize)]
struct WorkflowRequest {
    name: String,
    trigger: String,
    condition: Option<String>,
    action: String,
    payload: String,
}

async fn workflows(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let rows = state
        .store
        .workflows(&claims.guild_id, 100)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    Ok(Json(serde_json::json!({"workflows": rows})))
}

async fn create_workflow(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(request): Json<WorkflowRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    if !(1..=50).contains(&request.name.trim().len())
        || request.trigger != "message"
        || request.action != "reply"
        || request.payload.trim().is_empty()
        || request.payload.len() > 1_000
        || request.condition.as_deref().unwrap_or_default().len() > 200
    {
        return Err(client_error(StatusCode::BAD_REQUEST, "invalid_workflow"));
    }
    let plan = effective_plan(&state, &claims).await;
    let workflow_limit = quota_limit(&plan, "workflows");
    let Some(id) = state
        .store
        .create_workflow_bounded(
            &claims.guild_id,
            request.name.trim(),
            &request.trigger,
            request.condition.as_deref().unwrap_or_default(),
            &request.action,
            request.payload.trim(),
            workflow_limit,
        )
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?
    else {
        return Err(client_error(
            StatusCode::TOO_MANY_REQUESTS,
            "workflow_quota_exceeded",
        ));
    };
    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

async fn delete_workflow(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let claims = require_auth(&state, &headers)?;
    let deleted = state
        .store
        .delete_workflow(&claims.guild_id, id)
        .map_err(|_| client_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error"))?;
    if !deleted {
        return Err(client_error(StatusCode::NOT_FOUND, "workflow_not_found"));
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

fn require_auth(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<SessionClaims, (StatusCode, Json<ApiError>)> {
    authenticate(state, headers)
        .ok_or_else(|| client_error(StatusCode::UNAUTHORIZED, "unauthenticated"))
}
fn authenticate(state: &ApiState, headers: &HeaderMap) -> Option<SessionClaims> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|value| {
                    value
                        .split(';')
                        .find_map(|item| item.trim().strip_prefix("vh_session="))
                })
        });
    let claims = raw
        .and_then(|token| verify_session(token, &state.session_secret))
        .filter(|claims| {
            claims.expires_at > Utc::now()
                && claims.last_seen_at + Duration::minutes(IDLE_MINUTES) > Utc::now()
        });
    if let Some(claims) = claims.as_ref() {
        state.store.load_session(claims.session_id).ok().flatten()?;
    }
    claims
}
fn sign_session(claims: &SessionClaims, secret: &str) -> String {
    let payload = format!(
        "{}.{}.{}.{}",
        claims.session_id,
        claims.user_id,
        claims.guild_id,
        claims.expires_at.timestamp()
    );
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts every key length");
    mac.update(payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}
fn verify_session(token: &str, secret: &str) -> Option<SessionClaims> {
    let (payload, signature) = token.split_once('.')?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(&payload);
    mac.verify_slice(&signature).ok()?;
    let values: Vec<&str> = std::str::from_utf8(&payload).ok()?.split('.').collect();
    if values.len() != 4 {
        return None;
    }
    Some(SessionClaims {
        session_id: Uuid::parse_str(values[0]).ok()?,
        user_id: values[1].into(),
        guild_id: values[2].into(),
        issued_at: Utc::now(),
        expires_at: chrono::DateTime::from_timestamp(values[3].parse().ok()?, 0)?,
        last_seen_at: Utc::now(),
    })
}
fn sign_oauth_state(payload: &str, secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts every key length");
    mac.update(payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}
fn verify_oauth_state(token: &str, secret: &str) -> Option<String> {
    let (payload, signature) = token.split_once('.')?;
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(&payload_bytes);
    mac.verify_slice(&signature).ok()?;
    String::from_utf8(payload_bytes).ok()
}
fn has_manage_permission(value: &str) -> bool {
    value
        .parse::<u64>()
        .map(|bits| bits & (1 << 5) != 0 || bits & (1 << 3) != 0)
        .unwrap_or(false)
}
fn client_error(status: StatusCode, code: &str) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            code: code.into(),
            message: code.into(),
            request_id: None,
        }),
    )
}

pub async fn serve(bind_addr: &str, state: ApiState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "helper api listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use helper_store::Store;
    use tower::ServiceExt;

    fn state(store: Store) -> ApiState {
        ApiState {
            store,
            session_secret: "test-session-secret-with-at-least-32-bytes".into(),
            oauth_client_id: "client".into(),
            oauth_client_secret: "secret".into(),
            oauth_redirect_uri: "https://example.test/callback".into(),
            allow_legacy_session: false,
            allowed_origin: None,
            entitlements: None,
        }
    }

    fn claims(guild_id: &str) -> SessionClaims {
        let now = Utc::now();
        SessionClaims {
            session_id: Uuid::new_v4(),
            user_id: "user-1".into(),
            guild_id: guild_id.into(),
            issued_at: now,
            expires_at: now + Duration::hours(1),
            last_seen_at: now,
        }
    }

    #[tokio::test]
    async fn authenticated_api_isolates_guild_data_and_import_target() {
        let store = Store::open(":memory:").unwrap();
        store
            .record_case("guild-a", "warn", "user-a", "mod", "a", None)
            .unwrap();
        store
            .record_case("guild-b", "warn", "user-b", "mod", "b", None)
            .unwrap();
        store.open_ticket("guild-a", "user-a", "channel-a").unwrap();
        store.open_ticket("guild-b", "user-b", "channel-b").unwrap();
        let session = claims("guild-a");
        store.save_session(&session).unwrap();
        let token = sign_session(&session, "test-session-secret-with-at-least-32-bytes");
        let app = router(state(store.clone()));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/cases")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["cases"].as_array().unwrap().len(), 1);
        assert_eq!(body["cases"][0]["guild_id"], "guild-a");
        assert!(!body.to_string().contains("guild-b"));

        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .uri("/api/tickets")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["tickets"].as_array().unwrap().len(), 1);
        assert_eq!(body["tickets"][0]["guild_id"], "guild-a");
        assert!(!body.to_string().contains("channel-b"));

        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .uri("/api/permissions")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(body["permissions"].as_array().unwrap().len() >= 8);

        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .uri("/api/security/health")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["guildId"], "guild-a");
        assert_eq!(body["score"], 35);
        assert!(
            body["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| finding["id"] == "security.anti_nuke.enabled")
        );

        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .uri("/api/privacy/receipt")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["guildId"], "guild-a");
        assert!(
            body["notPersistedByDefault"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "Discord tokens")
        );

        store
            .set_setting("guild-a", "support.panel.1", "{}")
            .unwrap();
        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .uri("/api/quotas")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["usage"]["panels"], 1);
        assert_eq!(body["limits"]["panels"], 1);

        let import = serde_json::json!({
            "version": 1,
            "guildId": "guild-b",
            "settings": [{"key": "welcome.channel", "value": "123"}],
            "tags": [],
            "workflows": []
        });
        let response = router(state(store.clone()))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/config/import")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(import.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store.get_setting("guild-a", "welcome.channel").unwrap(),
            Some("123".into())
        );
        assert!(
            store
                .get_setting("guild-b", "welcome.channel")
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn api_enforces_free_workflow_quota() {
        let store = Store::open(":memory:").unwrap();
        let session = claims("guild-a");
        store.save_session(&session).unwrap();
        let token = sign_session(&session, "test-session-secret-with-at-least-32-bytes");
        for index in 0..3 {
            store
                .create_workflow(
                    "guild-a",
                    &format!("workflow-{index}"),
                    "message",
                    "hello",
                    "reply",
                    "Hi",
                )
                .unwrap();
        }
        let response = router(state(store))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/workflows")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "workflow-4",
                            "trigger": "message",
                            "condition": "hello",
                            "action": "reply",
                            "payload": "Hi"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn session_signature_binds_guild_claim() {
        let original = claims("guild-a");
        let token = sign_session(&original, "test-session-secret-with-at-least-32-bytes");
        assert_eq!(
            verify_session(&token, "test-session-secret-with-at-least-32-bytes")
                .unwrap()
                .guild_id,
            "guild-a"
        );
        let mut parts = token.split('.');
        let payload = URL_SAFE_NO_PAD.decode(parts.next().unwrap()).unwrap();
        let signature = parts.next().unwrap();
        let tampered = String::from_utf8(payload)
            .unwrap()
            .replace("guild-a", "guild-b");
        let forged = format!("{}.{}", URL_SAFE_NO_PAD.encode(tampered), signature);
        assert!(verify_session(&forged, "test-session-secret-with-at-least-32-bytes").is_none());
    }
}
