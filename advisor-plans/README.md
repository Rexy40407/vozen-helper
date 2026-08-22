# Vozen Helper improvement plans

Reconciled by a standard `improve` audit on 2026-08-19 against commit
`03f09a8`. The Rust production runtime and CI have advanced since the original
audit; executors must inspect the working tree before applying a plan and stop
if cited state has drifted.

## Execution order and status

| Plan | Title | Priority | Effort | Depends on | Status |
|---|---|---:|---:|---|---|
| 001 | Harden OAuth, sessions and mutation access | P0 | M | — | DONE (Rust security gates pass) |
| 002 | Make feature settings typed, atomic and runtime-backed | P0 | L | 001 | PARTIAL (runtime coverage is broad; parity audit remains) |
| 003 | Close legacy API, import and moderation exposure gaps | P1 | M | 001 | PARTIAL (permissions/mentions fixed; import/legacy retirement remains) |
| 004 | Restore reproducible verification across Node, Rust and panel | P1 | M | — | DONE (all local CI gates pass) |
| 005 | Establish migration parity and reconcile operating docs | P1 | L | 004 | PARTIAL (runtime/docs reconciliation remains) |
| 006 | Refresh the Node lockfile and restore a clean high-severity audit | P1 | S | 004 | DONE (npm audits report 0 vulnerabilities) |
| 007 | Close provider readiness gates | P1 | L | 001, 004, 005 | BLOCKED BY EXTERNAL CREDENTIALS/APPROVAL |

## Dependency notes

- Plan 001 must land first because every later write/configuration test depends on a trustworthy session and mutation boundary.
- Plan 002 should follow 001 so feature saves cannot be tested through an insecure or ambiguous session path.
- Plan 004 can run in parallel with 001, but its new gates should be green before the migration work in 005.
- Plan 003 keeps the rollback Node API safe while Rust becomes canonical.
- Plan 006 is deliberately narrow: it removes the known direct `nanoid`
  advisory without turning a lockfile refresh into a broad runtime upgrade.

## Findings considered and rejected

- Redesigning the panel again was not treated as a separate defect: the current panel work is uncommitted and should be reviewed after the correctness/security plans land.
- Broad dependency upgrades were not recommended; only the direct high-severity
  Node audit finding is planned until an isolated compatibility test supports a
  larger upgrade.
- Micro-optimisations in single-guild hot paths were not prioritised over authentication, runtime truth and verification gates.
