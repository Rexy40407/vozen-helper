# Rust parity gate

This matrix is a release gate, not a claim that the migration is complete.

| Area | Rust status | Evidence / remaining gate |
| --- | --- | --- |
| Core gateway and API | Implemented | `cargo test --workspace`, release build, API smoke; authenticated multi-guild isolation and versioned config export/import tests |
| OAuth and sessions | Implemented with PKCE | HMAC state, S256 verifier, guild permission recheck; legacy token route is opt-in |
| Moderation | Partial but growing | warn, violation, note, reason, timeout, untimeout, kick, ban, unban, purge, quarantine, native AutoMod audit, bounded join-gate, configurable join-burst anti-raid latch and Audit Log anti-nuke containment; role restoration/advanced recovery remains |
| Support | Functional core | private ticket panel, claim, close, routing, transcripts and durable SLA reminders; richer escalation remains |
| Community | Functional core | AFK, reminders, tags, XP/leaderboard, stats, self-role panels, suggestions, giveaways, welcome and starboard; scoped export/delete now available |
| Events | Functional lifecycle | native Discord Scheduled Events create/list/cancel plus durable polls and giveaways with votes/entries, scheduled close and anti-abuse bounds; richer event templates remain |
| Automate | Functional bounded MVP | message trigger, optional contains condition, reply action, durable workflows/runs and dashboard endpoints; broader trigger/action catalog remains |
| Insights | Functional API | cases/stats/quotas, analytics and workflow endpoints; privacy-scoped export/delete and versioned config export/import now available |
| Entitlements | Source integrated | signed central resolver exists and is tested; production service activation remains |
| VPS rollout | Staged only | migration/doctor/API smoke passed; privileged systemd cutover and 7-day soak remain |

The goal cannot be marked complete while any required row is partial/not complete or while the
production memory, security, parity and rollback gates lack live evidence.
