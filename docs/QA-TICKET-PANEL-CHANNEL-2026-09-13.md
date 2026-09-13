# Ticket panel channel — local verification

Scope: add a public panel destination separate from the private-ticket category and transcript channel. Helper base 5055774e55cca4ed38681ee9d2d656f57c5d99de; website worktree `vozen-org-ticket-release`, base 20812c67be5ab39bf068ffd438ee8c3d7df61eb7. Changes are not committed, pushed or deployed.

## Behavior

- `panelChannel` is an optional, validated Discord channel selector. Empty preserves existing installations without automatically posting anything.
- Saving enabled tickets saves the revision, then publishes/updates the public panel. The response separately reports `discordApply`; the website warns when configuration was saved but Discord publication failed.
- Guild-scoped references reuse a dashboard or previously registered slash-command panel in the selected channel. Old panels in other channels remain, and the existing plan quota still applies to new panels.
- Only a confirmed HTTP 404 permits replacing a missing message. 403/timeouts/server failures do not trigger replacement.
- A pending nonce is persisted before POST and reused for short retries. An uncertain outcome older than two minutes is blocked for manual inspection, not silently retried. This follows the bounded deduplication described in [Discord's Create Message documentation](https://docs.discord.com/developers/resources/message#create-message).
- Public panel content cannot ping roles/users/everyone. The existing ticket-opening flow retains its separately configured staff notification.
- `/ticket-panel` also consumes the configured destination, verifies it belongs to the current guild, and falls back to its invocation channel when no destination is configured.

## Checks

- Red phase: publisher tests initially failed because implementation was missing.
- `cargo test --workspace --locked --offline --quiet`: PASS, 264 tests including six new publisher tests with local mock HTTP and in-memory SQLite. Covers publish/update, legacy adoption at quota, guild separation, 403 vs 404, quota rejection, stale uncertain delivery, empty config and mention-safe button payload.
- The full regression suite initially caught a missing runtime consumer/projection. Fixed by wiring the same destination into the slash command; no guards were removed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- Website `npm test -- --run`: PASS, existing 12 tests.
- Website `npm run build` and `npm run ui:check`: PASS. Existing font-resolution/chunk-size warnings remain.
- Browser local demo, guild `demo`, no credentials: Portuguese label/help/placeholder visible, selected `#geral`, saved, observed preview-saved toast and retained selection. This is in-memory preview, not production persistence. Test tab and local dev server closed afterwards.
- Local demo also logged an unrelated WorkspaceSkeleton list-key warning and a public-font path warning; neither was changed in this task.

## Remaining verification

No Discord production messages, settings, roles, deployments or credentials were changed. After authorized publication, verify with the real server: select the existing panel channel, save twice, confirm only one Open ticket panel, change title, open/claim a ticket and confirm staff ping. Test an alternate destination only if the plan quota allows it. Multiple API processes or concurrent slash/dashboard creation are not covered by the process-local save mutex. Coverage percentage and mobile layout were not measured.
