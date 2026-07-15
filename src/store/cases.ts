import type Database from 'better-sqlite3';

// Acesso à DB para casos, notas, strikes e ações agendadas (Fase 2). Funções finas
// sobre o SQLite — testáveis com uma DB `:memory:`.

export type CaseType =
  | 'warn'
  | 'timeout'
  | 'untimeout'
  | 'kick'
  | 'ban'
  | 'tempban'
  | 'unban'
  | 'softban'
  | 'quarantine'
  | 'unquarantine';

export interface ModCase {
  id: number;
  guildId: string;
  type: CaseType;
  targetId: string;
  moderatorId: string;
  reason: string;
  durationMs: number | null;
  createdAt: number;
}

export interface NewCase {
  guildId: string;
  type: CaseType;
  targetId: string;
  moderatorId: string;
  reason?: string;
  durationMs?: number | null;
  createdAt: number;
}

/** Insere um caso e devolve o nº atribuído (id AUTOINCREMENT). */
export function insertCase(db: Database.Database, c: NewCase): number {
  const info = db
    .prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, duration_ms, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(c.guildId, c.type, c.targetId, c.moderatorId, c.reason ?? '', c.durationMs ?? null, c.createdAt);
  return Number(info.lastInsertRowid);
}

function rowToCase(r: Record<string, unknown>): ModCase {
  return {
    id: r.id as number,
    guildId: r.guild_id as string,
    type: r.type as CaseType,
    targetId: r.target_id as string,
    moderatorId: r.moderator_id as string,
    reason: r.reason as string,
    durationMs: (r.duration_ms as number | null) ?? null,
    createdAt: r.created_at as number,
  };
}

/** Casos de um utilizador, do mais recente ao mais antigo (limite opcional). */
export function getCasesForUser(
  db: Database.Database,
  guildId: string,
  targetId: string,
  limit = 25,
): ModCase[] {
  return db
    .prepare(
      `SELECT * FROM cases WHERE guild_id = ? AND target_id = ? ORDER BY id DESC LIMIT ?`,
    )
    .all(guildId, targetId, limit)
    .map((r) => rowToCase(r as Record<string, unknown>));
}

/** Casos mais recentes do servidor (todos os utilizadores), do mais recente ao mais antigo. */
export function getRecentCases(db: Database.Database, guildId: string, limit = 50): ModCase[] {
  return db
    .prepare(`SELECT * FROM cases WHERE guild_id = ? ORDER BY id DESC LIMIT ?`)
    .all(guildId, limit)
    .map((r) => rowToCase(r as Record<string, unknown>));
}

/** Nº total de casos do servidor. */
export function countCases(db: Database.Database, guildId: string): number {
  const r = db
    .prepare(`SELECT COUNT(*) AS n FROM cases WHERE guild_id = ?`)
    .get(guildId) as { n: number };
  return r.n;
}

/** Obtém um caso por nº. */
export function getCase(db: Database.Database, guildId: string, id: number): ModCase | null {
  const r = db.prepare(`SELECT * FROM cases WHERE guild_id = ? AND id = ?`).get(guildId, id);
  return r ? rowToCase(r as Record<string, unknown>) : null;
}

/** Edita a razão de um caso. Devolve true se o caso existia. */
export function editCaseReason(
  db: Database.Database,
  guildId: string,
  id: number,
  reason: string,
): boolean {
  const info = db
    .prepare(`UPDATE cases SET reason = ? WHERE guild_id = ? AND id = ?`)
    .run(reason, guildId, id);
  return info.changes > 0;
}

// ─── Notas ────────────────────────────────────────────────────────────────────

export function addNote(
  db: Database.Database,
  guildId: string,
  targetId: string,
  authorId: string,
  content: string,
  createdAt: number,
): number {
  const info = db
    .prepare(
      `INSERT INTO notes (guild_id, target_id, author_id, content, created_at) VALUES (?, ?, ?, ?, ?)`,
    )
    .run(guildId, targetId, authorId, content, createdAt);
  return Number(info.lastInsertRowid);
}

export interface StaffNote {
  id: number;
  authorId: string;
  content: string;
  createdAt: number;
}

