# Plan 006: Refresh the Node lockfile and restore a clean high-severity audit

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. If a
> STOP condition occurs, stop and report — do not improvise. When complete,
> update this plan's row in `advisor-plans/README.md`.
>
> **Drift check (run first)**:
>
> ```powershell
> git diff --stat 03f09a8..HEAD -- package.json package-lock.json .github/workflows/ci.yml
> ```
>
> If `nanoid` or the audit gate have already changed, re-run the audit before
> deciding whether this plan remains necessary.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: `advisor-plans/004-reproducible-verification.md`
- **Category**: dependencies / dx
- **Planned at**: commit `03f09a8`, 2026-08-19
- **Implementation**: complete; root and panel npm audit gates report zero
  vulnerabilities in the current checkout.

## Why this matters

The Helper root manifest directly permits `nanoid ^3.3.17`. The audit observed
during this review identified the pre-fixed Nano ID range as a high-severity
advisory. CI correctly fails on high audit findings, but a stale lockfile makes
that result non-reproducible and blocks trustworthy releases. This is a narrow
lockfile repair, not permission for a broad `npm audit fix` upgrade.

## Current state

- `package.json:44-55` contains the direct dev dependency:

  ```json
  "devDependencies": {
    "nanoid": "^3.3.17",
    "prettier": "^3.9.4",
    "tsx": "^4.19.2",
    "vitest": "^3.0.5"
  }
  ```

- `.github/workflows/ci.yml:26-31` runs `npm ci`, `npm audit
  --audit-level=high`, lint, typecheck, build, and Vitest. Keep this gate; do
  not lower its severity threshold.
- `package.json:34-43` already documents that `npm audit fix --force` is not
  acceptable because it may downgrade Discord.js. Preserve that policy.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Clean install | `npm ci` | exit 0 |
| Dependency audit | `npm audit --audit-level=high` | exit 0, no high/critical findings |
| Lint | `npm run lint` | exit 0 |
| Typecheck | `npm run typecheck` | exit 0 |
| Build | `npm run build` | exit 0 |
| Tests | `npm test` | all pass |

## Scope

**In scope**:

- `package.json`
- `package-lock.json`
- an existing dependency/audit note only if it needs an exact updated version

**Out of scope**:

- Major Discord.js, Fastify, TypeScript, or Node upgrades.
- `npm audit fix --force`.
- Rust dependency changes and production service deployment.

## Git workflow

- Branch: `advisor/006-node-audit-baseline`.
- One focused commit, for example `chore(deps): refresh helper nanoid audit fix`.

## Steps

### Step 1: Identify the resolved vulnerable edge

On a clean branch, run `npm ci` and `npm audit --json`. Record only package
names/versions/advisory IDs in the PR, never cache or registry credentials.
Use `npm ls nanoid` to show whether the vulnerable package is direct or
transitive.

**Verify**: audit output identifies a concrete resolution path; if no
high/critical result remains, mark this plan REJECTED as independently fixed.

### Step 2: Upgrade only to the minimal compatible fixed version

Update the direct `nanoid` range or a targeted override to the smallest
compatible fixed release indicated by the audit. Regenerate `package-lock.json`
with the repository's Node 22 toolchain. Do not use force mode and do not alter
unrelated direct dependencies.

**Verify**: `git diff -- package.json package-lock.json` contains only the
targeted package/necessary lock resolution changes.

### Step 3: Prove the normal CI chain remains reproducible

Run the full Commands table from a removed/reinstalled `node_modules` state.
Do not edit CI unless it has ceased to run the existing high-severity audit.

**Verify**: every command exits 0 and `git diff --check` exits 0.

## Test plan

- Clean installation resolves the intended fixed version.
- `npm audit --audit-level=high` exits 0.
- Existing lint/typecheck/build/Vitest coverage remains green.

## Done criteria

- [ ] The locked graph has no high/critical npm audit finding.
- [ ] The upgrade is minimal and does not invoke force mode.
- [ ] All Node CI commands pass from a clean install.
- [ ] No files outside the in-scope list changed.

## STOP conditions

- The only available fix requires a breaking Discord.js/Fastify/Node upgrade.
- Audit still finds a high/critical issue outside Nano ID after the targeted
  fix; split it into its own plan rather than widening this one.
- Native `better-sqlite3` cannot build on the documented Node version.

## Maintenance notes

- Keep the audit threshold at high; do not hide advisories to unblock deploys.
- Re-run this narrow procedure after Node toolchain updates.
