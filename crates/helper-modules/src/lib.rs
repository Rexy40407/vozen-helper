//! Module registry for Core, Studio, Security, Support, Events, Community,
//! Automate and Insights. Feature handlers are added behind these boundaries.

use helper_core::Capability;
use helper_store::Store;
use std::time::Duration;
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
                    for (id, guild_id, kind, target_id) in actions {
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
