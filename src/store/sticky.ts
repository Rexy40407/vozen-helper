import type Database from 'better-sqlite3';

// Sticky roles: cargos guardados quando um membro sai, para repor no rejoin. Os IDs
// ficam em JSON (lista curta). Testável com DB `:memory:`.

export function saveStickyRoles(
  db: Database.Database,
  guildId: string,
  userId: string,
  roleIds: readonly string[],
  now: number,
): void {
  db.prepare(
    `INSERT INTO sticky_roles (guild_id, user_id, role_ids, updated_at) VALUES (?, ?, ?, ?)
     ON CONFLICT(guild_id, user_id) DO UPDATE SET role_ids = excluded.role_ids, updated_at = excluded.updated_at`,
  ).run(guildId, userId, JSON.stringify([...roleIds]), now);
}

export function getStickyRoles(db: Database.Database, guildId: string, userId: string): string[] {
  const row = db
    .prepare(`SELECT role_ids FROM sticky_roles WHERE guild_id = ? AND user_id = ?`)
    .get(guildId, userId) as { role_ids: string } | undefined;
  if (!row) return [];
  try {
    const parsed = JSON.parse(row.role_ids);
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

export function clearStickyRoles(db: Database.Database, guildId: string, userId: string): void {
  db.prepare(`DELETE FROM sticky_roles WHERE guild_id = ? AND user_id = ?`).run(guildId, userId);
}
