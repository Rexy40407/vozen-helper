# Rust parity gate

This matrix is a release gate, not a claim that the migration is complete.

| Area | Rust status | Evidence / remaining gate |
| --- | --- | --- |
| Core gateway and API | Implemented | `cargo test --workspace`, release build, API smoke; authenticated multi-guild isolation, Permission Passport and versioned config export/import tests |
| OAuth and sessions | Implemented with PKCE | HMAC state, S256 verifier, guild permission recheck; legacy token route is opt-in |
| Moderation | Partial but growing | warn, violation, note, reason, timeout, untimeout, kick, ban, unban, purge, quarantine, native AutoMod audit, bounded join-gate, configurable join-burst anti-raid latch, Audit Log anti-nuke containment, explicit shadow mode, deterministic Safety Health Score and structured `/api/audit` evidence; role restoration/advanced recovery remains |
| Support | Functional core | private ticket panel, claim, close, routing, transcripts and durable SLA reminders; richer escalation remains |
| Community | Functional core | AFK, reminders, tags, XP/leaderboard, stats, self-role panels, suggestions, giveaways, welcome and starboard; scoped export/delete now available |
| Events | Functional lifecycle | native Discord Scheduled Events create/list/cancel plus durable polls and giveaways with votes/entries, scheduled close and anti-abuse bounds; richer event templates remain |
| Automate | Functional bounded MVP | message trigger, optional contains condition, reply action, durable workflows/runs and dashboard endpoints; broader trigger/action catalog remains |
| Insights | Functional API | cases/stats/quotas, analytics and workflow endpoints; privacy receipt, scoped export/delete and versioned config export/import now available |
| Entitlements | Source integrated and live | signed central resolver is active as `vozen-entitlementd.service` on loopback; HMAC, replay rejection, Free/Plus/Premium and seat isolation smoke checks passed against an isolated DB copy; real billing/webhook purchase remains an external gate |
| VPS rollout | Rust live, soak pending | `vozen-helper.service` runs release `9c707a2` under systemd; health, Discord gateway sockets, command registration, rollback backup and memory checks passed; the seven-day soak and full interactive parity gate remain |

The goal cannot be marked complete while any required row is partial/not complete or while the
production memory, security, parity and rollback gates lack live evidence.
