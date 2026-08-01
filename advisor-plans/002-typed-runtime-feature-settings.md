# Plan 002: Make feature settings typed, atomic and runtime-backed

> **Executor instructions**: Implement only validated, runtime-backed settings. If a field cannot be consumed safely by the live Rust/Discord runtime, remove it from the editable surface or mark it unavailable; never report a stored no-op as operational.

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `001-harden-oauth-sessions.md`
- **Category**: correctness / security
- **Planned at**: `b748ea7`, 2026-08-01

## Why this matters

The panel now exposes rich configuration, but the API accepts any JSON object up to 64 KiB and only maps anti-raid, join-gate, starboard and tickets into runtime keys. Anti-spam, levels, welcome, suggestions, giveaways and polls can therefore show “published” while the bot ignores the values. The current multi-write sequence can also leave enabled/config/runtime keys inconsistent.

## Current state

- `crates/helper-api/src/lib.rs:710-732` has a partial `sync_runtime_feature_config` switch.
- `crates/helper-api/src/lib.rs:781-798` validates only object/size and writes several settings without a transaction.
- `crates/helper-discord/src/lib.rs:1268-1296` assigns the join-gate role before evaluating account age and does not apply all exposed gate rules.
- `crates/helper-discord/src/lib.rs:1550-1570` hardcodes starboard promotion/removal at 3 reactions while the panel/API writes `community.starboard.threshold` at `crates/helper-api/src/lib.rs:723-725`.
- `crates/helper-discord/src/lib.rs:1617-1667,1329` hardcodes XP, anti-spam and welcome behavior.

## Steps

### Step 1: Define a typed registry

Create one Rust registry containing each editable feature, default values, field types, numeric bounds, Discord-ID validation, cross-field invariants and the runtime keys it owns. Deserialize requests into per-feature structs; reject unknown fields and return field-level 400 errors. Keep the existing feature catalogue generated from this registry so UI/API availability cannot drift.

**Verify**: unit tests reject unknown fields, invalid IDs, inverted ranges, oversized arrays and unsupported feature keys; defaults round-trip through JSON.

### Step 2: Make save atomic and auditable

Wrap enabled flag, JSON config, runtime-key projections and audit record in one SQLite transaction. Roll back all writes if any projection fails. Add a revision/updated-at value so stale editors receive a conflict instead of overwriting newer settings.

**Verify**: failure-injection tests prove zero partial writes; concurrent revision tests return 409; audit entries contain actor, feature, changed fields and timestamp without secrets/member content.

### Step 3: Wire every exposed field or hide it

Add bounded runtime resolvers with the existing store/cache conventions and update each handler. At minimum cover anti-spam thresholds/escalation, anti-scam lists/actions, anti-raid, join-gate order and actions, levels/XP, welcome, starboard, suggestions, tickets, giveaways, polls and workflows. Do not assign a verified role until join-gate checks pass. Read the configured starboard threshold for both promotion and removal.

**Verify**: API→SQLite→runtime integration tests exercise every editable field, including starboard threshold boundaries and a rejected join-gate member. A field with no runtime consumer is removed from the panel schema and catalog status says unavailable.

### Step 4: Add safe simulations

Make `/test` run the same validators and pure decision functions as runtime without sending Discord actions. Return a structured preview, not an echo of arbitrary input.

**Verify**: simulations cover positive/negative protection cases and assert no Discord/store mutation occurred.

## Scope

**In scope**: `crates/helper-api/src/lib.rs`, `crates/helper-store/src/lib.rs`, `crates/helper-discord/src/lib.rs`, typed contract modules, panel API/types/forms, and tests for these contracts.

**Out of scope**: new product categories, MEE6 feature cloning, custom user-uploaded images, billing.

## Done criteria

- Every visible editable field has a typed schema, runtime consumer and integration test.
- Saves are transactional and stale edits cannot overwrite newer revisions.
- Starboard threshold and join-gate action/order are runtime-correct.
- Simulation never mutates Discord or SQLite.
- `cargo test --workspace`, panel build and relevant Vitest tests pass.

## STOP conditions

Stop before enabling a setting in production if the runtime consumer cannot be identified or if a migration would rewrite existing guild configuration without a backup/rollback test.
