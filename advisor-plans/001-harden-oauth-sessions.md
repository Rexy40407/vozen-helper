# Plan 001: Harden OAuth, sessions and mutation access

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. If a
> STOP condition occurs, stop and report — do not improvise. When complete,
> update this plan's row in `advisor-plans/README.md`.
>
> **Drift check (run first)**:
>
> ```powershell
> git diff --stat 03f09a8..HEAD -- crates/helper-api/src/lib.rs crates/helper-store src/api .github/workflows/ci.yml
> git -C ..\vozen-org-ui-fix diff --stat e79571e..HEAD -- apps/helper-panel/src/api.ts
> ```
>
> If the OAuth/session functions or the panel token bridge differ materially
> from the Current state, stop and report before changing them.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: none
- **Category**: security / bug
- **Planned at**: commit `03f09a8`, 2026-08-19
- **Implementation**: complete in the current Rust API/store; session
  revocation/idle-expiry and PKCE regression tests pass.

## Why this matters

The Helper panel controls configuration for Discord servers. Its OAuth state
and signed session must be bound to the original browser request and must obey
the intended idle expiry. The current implementation has a request value that
is placed directly in a response header with `unwrap`, accepts a PKCE verifier
from the callback URL, and rebuilds `last_seen_at` during token verification.
These flaws weaken the browser/session boundary and can produce confusing
login persistence. This plan makes the Rust store authoritative and retires
legacy token transport without changing the public account UX.

## Current state

- `crates/helper-api/src/lib.rs:351-425` receives a client-supplied
  `code_verifier`, validates only length/challenge, stores it, and injects it
  directly into a `Set-Cookie` header with `parse().unwrap()`.

  ```rust
  if !(43..=128).contains(&code_verifier.len()) || code_challenge.trim().len() < 43 { ... }
  state.store.register_oauth_state(&state_hash, expires, code_verifier)?;
  response.headers_mut().insert(
      header::SET_COOKIE,
      format!("{OAUTH_COOKIE}={}; HttpOnly; Secure; SameSite=None; Path=/; Max-Age=600", code_verifier)
          .parse()
          .unwrap(),
  );
  ```

- `crates/helper-api/src/lib.rs:429-475` accepts `code_verifier` in the
  callback query and prioritizes it over the signed/one-time stored state.

  ```rust
  struct OAuthCallbackQuery { code: String, state: String, code_verifier: Option<String> }
  let code_verifier = query.code_verifier
      .or_else(|| cookie_value(&headers, OAUTH_COOKIE))
      .or(stored_verifier)
      .ok_or_else(|| client_error(StatusCode::BAD_REQUEST, "missing_pkce_verifier"))?;
  ```

- `crates/helper-api/src/lib.rs:11422-11497` checks idle expiry but
  `verify_session` recreates `issued_at` and `last_seen_at` with `Utc::now()`.
  Consequently the idle check is always freshly satisfied as long as the
  signature/session row and absolute expiry are valid.

  ```rust
  claims.last_seen_at + Duration::minutes(IDLE_MINUTES) > Utc::now()
  // later in verify_session
  issued_at: Utc::now(),
  last_seen_at: Utc::now(),
  ```

- `../vozen-org-ui-fix/apps/helper-panel/src/api.ts:463-496` has the intended
  canonical direction: it exchanges the account token once for a secure
  first-party Helper session cookie. `api.ts:498-521` still supports a legacy
  session bearer in request headers. Do not move the Discord account token to
  local storage or URL fragments.
- The current CI commands in `.github/workflows/ci.yml` are `cargo audit`,
  `cargo fmt --all -- --check`, `cargo test --workspace --all-targets`, and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
  Preserve these gates and extend their test coverage where needed.

## Commands you will need

| Purpose              | Command                                                                                                                                                                                | Expected on success |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| Rust audit           | `cargo audit`                                                                                                                                                                          | exit 0              |
| Format               | `cargo fmt --all -- --check`                                                                                                                                                           | exit 0              |
| Tests                | `cargo test --workspace --all-targets`                                                                                                                                                 | all pass            |
| Lint                 | `cargo clippy --workspace --all-targets --all-features -- -D warnings`                                                                                                                 | exit 0              |
| Node rollback checks | `npm ci; npm run lint; npm run typecheck; npm run build; npm test`                                                                                                                     | all exit 0          |
| Panel bridge check   | `npm ci --prefix ..\vozen-org-ui-fix\apps\helper-panel; npm run ui:check --prefix ..\vozen-org-ui-fix\apps\helper-panel; npm run build --prefix ..\vozen-org-ui-fix\apps\helper-panel` | all exit 0          |

## Scope

**In scope**:

- `crates/helper-api/src/lib.rs` and its Rust tests
- the session/OAuth persistence functions in `crates/helper-store`
- any shared session contract type required by those two crates
- `../vozen-org-ui-fix/apps/helper-panel/src/api.ts` only to remove retired
  legacy bearer/hash transport after the cookie bridge is proven
- focused tests and CI wiring needed for these paths

**Out of scope**:

