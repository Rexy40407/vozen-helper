# Vozen Helper improvement plans

Generated from a standard `improve` audit on 2026-08-01 against commit `b748ea7`.
The repository had pre-existing uncommitted changes; executors must inspect the
working tree before applying any plan and stop if the cited state has drifted.

## Execution order and status

| Plan | Title | Priority | Effort | Depends on | Status |
|---|---|---:|---:|---|---|
| 001 | Harden OAuth, sessions and mutation access | P0 | M | — | TODO |
| 002 | Make feature settings typed, atomic and runtime-backed | P0 | L | 001 | TODO |
| 003 | Close legacy API, import and moderation exposure gaps | P1 | M | 001 | TODO |
| 004 | Restore reproducible verification across Node, Rust and panel | P1 | M | — | TODO |
| 005 | Establish migration parity and reconcile operating docs | P1 | L | 004 | TODO |

## Dependency notes

- Plan 001 must land first because every later write/configuration test depends on a trustworthy session and mutation boundary.
- Plan 002 should follow 001 so feature saves cannot be tested through an insecure or ambiguous session path.
- Plan 004 can run in parallel with 001, but its new gates should be green before the migration work in 005.
- Plan 003 keeps the rollback Node API safe while Rust becomes canonical.

## Findings considered and rejected

- Redesigning the panel again was not treated as a separate defect: the current panel work is uncommitted and should be reviewed after the correctness/security plans land.
- Broad dependency upgrades were not recommended; only reproducibility fixes are planned until the lockfile audit is available.
- Micro-optimisations in single-guild hot paths were not prioritised over authentication, runtime truth and verification gates.
