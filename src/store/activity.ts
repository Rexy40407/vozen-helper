import type Database from 'better-sqlite3';

// Registo de atividade do servidor (dashboard do painel, estilo audit-log mas mais rico).
// Funções finas sobre o SQLite — testáveis com uma DB `:memory:`.

export type ActivityType = 'join' | 'leave' | 'ban' | 'unban' | 'kick';

export interface NewActivity {
  guildId: string;
  /** 'join' | 'leave' | 'ban' | ... (extensível). */
  type: ActivityType | string;
  userId: string;
  /** Snapshot do tag (quem sai deixa de ser resolúvel só pelo id). */
  userTag?: string | null;
  /** Quem executou (moderador), quando aplicável. */
  actorId?: string | null;
  /** Extra por tipo (join: inviteCode/inviterId/…; leave: membershipMs/roles; …). */
  detail?: Record<string, unknown>;
  createdAt: number;
}

export interface ActivityRow {
  id: number;
  type: string;
  userId: string;
  userTag: string | null;
  actorId: string | null;
  detail: Record<string, unknown>;
  createdAt: number;
}

/** Regista um evento de atividade. Devolve o id atribuído. */
export function logActivity(db: Database.Database, e: NewActivity): number {
  const info = db
    .prepare(
      `INSERT INTO activity_log (guild_id, type, user_id, user_tag, actor_id, detail, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      e.guildId,
      e.type,
      e.userId,
      e.userTag ?? null,
      e.actorId ?? null,
      JSON.stringify(e.detail ?? {}),
      e.createdAt,
    );
  return Number(info.lastInsertRowid);
}

function rowToActivity(r: Record<string, unknown>): ActivityRow {
  let detail: Record<string, unknown>;
  try {
    detail = JSON.parse((r.detail as string) || '{}') as Record<string, unknown>;
  } catch {
    detail = {};
  }
  return {
    id: r.id as number,
    type: r.type as string,
    userId: r.user_id as string,
    userTag: (r.user_tag as string | null) ?? null,
    actorId: (r.actor_id as string | null) ?? null,
    detail,
    createdAt: r.created_at as number,
  };
}

/** Eventos mais recentes (opcionalmente filtrados por tipo), do mais recente ao mais antigo. */
export function getRecentActivity(
  db: Database.Database,
  guildId: string,
  limit = 50,
  type?: string,
): ActivityRow[] {
  const rows = type
    ? db
        .prepare(
          `SELECT * FROM activity_log WHERE guild_id = ? AND type = ? ORDER BY id DESC LIMIT ?`,
        )
        .all(guildId, type, limit)
    : db
        .prepare(`SELECT * FROM activity_log WHERE guild_id = ? ORDER BY id DESC LIMIT ?`)
        .all(guildId, limit);
  return rows.map((r) => rowToActivity(r as Record<string, unknown>));
}

/** Nº total de eventos do servidor. */
export function countActivity(db: Database.Database, guildId: string): number {
  const r = db
    .prepare(`SELECT COUNT(*) AS n FROM activity_log WHERE guild_id = ?`)
    .get(guildId) as { n: number };
  return r.n;
}
