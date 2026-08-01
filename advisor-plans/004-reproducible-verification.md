# Plan 004: Restore reproducible verification across Node, Rust and panel

> **Executor instructions**: Do not upgrade dependencies opportunistically. Pin the versions already proven by the repository, then add gates before changing behavior.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW/MED
- **Depends on**: none
- **Category**: tests / DX / dependencies
- **Planned at**: `b748ea7`, 2026-08-01

## Current state

- Root `package.json:16` runs Vitest over files including `site-publish/tests/*.mjs`, which import `node:test`/`node:assert`; `npm test` reports “No test suite found” for those files.
- `.github/workflows/ci.yml:13-18` checks only Node; Rust gates are branch-specific in `rust-release.yml`, and neither panel nor site-publish is built.
- `panel/package.json:11-18` uses `latest` and has no lockfile.
- Root `package.json:8-10,29` permits broad Node versions while native `better-sqlite3` can fail with a Node ABI mismatch before tests execute.
- `crates/helper-runtime/src/main.rs:28-90`, `crates/helper-modules/src/lib.rs:136-164` and `panel/src/App.tsx:68-98` have no executable tests.

## Steps

1. Scope root Vitest to `tests/**/*.test.ts`; add a `site-publish` `node --test` command and run both explicitly in CI.
2. Pin panel React/Vite/TypeScript versions, generate `panel/package-lock.json`, add `npm ci` and `npm run build` for the panel. Add a minimal panel test harness for hash routes, OAuth error state, save/discard and guild isolation.
3. Pin the supported Node toolchain with `.node-version` or equivalent, document clean install/rebuild for `better-sqlite3`, and add a preflight that reports ABI mismatch clearly.
4. Extend normal CI with Rust fmt, workspace tests, clippy, release build, panel build, site-publish tests/build and dependency audits. Use caching but keep release packaging separate.
5. Add Rust startup/config smoke tests, scheduler fake-clock tests and contract serialization tests following existing Rust test patterns.

**Verify**: `npm test`, `npm run typecheck`, `npm run lint`, `npm run build`, `npm --prefix panel ci && npm --prefix panel run build`, `node --test site-publish/tests/*.test.mjs`, `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings` all pass.

## Done criteria

- The documented test command is green and site tests are run by the correct runner.
- Ordinary PR CI compiles/tests Rust and builds panel/site.
- Panel dependencies resolve from a committed lockfile.
- Native SQLite setup gives a deterministic, actionable ABI error.
- Auth/config/routing critical paths have automated coverage.
