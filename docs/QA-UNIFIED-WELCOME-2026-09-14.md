# Unified welcome — local QA

Scope: user requested one welcome feature instead of two overlapping modules.
Bases: Helper `2329bee8bce3e6b8a93682cacf63061b3fa27beb`; website `a7431d9ea66d0b51e8a6a57d41d28e1ecd80b978` in `vozen-org-ticket-release`.

## Behavior and compatibility

- Only `support.welcome` appears in the API and frontend catalogue (46 visible modules).
- Public greeting, optional guide buttons, DM, automatic role, farewell and templates share one configuration. Member join has one public delivery path and one deduplication claim.
- The old UI route resolves to the unified editor. Legacy detail GET returns the canonical contract. Legacy publishing/preflight rejects writes with HTTP 410 rather than creating an independent module again.
- Guild-scoped read-through compatibility preserves stored legacy configuration without deleting records or rewriting databases at startup. With both modules active, the primary welcome message/channel wins and the guide supplies buttons. With only the guide active, its message/channel/template are used; disabled plain-welcome DM, role and farewell side effects stay off.
- Saving canonical welcome atomically records an authoritative marker together with the normal revision/audit write. Disabling, or restoring a configuration without guide fields after that save, cannot reactivate the archived guide.
- Template rendering retains the bounded renderer and guild feature gating. User-facing messages restrict allowed mentions. The guild's actual name is used where available.

## Verification

- TDD red phase: store compatibility methods and frontend key helpers were absent and tests failed before implementation.
- Rust suites passed across runs: API 65 + integration 1, core 71, Discord 45, store 61, modules 26 (269 total). Windows App Control intermittently returned error 4551 before test execution; normal retries passed, with no protection or policy changes. Final marker-default adjustment additionally passed all three store compatibility tests.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed. `cargo fmt --all -- --check` and diff whitespace checks passed before the last formatter run.
- Website: 14 Vitest tests passed; TypeScript/Vite production build and `ui:check` passed. Existing font-resolution/chunk-size warnings remain.
- Browser local demo, guild `demo`, no production credentials: old `support.welcome_channel` URL opened the unified editor; orientation toggle saved successfully; catalogue showed exactly one Boas-vindas card and 46 modules. Browser tab and dev server closed.
- Production Discord delivery, real member joins and mobile layout were not exercised. Coverage percentage was not measured. No commit, push or deploy was performed for this change.

## Publication and recovery notes

Publish Helper and website together after authorization. Back up live SQLite before activation. No schema change or destructive data migration is introduced. Legacy records are intentionally retained; rolling back the binary can restore the previous two-module behavior, including duplicate messages if both legacy modules were enabled. Do not restore an older database over live writes automatically.

Acceptance after publication: a new member receives one public message, optional working guide buttons, only the configured DM/role/farewell behavior; disabling unified welcome stops both former paths. TikTok remains outside this task.
