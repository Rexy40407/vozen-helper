//! Module registry for Core, Studio, Security, Support, Events, Community,
//! Automate and Insights. Feature handlers are added behind these boundaries.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use helper_contracts::{EntitlementSnapshot, Plan};
use helper_core::Capability;
use helper_store::Store;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::task::JoinHandle;

pub const MODULES: &[Capability] = &[
    Capability::Core,
    Capability::Studio,
    Capability::Security,
    Capability::Support,
    Capability::Events,
    Capability::Community,
    Capability::Automate,
    Capability::Insights,
];

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct EntitlementClient {
    endpoint: String,
    secret: String,
    http: Client,
    cache: Arc<Mutex<HashMap<String, EntitlementSnapshot>>>,
}

#[derive(Debug, Deserialize)]
struct CentralResponse {
    subject_id: String,
    plan: String,
    guild_limit: u16,
    active: bool,
    expires_at_ms: Option<i64>,
    version: i64,
}

impl EntitlementClient {
    pub fn new(endpoint: Option<String>, secret: Option<String>) -> Option<Self> {
        Some(Self {
            endpoint: endpoint?,
            secret: secret?,
            http: Client::new(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn resolve(
        &self,
        subject_id: &str,
        guild_id: Option<&str>,
    ) -> anyhow::Result<EntitlementSnapshot> {
        if let Some(snapshot) = self.cached(subject_id, guild_id)
            && snapshot.active
            && snapshot
                .expires_at
                .is_none_or(|expires_at| expires_at > Utc::now())
        {
            return Ok(snapshot);
        }
        let body = serde_json::json!({"subject_id": subject_id, "guild_id": guild_id});
        let bytes = serde_json::to_vec(&body)?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let nonce = uuid::Uuid::new_v4().to_string();
        let body_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())?;
        mac.update(format!("{timestamp}\n{nonce}\n{body_hash}").as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        let response = self
            .http
            .post(&self.endpoint)
            .header("x-vozen-timestamp", timestamp.to_string())
            .header("x-vozen-nonce", &nonce)
            .header("x-vozen-signature", format!("v1={signature}"))
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("central entitlement service returned {}", response.status());
        }
        let central: CentralResponse = response.json().await?;
        let plan = match central.plan.as_str() {
            "plus" => Plan::Plus,
            "premium" => Plan::Premium {
                guild_limit: central.guild_limit,
            },
            _ => Plan::Free,
        };
        let snapshot = EntitlementSnapshot {
            product: "vozen-helper".into(),
            subject_id: central.subject_id,
            plan,
            active: central.active,
            version: central.version.try_into().unwrap_or_default(),
            expires_at: central
                .expires_at_ms
                .and_then(chrono::DateTime::from_timestamp_millis),
            fetched_at: Utc::now(),
        };
        self.cache
            .lock()
            .expect("entitlement cache poisoned")
            .insert(Self::cache_key(subject_id, guild_id), snapshot.clone());
        Ok(snapshot)
    }

    fn cache_key(subject_id: &str, guild_id: Option<&str>) -> String {
        format!("{subject_id}:{}", guild_id.unwrap_or("user"))
    }

    pub fn cached(&self, subject_id: &str, guild_id: Option<&str>) -> Option<EntitlementSnapshot> {
        self.cache
            .lock()
            .ok()?
            .get(&Self::cache_key(subject_id, guild_id))
            .cloned()
    }
}

/// Bounded persistent scheduler. It never runs unbounded work on the gateway
/// task: each tick claims at most 100 due rows and removes them only after the
/// dispatch boundary has been reached.
pub fn start_scheduler(store: Store) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            match store.due_scheduled_actions(chrono::Utc::now().timestamp_millis(), 100) {
                Ok(actions) => {
                    for (id, guild_id, kind, target_id, _payload) in actions {
                        tracing::info!(%id, %guild_id, %kind, %target_id, "dispatching scheduled helper action");
                        if let Err(error) = store.delete_scheduled_action(id) {
                            tracing::error!(%error, %id, "failed to ack scheduled action");
                        }
                    }
                }
                Err(error) => tracing::error!(%error, "scheduler tick failed"),
            }
        }
    })
}
