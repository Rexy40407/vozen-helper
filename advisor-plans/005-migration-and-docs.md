# Plan 005: Establish migration parity and reconcile operating docs

> **Executor instructions**: Treat the Node database schema as production data. Do not edit or reorder released Node migrations. Stop before any destructive migration and require a verified backup fixture.

## Why this matters

The project now describes Rust as the canonical multi-guild runtime, but several docs still describe the retired single-guild Node API. More importantly, Node has ordered `PRAGMA user_version` migrations while Rust uses large idempotent creation/ALTER checks. Without a versioned bridge, rollback and cutover can silently lose or misinterpret settings.

## Current state

- Node migrations: `src/store/db.ts:5-16,279-292`.
- Rust schema setup: `crates/helper-store/src/lib.rs:202-229`, including a compatibility column comment at `:205-207`.
- Conflicting architecture statements: `README.md:3-5,17-19`, `CONTRIBUTING.md:16-24`, `docs/PLAN.md:7,27-32`, `docs/PLAN-SETTINGS-UI.md:3-10`.
- Broken setup command: `README.md:40` says `cargo run -p vozen-helper`, while `crates/helper-runtime/Cargo.toml:2,21` names package `helper-runtime` and binary `vozen-helper`; Node deployment instructions remain in `docs/SETUP.md:76-80,100,124-127`.

## Steps

1. Inventory every released Node migration and Rust table/column, including settings, cases, community tables and audit/privacy records. Produce fixtures for each supported Node schema version.
2. Add explicit Rust migration IDs/checksums (or a documented adapter that records the Node version), idempotent forward migration, and a rollback-safe preflight. Never infer success from `CREATE TABLE IF NOT EXISTS` alone.
3. Add round-trip/parity tests: Node fixture → Rust startup/read → export → rollback fixture comparison. Include empty DB, latest production-shaped DB and one older migration.
4. Update README, CONTRIBUTING, setup, rollback, parity and panel docs to state one canonical architecture: Rust multi-guild runtime, authenticated panel, Node only as controlled rollback. Correct commands, service names, health checks and OAuth flow.
5. Add a release gate requiring backup verification, migration dry-run, parity tests and explicit rollback evidence before VPS promotion.

**Verify**: migration fixtures pass in CI; the corrected Rust start command exits successfully in a disposable DB; docs grep contains no active Node deployment instructions outside rollback documentation.

## Done criteria

- Rust migration state is versioned and observable.
- Node→Rust parity fixtures pass without destructive changes.
- README/setup/rollback/parity docs agree on runtime, tenancy and commands.
- Release CI blocks promotion when migration/backup/parity checks fail.

## STOP conditions

Stop if the live SQLite schema/version cannot be copied into a disposable fixture without exposing secrets or if parity reveals a column with no Rust consumer; record the mismatch instead of improvising a migration.
