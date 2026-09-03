import type Database from 'better-sqlite3';

// Settings de runtime: overrides guardados na DB que se sobrepõem aos defaults de
// `modConfig` (config-as-code). Escritos pelo painel (API) e lidos pelo bot. Funções
// finas sobre a tabela `settings` — testáveis com DB `:memory:`.

/** Lê o valor cru de uma chave (ou `null` se não existir). */
export function getSetting(db: Database.Database, guildId: string, key: string): string | null {
  const r = db
    .prepare(`SELECT value FROM settings WHERE guild_id = ? AND key = ?`)
    .get(guildId, key) as { value: string } | undefined;
  return r ? r.value : null;
}

/** Grava (upsert) o valor de uma chave. */
export function setSetting(
  db: Database.Database,
  guildId: string,
  key: string,
  value: string,
  now: number,
): void {
  db.prepare(
    `INSERT INTO settings (guild_id, key, value, updated_at) VALUES (?, ?, ?, ?)
     ON CONFLICT(guild_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at`,
  ).run(guildId, key, value, now);
}

/** Remove uma chave. Devolve true se existia. */
export function deleteSetting(db: Database.Database, guildId: string, key: string): boolean {
  return (
    db.prepare(`DELETE FROM settings WHERE guild_id = ? AND key = ?`).run(guildId, key).changes > 0
  );
}

/** Todas as settings de um guild como mapa key -> value. */
export function getAllSettings(db: Database.Database, guildId: string): Record<string, string> {
  const rows = db.prepare(`SELECT key, value FROM settings WHERE guild_id = ?`).all(guildId) as {
    key: string;
    value: string;
  }[];
  const out: Record<string, string> = {};
  for (const r of rows) out[r.key] = r.value;
  return out;
}
