# Vozen Helper — production readiness for all 47 features

The Rust catalogue has 47 unique keys and every key has a bounded adapter, a
schema, defaults, validation and a runtime preview. A feature is only shown as
ready for a guild after the running process has the required Discord
permissions and provider dependencies.

With the default VPS environment (no optional third-party grants), the panel
should show **47 implemented/configuration cards**. Every card has a real
adapter, schema, validation, preview and runtime projection. Some cards can
still show `dependency_down` or `blocked_*_approval` until their API key,
RPC, external account or legal approval is configured. The setup page remains
visible so the owner can see exactly what is required; enabling delivery is
still rejected until the running provider is ready.

## What can be enabled without a third-party approval

The internal moderation, community, support, utility, XP, templates, stats,
economy, Bluesky and public CoinGecko paths are implemented in Rust. Their
actual Discord behaviour still depends on the bot being installed with the
permissions shown by the panel preflight.

## Provider-gated features

These seven areas remain blocked until the dependency is configured and
approved. This is intentional: enabling a card without its provider would
create a setting that cannot deliver anything.

| Feature             | Server-side requirement                                                                                                                                                                                                          |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Instagram           | `META_INSTAGRAM_ACCESS_TOKEN`, `META_INSTAGRAM_USER_ID`, Meta approval; an explicitly enabled `META_INSTAGRAM_DEVELOPMENT_MODE=true` may be used only for an authorised tester account while the Meta app remains in development |
| TikTok              | `TIKTOK_ACCESS_TOKEN`, Display API review, `TIKTOK_APP_APPROVED=true`                                                                                                                                                            |
| Kick                | `KICK_ACCESS_TOKEN`, official API/webhook approval, `KICK_APP_APPROVED=true`                                                                                                                                                     |
| Server monetization | Stripe Connect credentials, signed webhooks, KYC/tax/refund support, `STRIPE_CONNECT_APPROVED=true`                                                                                                                              |
| Wallet gating       | `SIWE_DOMAIN`, `SIWE_URI`, `SIWE_SESSION_SECRET`, approved RPC and contract allow-list                                                                                                                                           |

Secrets belong only in the VPS environment (or a secret manager). They must
never be pasted into Discord, the panel, commits, logs or this document.

`META_INSTAGRAM_DEVELOPMENT_MODE` is intentionally opt-in and does not claim
Meta production approval. It should be removed or set back to `false` before
serving public accounts.

## Verification after configuring a provider

1. Restart the Rust Helper so the process constructs the provider client with
   the new environment.
2. Open the feature in the panel and run **Preflight**.
3. Run **Simulate** with a fixture before enabling delivery.
4. Use the provider **Test delivery** action and inspect the audit entry.
5. Confirm health is `ready`, then publish the revision.
6. Keep the provider kill switch available and test rollback in the canary
   guild before enabling other guilds.

For a feature whose stored revision exists but whose runtime projection or
provider subscription is stale, use `POST /api/config/features/{key}/repair`.
Repair is not a destructive reset: it reruns preflight and validation through
the normal atomic publisher and creates a new revision, preserving history.

The API deliberately reports `dependency_down`, `misconfigured` or
`blocked_*_approval` instead of presenting a false operational state.
The feature detail and health routes also run the same Discord preflight used
by publication whenever a feature is enabled. This means a saved revision is
reported as `misconfigured` when the current bot/user permissions, selected
channel, role hierarchy or Discord context would prevent the runtime action.
Disabled features skip the network preflight and remain available for setup.

## Verification recorded on 2026-08-04

The current worktree passed the local release gates:

- `cargo fmt --all`
- `cargo check --workspace --all-targets --offline`
- `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings`
- `cargo test --workspace --all-targets --offline` (as suites API 20, core 30, Discord 18, modules 15 e store 31 passaram; a política Windows Application Control bloqueou alguns executáveis recompilados do runtime/store com o erro 4551, antes de esses testes poderem arrancar)
- `npm.cmd run lint`, `npm.cmd run typecheck`, `npm.cmd test -- --run` (27 files / 255 tests)
- `npm.cmd run build` and `npm.cmd run build --prefix panel`
- `cargo audit` (0 advisories reported)
- `npm.cmd audit --audit-level=high` and panel audit (0 vulnerabilities reported)
- site minification/build and `git diff --check`

These checks prove that the contracts, API, runtime adapters and panel build
are internally consistent. The Windows runner restriction is environmental,
not a failing assertion; the same suites must be rerun on the Linux release
runner. They do **not** replace a Discord canary: the live
VPS must run this release, the bot must have the permissions shown by
preflight, and provider-gated integrations need their own credentials and
approval before delivery can be observed.