export function getNotes(db: Database.Database, guildId: string, targetId: string): StaffNote[] {
  return db
    .prepare(`SELECT * FROM notes WHERE guild_id = ? AND target_id = ? ORDER BY id DESC`)
    .all(guildId, targetId)
    .map((r) => {
      const row = r as Record<string, unknown>;
      return {
        id: row.id as number,
        authorId: row.author_id as string,
        content: row.content as string,
        createdAt: row.created_at as number,
      };
    });
}

// ─── Strikes / infrações ────────────────────────────────────────────────────────

/** Regista um strike. */
export function addInfraction(
  db: Database.Database,
  guildId: string,
  targetId: string,
  createdAt: number,
  weight = 1,
  source = 'manual',
): void {
  db.prepare(
    `INSERT INTO infractions (guild_id, target_id, weight, source, created_at) VALUES (?, ?, ?, ?, ?)`,
  ).run(guildId, targetId, weight, source, createdAt);
}

/** Soma dos strikes de um utilizador dentro da janela [since, ∞). */
export function countInfractions(
  db: Database.Database,
  guildId: string,
  targetId: string,
  since: number,
): number {
  const r = db
    .prepare(
      `SELECT COALESCE(SUM(weight), 0) AS total FROM infractions
       WHERE guild_id = ? AND target_id = ? AND created_at >= ?`,
    )
    .get(guildId, targetId, since) as { total: number };
  return r.total;
}

// ─── Ações agendadas (expirações) ───────────────────────────────────────────────

export type ScheduledType =
  | 'unban'
  | 'untimeout'
  | 'unmute'
  | 'reminder'
  | 'giveaway_end'
  | 'birthday_role_remove'
  | 'bump_reminder';

export interface ScheduledAction {
  id: number;
  guildId: string;
  type: ScheduledType;
  targetId: string;
  executeAt: number;
  payload: string;
  caseId: number | null;
}

export function scheduleAction(
  db: Database.Database,
  a: Omit<ScheduledAction, 'id'>,
): number {
  const info = db
    .prepare(
      `INSERT INTO scheduled_actions (guild_id, type, target_id, execute_at, payload, case_id)
       VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .run(a.guildId, a.type, a.targetId, a.executeAt, a.payload, a.caseId ?? null);
  return Number(info.lastInsertRowid);
}

/** Ações cujo tempo já passou (execute_at <= now). */
export function getDueActions(db: Database.Database, now: number): ScheduledAction[] {
  return db
    .prepare(`SELECT * FROM scheduled_actions WHERE execute_at <= ? ORDER BY execute_at ASC`)
    .all(now)
    .map((r) => {
      const row = r as Record<string, unknown>;
      return {
        id: row.id as number,
        guildId: row.guild_id as string,
        type: row.type as ScheduledType,
        targetId: row.target_id as string,
        executeAt: row.execute_at as number,
        payload: row.payload as string,
        caseId: (row.case_id as number | null) ?? null,
      };
    });
}

export function deleteScheduled(db: Database.Database, id: number): void {
  db.prepare(`DELETE FROM scheduled_actions WHERE id = ?`).run(id);
}

/** Remove ações agendadas pendentes de um tipo para um alvo (ex.: unban manual cancela o auto-unban). */
export function cancelScheduled(
  db: Database.Database,
  guildId: string,
  type: ScheduledType,
  targetId: string,
): void {
  db.prepare(`DELETE FROM scheduled_actions WHERE guild_id = ? AND type = ? AND target_id = ?`).run(
    guildId,
    type,
    targetId,
  );
}

/** Cancela ações agendadas de um tipo cujo payload é EXATAMENTE `payload`
 *  (ex.: giveaway_end de UM giveaway específico, cujo payload é o id). */
export function cancelScheduledByPayload(
  db: Database.Database,
  guildId: string,
  type: ScheduledType,
  payload: string,
): void {
  db.prepare(`DELETE FROM scheduled_actions WHERE guild_id = ? AND type = ? AND payload = ?`).run(
    guildId,
    type,
    payload,
  );
}
