import type Database from 'better-sqlite3';

// RGPD: retenção por prazo (purga), exportação (art. 15.º) e apagamento (art. 17.º).
// Funções puras sobre o SQLite — testáveis com uma DB `:memory:`. Os prazos vêm do
// inventário em docs/GDPR-INVENTARIO.md.

export const DAY_MS = 86_400_000;

/** Prazos de retenção por categoria (ms). Ver docs/GDPR-INVENTARIO.md. */
export const RETENTION = {
  casesMs: 730 * DAY_MS, // 2 anos
  notesMs: 730 * DAY_MS,
  infractionsMs: 365 * DAY_MS, // 1 ano
  quarantineMs: 365 * DAY_MS,
  suggestionsMs: 365 * DAY_MS,
  ticketsMs: 365 * DAY_MS,
  stickyRolesMs: 365 * DAY_MS,
  giveawaysMs: 90 * DAY_MS,
  scheduledOrphanMs: 90 * DAY_MS,
  afkOrphanMs: 90 * DAY_MS,
  statsMs: 365 * DAY_MS,
  activityMs: 90 * DAY_MS, // registo de atividade (join/leave) do dashboard
} as const;

export type PurgeCounts = Record<string, number>;

/** Converte um instante (ms) na data UTC 'YYYY-MM-DD' (formato usado na tabela stats). */
function toDateStr(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/**
 * Apaga os registos que ultrapassaram a retenção. Idempotente. Devolve a contagem de
 * linhas removidas por tabela (só entradas > 0). Deve correr diariamente.
 */
export function purgeExpired(db: Database.Database, guildId: string, now: number): PurgeCounts {
  const counts: PurgeCounts = {};
  const del = (table: string, sql: string, ...params: unknown[]): void => {
    const info = db.prepare(sql).run(...params);
    if (info.changes > 0) counts[table] = info.changes;
  };

  const purge = db.transaction(() => {
    del('cases', `DELETE FROM cases WHERE guild_id = ? AND created_at < ?`, guildId, now - RETENTION.casesMs);
    del('activity_log', `DELETE FROM activity_log WHERE guild_id = ? AND created_at < ?`, guildId, now - RETENTION.activityMs);
    del('notes', `DELETE FROM notes WHERE guild_id = ? AND created_at < ?`, guildId, now - RETENTION.notesMs);
    del(
      'infractions',
      `DELETE FROM infractions WHERE guild_id = ? AND created_at < ?`,
      guildId,
      now - RETENTION.infractionsMs,
    );
    del(
      'quarantine',
      `DELETE FROM quarantine WHERE guild_id = ? AND created_at < ?`,
      guildId,
      now - RETENTION.quarantineMs,
    );
    del(
      'suggestions',
      `DELETE FROM suggestions WHERE guild_id = ? AND created_at < ?`,
      guildId,
      now - RETENTION.suggestionsMs,
    );
    // Tickets já fechados além do prazo (um ticket aberto nunca é purgado).
    del(
      'tickets',
      `DELETE FROM tickets WHERE guild_id = ? AND status != 'open' AND created_at < ?`,
      guildId,
      now - RETENTION.ticketsMs,
    );
    del(
      'sticky_roles',
      `DELETE FROM sticky_roles WHERE guild_id = ? AND updated_at < ?`,
      guildId,
      now - RETENTION.stickyRolesMs,
    );
    // Giveaways terminados há mais de 90 dias.
    del(
      'giveaways',
      `DELETE FROM giveaways WHERE guild_id = ? AND ended = 1 AND end_at < ?`,
      guildId,
      now - RETENTION.giveawaysMs,
    );
    // Ações agendadas "órfãs" que nunca dispararam (o normal é apagarem-se ao executar).
    del(
      'scheduled_actions',
      `DELETE FROM scheduled_actions WHERE guild_id = ? AND execute_at < ?`,
      guildId,
      now - RETENTION.scheduledOrphanMs,
    );
    // AFK esquecido (o normal é limpar-se quando o membro volta).
    del('afk', `DELETE FROM afk WHERE guild_id = ? AND since < ?`, guildId, now - RETENTION.afkOrphanMs);
    // Stats agregados além de 1 ano (comparação lexicográfica de 'YYYY-MM-DD').
    del('stats', `DELETE FROM stats WHERE guild_id = ? AND date < ?`, guildId, toDateStr(now - RETENTION.statsMs));

    // Limpeza de órfãos por cascata (votos/entradas sem pai).
    del(
      'suggestion_votes',
      `DELETE FROM suggestion_votes WHERE suggestion_id NOT IN (SELECT id FROM suggestions)`,
    );
    del(
      'giveaway_entries',
      `DELETE FROM giveaway_entries WHERE giveaway_id NOT IN (SELECT id FROM giveaways)`,
    );
  });
  purge();
  return counts;
}

// ─── Exportação (art. 15.º) ──────────────────────────────────────────────────────

export interface UserExport {
  userId: string;
  guildId: string;
  exportedAt: number;
  cases: Record<string, unknown>[];
  infractions: Record<string, unknown>[];
  quarantine: Record<string, unknown>[];
  stickyRoles: Record<string, unknown>[];
  suggestions: Record<string, unknown>[];
  suggestionVotes: Record<string, unknown>[];
  afk: Record<string, unknown>[];
  birthdays: Record<string, unknown>[];
  giveawayEntries: Record<string, unknown>[];
  tickets: Record<string, unknown>[];
  reminders: Record<string, unknown>[];
  levels: Record<string, unknown> | null;
}

/** Reúne tudo o que a BD tem indexado a um utilizador (só do guild indicado). */
export function exportUserData(db: Database.Database, guildId: string, userId: string): UserExport {
  const rows = (sql: string, ...p: unknown[]): Record<string, unknown>[] =>
    db.prepare(sql).all(...p) as Record<string, unknown>[];

  const levels = db
    .prepare(`SELECT xp FROM levels WHERE guild_id = ? AND user_id = ?`)
    .get(guildId, userId) as Record<string, unknown> | undefined;

  // Casos SEM o moderator_id: quem aplicou a sanção é dado de terceiro e revelá-lo é um
  // vetor de retaliação (art. 15.º/4 + considerando 63 do RGPD permitem retê-lo).
  const cases = rows(
    `SELECT id, guild_id, type, target_id, reason, duration_ms, created_at
       FROM cases WHERE guild_id = ? AND target_id = ?`,
    guildId,
    userId,
  );

  return {
    userId,
    guildId,
    exportedAt: Date.now(),
    cases,
    // NOTA: a tabela `notes` (notas internas de staff sobre o membro) é DELIBERADAMENTE
    // omitida — o schema marca-a como invisível ao membro; expô-la aqui seria uma fuga.
    infractions: rows(`SELECT * FROM infractions WHERE guild_id = ? AND target_id = ?`, guildId, userId),
    quarantine: rows(`SELECT * FROM quarantine WHERE guild_id = ? AND user_id = ?`, guildId, userId),
    stickyRoles: rows(`SELECT * FROM sticky_roles WHERE guild_id = ? AND user_id = ?`, guildId, userId),
    suggestions: rows(`SELECT * FROM suggestions WHERE guild_id = ? AND author_id = ?`, guildId, userId),
    suggestionVotes: rows(`SELECT * FROM suggestion_votes WHERE user_id = ?`, userId),
    afk: rows(`SELECT * FROM afk WHERE guild_id = ? AND user_id = ?`, guildId, userId),
    birthdays: rows(`SELECT * FROM birthdays WHERE guild_id = ? AND user_id = ?`, guildId, userId),
    giveawayEntries: rows(`SELECT * FROM giveaway_entries WHERE user_id = ?`, userId),
    tickets: rows(`SELECT * FROM tickets WHERE guild_id = ? AND opener_id = ?`, guildId, userId),
    reminders: rows(
      `SELECT * FROM scheduled_actions WHERE guild_id = ? AND type = 'reminder' AND target_id = ?`,
      guildId,
      userId,
    ),
    levels: levels ?? null,
  };
}

// ─── Apagamento (art. 17.º) ──────────────────────────────────────────────────────

export interface DeleteResult {
  /** Linhas efetivamente apagadas (dados voluntários), por tabela. */
  deleted: Record<string, number>;
  /** Linhas mantidas por interesse legítimo (moderação), por tabela. */
  kept: Record<string, number>;
}

/**
 * Apaga os dados VOLUNTÁRIOS de um utilizador (aniversário, AFK, lembretes, sugestões e
 * votos, entradas em giveaways, XP). PRESERVA os registos de moderação (casos, notas,
 * infrações, quarentena) — art. 17.º/3-e: necessários à segurança e à defesa de direitos.
 */
export function deleteUserData(
  db: Database.Database,
  guildId: string,
  userId: string,
  now: number,
): DeleteResult {
  const deleted: Record<string, number> = {};
  const rec = (table: string, sql: string, ...p: unknown[]): void => {
    const info = db.prepare(sql).run(...p);
    if (info.changes > 0) deleted[table] = info.changes;
  };

  const run = db.transaction(() => {
    rec('birthdays', `DELETE FROM birthdays WHERE guild_id = ? AND user_id = ?`, guildId, userId);
    rec('afk', `DELETE FROM afk WHERE guild_id = ? AND user_id = ?`, guildId, userId);
    rec('levels', `DELETE FROM levels WHERE guild_id = ? AND user_id = ?`, guildId, userId);
    rec(
      'reminders',
      `DELETE FROM scheduled_actions WHERE guild_id = ? AND type = 'reminder' AND target_id = ?`,
      guildId,
      userId,
    );
    rec('giveaway_entries', `DELETE FROM giveaway_entries WHERE user_id = ?`, userId);
    rec('suggestion_votes', `DELETE FROM suggestion_votes WHERE user_id = ?`, userId);
    rec('suggestions', `DELETE FROM suggestions WHERE guild_id = ? AND author_id = ?`, guildId, userId);
  });
  run();
  // `now` fica disponível para futura auditoria/anonimização; não afeta o apagamento atual.
  void now;

  const count = (sql: string, ...p: unknown[]): number =>
    (db.prepare(sql).get(...p) as { n: number }).n;
  const kept: Record<string, number> = {
    cases: count(`SELECT COUNT(*) AS n FROM cases WHERE guild_id = ? AND target_id = ?`, guildId, userId),
    notes: count(`SELECT COUNT(*) AS n FROM notes WHERE guild_id = ? AND target_id = ?`, guildId, userId),
    infractions: count(
      `SELECT COUNT(*) AS n FROM infractions WHERE guild_id = ? AND target_id = ?`,
      guildId,
      userId,
    ),
    quarantine: count(
      `SELECT COUNT(*) AS n FROM quarantine WHERE guild_id = ? AND user_id = ?`,
      guildId,
      userId,
    ),
  };

  return { deleted, kept };
}

/** Apaga o XP de um utilizador (usado quando o membro sai — minimização de dados). */
export function deleteLevelsForUser(db: Database.Database, guildId: string, userId: string): void {
  db.prepare(`DELETE FROM levels WHERE guild_id = ? AND user_id = ?`).run(guildId, userId);
}

/** Mensagem para o utilizador a resumir o apagamento (o que saiu, o que ficou e porquê). */
export function summarizeDeletion(res: DeleteResult): string {
  const delEntries = Object.entries(res.deleted).filter(([, n]) => n > 0);
  const keptTotal = Object.values(res.kept).reduce((a, b) => a + b, 0);

  const lines: string[] = [];
  if (delEntries.length === 0) {
    lines.push('Não havia nenhum dado voluntário teu para apagar.');
  } else {
    const parts = delEntries.map(([t, n]) => `${n}× ${t}`).join(', ');
    lines.push(`✅ Apagados os teus dados voluntários: ${parts}.`);
  }

  if (keptTotal > 0) {
    const keptParts = Object.entries(res.kept)
      .filter(([, n]) => n > 0)
      .map(([t, n]) => `${n}× ${t}`)
      .join(', ');
    lines.push(
      `ℹ️ Mantidos ${keptParts} por serem registos de moderação — a lei permite conservá-los ` +
        `por interesse legítimo de segurança e defesa de direitos (art. 17.º/3 do RGPD).`,
    );
  }
  return lines.join('\n');
}