- Changing Discord OAuth scopes, production secrets, or redirect hosts.
- Redesigning account/panel pages.
- General feature configuration changes (Plan 002).
- Removing the Node rollback API without the parity/migration work in Plan 005.

## Git workflow

- Branch: `advisor/001-oauth-session-boundary` in the Helper repository; use
  a matching isolated branch in `vozen-org-ui-fix` only if the client cleanup
  is necessary.
- Commit in logical units, for example
  `fix(auth): bind helper OAuth verifier to one-time server state`.
- Do not expose actual verifier values, tokens, cookies, or session secrets in
  logs, tests, commits, or PR text.

## Steps

### Step 1: Make OAuth verifier handling header-safe and server-bound

Validate the verifier against the RFC 7636 unreserved character set and length
before persisting it. Do not place raw client input in a header with `unwrap`.
Prefer storing the verifier only in the one-time server-side OAuth-state row;
if a short-lived cookie remains needed for browser compatibility, encode it
safely and return a normal API error instead of panicking on header creation.

At callback, remove `code_verifier` from `OAuthCallbackQuery`. Use the
single-use stored verifier (and only a safely encoded compatibility cookie if
strictly necessary), then consume it atomically. Ensure a missing, expired, or
replayed state returns a stable 4xx error without calling Discord.

**Verify**: add Rust tests for malformed verifier characters, valid PKCE,
missing verifier, replay, expiry, and a callback query that includes the now
ignored/rejected `code_verifier`; run the Rust checks above.

### Step 2: Make persisted session metadata authoritative

Change `authenticate`/`verify_session` so signed token parsing establishes only
immutable identity and absolute expiry. Load the session record from the store
and use its persisted issued/last-seen/revocation metadata for idle expiry.
Update `last_seen_at` only on a successful authenticated request according to a
bounded touch policy; use one store operation that cannot revive a revoked or
expired session.

Do not encode a mutable timestamp into a token unless token rotation and
revocation semantics are fully specified. The store is the authority.

**Verify**: add fake-clock or explicit-time Rust tests proving: active session
works; idle-expired session returns 401; absolute-expired session returns 401;
revoked/deleted session returns 401; successful activity refreshes only the
persisted row; a stale signed token cannot bypass the store.

### Step 3: Retire legacy browser bearer transport after compatibility proof

Keep the first-party `/api/session/vozen` cookie exchange as the normal path.
Instrument tests (not production token logging) to prove account-to-panel
bootstrap works using only cookies. Then remove the legacy helper bearer from
URL fragments, session storage, and `Authorization` request decoration in the
panel/client paths that are no longer needed.

If any non-`vozen.org` redirect still returns a session fragment, replace it
with a same-origin completion page that establishes the HttpOnly cookie and
redirects without credentials in the URL.

**Verify**: panel API tests cover successful first-party bootstrap, 401 retry
once, expired session recovery, and no token/hash/bearer in the final URL or
browser storage. Run panel checks and all Helper checks.

### Step 4: Lock mutation enforcement to the hardened identity

Review every mutating `/api/*` route to ensure it calls the same
`require_mutation_auth`/origin path and derives guild identity from the verified
session, not a request body or query value. Add a table-driven route test for
unauthenticated, expired, cross-origin, and guild-mismatch writes.

**Verify**: the test suite proves all covered mutations return 401/403 without
altering stored configuration; `rg -n 'guild_id.*Json|guildId.*Json' crates/helper-api/src`
is manually reviewed for mutation handlers that could accept an unverified
guild selector.

## Test plan

- PKCE: valid, malformed, wrong challenge, missing, expired, replayed, and
  callback-query injection cases.
- Sessions: valid, idle-expired, absolute-expired, revoked, deleted, and
  touch/throttle cases with controlled time.
- Browser bridge: first-party cookie establishment and no credential in hash,
  query, localStorage, or sessionStorage after completion.
- Mutations: unauthenticated, cross-origin, and selected-other-guild attempts.

## Done criteria

- [ ] No raw verifier is inserted into an HTTP header with `unwrap`.
- [ ] OAuth callback ignores/rejects a URL-supplied PKCE verifier and consumes
      one server-side state record exactly once.
- [ ] Idle expiry uses persisted metadata, not `Utc::now()` reconstructed from
      a signed token.
- [ ] Browser completion uses an HttpOnly same-origin cookie and leaks no
      bearer/session credential through URL or web storage.
- [ ] Tests cover the specified negative/security cases and all commands pass.
- [ ] No file outside the in-scope list changed.

## STOP conditions

- The stored session record lacks immutable ID, expiry, and mutable last-seen
  fields required to implement this without a schema migration.
- A legacy redirect host cannot establish a same-origin cookie.
- A required migration would invalidate all currently active sessions without
  an agreed logout/rollover communication plan.
- A route needs a broader authorization model than session guild ownership.

## Maintenance notes

- Treat OAuth verifier, account token, session cookie, and signed session as
  credentials: redact all of them from diagnostics.
- Review new authenticated routes for the shared auth/origin helper rather than
  hand-rolled header parsing.
- Revisit compatibility redirects only when all callers use the first-party
  cookie bridge.
