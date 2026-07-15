import type Database from 'better-sqlite3';

// Persistência da quarentena: cargos removidos a uma conta suspeita, guardados para
// restauro. Sobrevive a restart (uma quarentena a meio de um nuke não se pode perder).

export function saveQuarantine(
  db: Database.Database,
  guildId: string,
  userId: string,
  roleIds: readonly string[],
  reason: string,
  now: number,
): void {
  db.prepare(
    `INSERT INTO quarantine (guild_id, user_id, role_ids, reason, created_at) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(guild_id, user_id) DO UPDATE SET role_ids = excluded.role_ids, reason = excluded.reason`,
  ).run(guildId, userId, JSON.stringify([...roleIds]), reason, now);
}

export function getQuarantine(
  db: Database.Database,
  guildId: string,
  userId: string,
): { roleIds: string[]; reason: string } | null {
  const row = db
    .prepare(`SELECT role_ids, reason FROM quarantine WHERE guild_id = ? AND user_id = ?`)
    .get(guildId, userId) as { role_ids: string; reason: string } | undefined;
  if (!row) return null;
  let roleIds: string[] = [];
  try {
    const parsed = JSON.parse(row.role_ids);
    if (Array.isArray(parsed)) roleIds = parsed.filter((x): x is string => typeof x === 'string');
  } catch {
    roleIds = [];
  }
  return { roleIds, reason: row.reason };
}

export function clearQuarantine(db: Database.Database, guildId: string, userId: string): void {
  db.prepare(`DELETE FROM quarantine WHERE guild_id = ? AND user_id = ?`).run(guildId, userId);
}

export function isQuarantined(db: Database.Database, guildId: string, userId: string): boolean {
  return getQuarantine(db, guildId, userId) !== null;
}
