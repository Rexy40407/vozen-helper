# Rust parity gate

This matrix is a release gate, not a claim that the migration is complete.

| Area | Rust status | Evidence / remaining gate |
| --- | --- | --- |
| Core gateway and API | Implemented | `cargo test --workspace`, release build, API smoke |
| OAuth and sessions | Implemented with PKCE | HMAC state, S256 verifier, guild permission recheck; legacy token route is opt-in |
| Moderation | Partial but growing | warn, violation, note, reason, timeout, untimeout, kick, ban, unban, purge; policy automod/raid/quarantine parity remains |
| Support | Basic workflow | private ticket panel, claim, close, durable ticket rows; transcripts, SLA, routing and retention remain |
| Community | Partial | AFK, reminders, tags, XP/leaderboard, stats, self-role panels; suggestions, giveaways, welcome and starboard remain |
| Events | Not complete | poll/giveaway lifecycle and anti-abuse gates remain |
| Automate | Skeleton | scheduler boundary exists; trigger/condition/action editor and durable worker parity remain |
| Insights | Basic API | cases/stats/quotas endpoints exist; retention, analytics and exports remain |
| Entitlements | Source integrated | signed central resolver exists and is tested; production service activation remains |
| VPS rollout | Staged only | migration/doctor/API smoke passed; privileged systemd cutover and 7-day soak remain |

The goal cannot be marked complete while any required row is partial/not complete or while the
production memory, security, parity and rollback gates lack live evidence.
