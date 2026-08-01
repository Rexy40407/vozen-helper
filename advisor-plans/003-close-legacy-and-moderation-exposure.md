# Plan 003: Close legacy API, import and moderation exposure gaps

> **Executor instructions**: Keep rollback compatibility, but apply the same defensive boundaries as the canonical Rust path. Never broaden accepted settings to make an import “work”.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: `001-harden-oauth-sessions.md`
- **Category**: security
- **Planned at**: `b748ea7`, 2026-08-01

## Evidence and impact

- `crates/helper-store/src/lib.rs:931-993,1068-1074` validates import size/secret needles but stores arbitrary setting keys. A crafted import can change internal flags or safety keys outside the public contract.
- `crates/helper-discord/src/lib.rs:142-143,1924-1928` exposes `/cases` without the permission map used by `modlogs` at `:3943-3965`, leaking moderation targets/reasons to ordinary members.
- `crates/helper-discord/src/lib.rs:2244-2254,2326-2342` publishes member-controlled suggestion/poll text without an explicit empty allowed-mentions policy.

## Steps

1. Define exportable namespaces and per-key schemas in the store; reject unknown/internal keys, validate types/ranges, and add dry-run/diff import before the transaction. Preserve versioned imports with an explicit migration path for older exports.
2. Require `MODERATE_MEMBERS` or `MANAGE_GUILD` for `/cases`, make the response ephemeral when supported, and add command-permission tests for allowed/denied members.
3. Set explicit empty allowed mentions for every bot message that contains suggestion/poll/member text. Add tests for everyone, role and user mention tokens.
4. Add regression tests to the legacy Node API proving cross-origin cookie-only destructive requests fail, then document the retirement condition for the rollback service.

**Verify**: `cargo test --workspace` and `npx vitest run tests/api.test.ts`; import tests show unknown/internal keys rejected and no settings written.

## Scope

`crates/helper-store/src/lib.rs`, `crates/helper-api/src/lib.rs`, `crates/helper-discord/src/lib.rs`, related Rust tests, `src/api/server.ts`, and `tests/api.test.ts` only. Do not remove the rollback service or change the public moderation vocabulary.

## Done criteria

- Imports accept only documented namespaces and are atomic.
- `/cases` is permission-gated and tested.
- User-controlled content cannot trigger Discord mentions.
- Legacy destructive mutations have CSRF/origin protection.
