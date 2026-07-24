# Rust cutover / rollback runbook

The Node service remains the fallback until the Rust release has passed the seven-day soak.
Build releases in CI, upload them to `/home/vozen/vozen-helper-rust/releases/<version>`, run
`vozen-helper migrate` against the copied SQLite file, and update `current` atomically. The
installed systemd unit is named `vozen-helper.service`; its original Node unit is preserved at
`/home/vozen/vozen-helper-rust/shared/vozen-helper-node.service` before promotion.

Promotion gate:

1. Create a verified SQLite backup before running migrations. The repository helper
   `deploy/verify-backup.py` uses SQLite's online backup API, runs `integrity_check`, copies the
   backup to an isolated restore target, and verifies the restored copy before returning success.
   Keep the resulting file under `shared/backups/` with its SHA256 in the release evidence.
2. Stop `vozen-helper.service` (Node) and confirm no second Discord session is active.
3. Install the release's `vozen-helper-rust.service` as `vozen-helper.service`, reload systemd,
   and start it; check `/health`, `/api/v1/health`, Discord `/ping`,
   `/warn`, `/cases`, guild join/leave and panel session smoke tests.
4. Record PSS, RSS, CPU, interaction latency, DB integrity and journald errors every day.

Rollback:

1. `systemctl stop vozen-helper.service`.
2. If the schema/data requires recovery, restore the last verified SQLite backup to a separate
   copy first and validate it with `PRAGMA integrity_check`; never overwrite the live DB without
   an explicit incident decision.
3. Restore the previous `current` symlink, install the saved Node unit, reload systemd, and start
   `vozen-helper.service`.
4. Do not delete the Rust release, backup or SQLite WAL; preserve evidence for diagnosis.

Never run both gateways with the same Discord token.
