# Plan 001: Harden OAuth, sessions and mutation access

> **Executor instructions**: Follow each step and run its verification before continuing. Do not copy credentials into tests, logs or plans. Stop if the callback contract or configured panel origin differs from the current state below.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security / correctness
- **Planned at**: `b748ea7`, 2026-08-01

## Why this matters

The panel's authentication is a production boundary. The Rust callback currently returns a browser-readable session response instead of a controlled panel handoff, accepts a PKCE verifier from the query, and does not enforce the documented idle timeout. A valid session can therefore be exposed or remain active longer than intended, while public OAuth-start requests can create unbounded pending state.

## Current state

- `crates/helper-api/src/lib.rs:202-230` writes the received verifier to a cookie and accepts a query verifier.
- `crates/helper-api/src/lib.rs:295-311` returns the callback's JSON response directly; `SessionResponse` includes a bearer token at `:338-346` and `:454-456`.
- `crates/helper-api/src/lib.rs:51-131,180-187` has no OAuth rate-limit layer and writes a pending state for each start.
- `crates/helper-api/src/lib.rs:1881-1889,1939-1940` checks `last_seen_at`, but `verify_session` creates it from the current clock and `authenticate` does not touch the persisted value.
- `crates/helper-api/src/lib.rs:526-556` switches guilds from the OAuth snapshot without a fresh membership/permission check.
- `src/api/server.ts:207-214,249-269,485-493` is the legacy rollback API; its cookie guard has no Origin/CSRF check for destructive mutations.

## Steps

### Step 1: Replace the callback JSON/token exposure with a one-use panel handoff

Keep the bearer token out of callback JSON, URLs, history and logs. Return a 303 only to the configured panel callback origin, carrying a short-lived, single-use result that the panel exchanges over an authenticated same-origin bridge. Clear the OAuth verifier/state cookie after success and on failure. Preserve an explicit JSON error for non-browser API clients.

**Verify**: add an API test asserting callback response is 303, has no token field/body, clears the verifier, and that the one-use exchange produces `/api/me` claims exactly once.

### Step 2: Enforce PKCE input and public endpoint limits

Accept the verifier only from the HttpOnly cookie/bridge, validate the base64url length and character set before writing headers, and replace every header-value `unwrap()` on this path with a 400 response. Add a per-IP limiter for OAuth start/callback plus a bounded pending-state count and cleanup index.

**Verify**: tests reject query-only/invalid verifiers, never panic on malformed input, return 429 after the configured threshold, and remove expired pending state.

### Step 3: Make idle expiry and guild authorization real

Load the persisted session claims, compare the stored last-seen value, atomically refresh it with throttling, and reject idle sessions. On guild switch and sensitive mutations, revalidate membership and Manage Server/Administrator permissions; revoke the session when access is lost, with a safe deny policy during Discord outages.

**Verify**: tests cover idle expiry, active refresh, revoked sessions, removed permissions, removed bot membership and API outage denial.

### Step 4: Protect the legacy rollback API

Add a mutation guard equivalent to Rust's Origin check: require an explicit bearer for destructive requests or require a valid CSRF token plus an allowed Origin for cookie requests. Cover `/api/web-config/delete` and import/update routes.

**Verify**: a cross-origin cookie-only POST receives 403 and an allowed-origin bearer/cookie request still succeeds.

## Scope

**In scope**: `crates/helper-api/src/lib.rs`, its Rust API tests, `src/api/server.ts`, `tests/api.test.ts`, and the minimum session-store helper files required by the implementation.

**Out of scope**: UI redesign, feature schemas, Discord runtime handlers, token rotation operations on the VPS.

## Done criteria

- No OAuth callback response contains a bearer token.
- Query verifier fallback and header `unwrap()` are gone.
- Idle timeout and guild permission recheck have tests.
- OAuth start is rate-limited and pending state is bounded.
- Legacy destructive cookie mutations reject cross-origin requests.
- `cargo test -p helper-api` and `npm run typecheck && npx vitest run tests/api.test.ts` pass.

## STOP conditions

Stop if the configured panel callback origin is not discoverable from the current environment/config, or if a change would require exposing a long-lived token to GitHub Pages.
