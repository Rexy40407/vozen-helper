import type Database from 'better-sqlite3';

// Acessos à DB para as features de comunidade (vagas A-D). Funções finas sobre o
// SQLite, testáveis com uma DB `:memory:`.

// ─── Sugestões ──────────────────────────────────────────────────────────────────
export type SuggestionStatus = 'pending' | 'approved' | 'denied' | 'considered';

export interface Suggestion {
  id: number;
  guildId: string;
  authorId: string;
  content: string;
  messageId: string | null;
  status: SuggestionStatus;
  createdAt: number;
}

export function createSuggestion(
  db: Database.Database,
  guildId: string,
  authorId: string,
  content: string,
  createdAt: number,
): number {
  const info = db
    .prepare(`INSERT INTO suggestions (guild_id, author_id, content, created_at) VALUES (?, ?, ?, ?)`)
    .run(guildId, authorId, content, createdAt);
  return Number(info.lastInsertRowid);
}

export function setSuggestionMessage(db: Database.Database, id: number, messageId: string): void {
  db.prepare(`UPDATE suggestions SET message_id = ? WHERE id = ?`).run(messageId, id);
}

export function getSuggestion(db: Database.Database, id: number): Suggestion | null {
  const r = db.prepare(`SELECT * FROM suggestions WHERE id = ?`).get(id) as
    | Record<string, unknown>
    | undefined;
  if (!r) return null;
  return {
    id: r.id as number,
    guildId: r.guild_id as string,
    authorId: r.author_id as string,
    content: r.content as string,
    messageId: (r.message_id as string | null) ?? null,
    status: r.status as SuggestionStatus,
    createdAt: r.created_at as number,
  };
}

export function setSuggestionStatus(
  db: Database.Database,
  id: number,
  status: SuggestionStatus,
): boolean {
  return db.prepare(`UPDATE suggestions SET status = ? WHERE id = ?`).run(status, id).changes > 0;
}

/** Regista/atualiza o voto de um utilizador (1 ou -1). */
export function voteSuggestion(
  db: Database.Database,
  suggestionId: number,
  userId: string,
  vote: 1 | -1,
): void {
  db.prepare(
    `INSERT INTO suggestion_votes (suggestion_id, user_id, vote) VALUES (?, ?, ?)
     ON CONFLICT(suggestion_id, user_id) DO UPDATE SET vote = excluded.vote`,
  ).run(suggestionId, userId, vote);
}

export function countSuggestionVotes(
  db: Database.Database,
  suggestionId: number,
): { up: number; down: number } {
  const up = db
    .prepare(`SELECT COUNT(*) AS n FROM suggestion_votes WHERE suggestion_id = ? AND vote = 1`)
    .get(suggestionId) as { n: number };
  const down = db
    .prepare(`SELECT COUNT(*) AS n FROM suggestion_votes WHERE suggestion_id = ? AND vote = -1`)
    .get(suggestionId) as { n: number };
  return { up: up.n, down: down.n };
}

// ─── AFK ────────────────────────────────────────────────────────────────────────
export interface AfkState {
  reason: string;
  since: number;
}

export function setAfk(
  db: Database.Database,
  guildId: string,
  userId: string,
  reason: string,
  since: number,
): void {
  db.prepare(
    `INSERT INTO afk (guild_id, user_id, reason, since) VALUES (?, ?, ?, ?)
     ON CONFLICT(guild_id, user_id) DO UPDATE SET reason = excluded.reason, since = excluded.since`,
  ).run(guildId, userId, reason, since);
}

export function getAfk(db: Database.Database, guildId: string, userId: string): AfkState | null {
  const r = db
    .prepare(`SELECT reason, since FROM afk WHERE guild_id = ? AND user_id = ?`)
    .get(guildId, userId) as { reason: string; since: number } | undefined;
  return r ? { reason: r.reason, since: r.since } : null;
}

export function clearAfk(db: Database.Database, guildId: string, userId: string): boolean {
  return db.prepare(`DELETE FROM afk WHERE guild_id = ? AND user_id = ?`).run(guildId, userId).changes > 0;
}

// ─── Tags ─────────────────────────────────────────────────────────────────────
export function setTag(
  db: Database.Database,
  guildId: string,
  name: string,
  content: string,
  authorId: string,
  createdAt: number,
): void {
  db.prepare(
    `INSERT INTO tags (guild_id, name, content, author_id, created_at) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(guild_id, name) DO UPDATE SET content = excluded.content`,
  ).run(guildId, name.toLowerCase(), content, authorId, createdAt);
}

export function getTag(db: Database.Database, guildId: string, name: string): string | null {
  const r = db
    .prepare(`SELECT content FROM tags WHERE guild_id = ? AND name = ?`)
    .get(guildId, name.toLowerCase()) as { content: string } | undefined;
  return r ? r.content : null;
}

export function deleteTag(db: Database.Database, guildId: string, name: string): boolean {
  return db.prepare(`DELETE FROM tags WHERE guild_id = ? AND name = ?`).run(guildId, name.toLowerCase())
    .changes > 0;
}

export function listTags(db: Database.Database, guildId: string): string[] {
  return db
    .prepare(`SELECT name FROM tags WHERE guild_id = ? ORDER BY name`)
    .all(guildId)
    .map((r) => (r as { name: string }).name);
}

// ─── Aniversários ────────────────────────────────────────────────────────────────
export function setBirthday(
  db: Database.Database,
  guildId: string,
  userId: string,
  day: number,
  month: number,
): void {
  db.prepare(
    `INSERT INTO birthdays (guild_id, user_id, day, month) VALUES (?, ?, ?, ?)
     ON CONFLICT(guild_id, user_id) DO UPDATE SET day = excluded.day, month = excluded.month`,
  ).run(guildId, userId, day, month);
}

export function clearBirthday(db: Database.Database, guildId: string, userId: string): boolean {
  return db.prepare(`DELETE FROM birthdays WHERE guild_id = ? AND user_id = ?`).run(guildId, userId)
    .changes > 0;
}

export function getBirthdaysOn(
  db: Database.Database,
  guildId: string,
  day: number,
  month: number,
): string[] {
  return db
    .prepare(`SELECT user_id FROM birthdays WHERE guild_id = ? AND day = ? AND month = ?`)
    .all(guildId, day, month)
    .map((r) => (r as { user_id: string }).user_id);
}

// ─── Self-roles ─────────────────────────────────────────────────────────────────
export type SelfRoleMode = 'normal' | 'unique' | 'verify';

export function setSelfRole(
  db: Database.Database,
  messageId: string,
  customId: string,
  roleId: string,
  mode: SelfRoleMode,
): void {
  db.prepare(
    `INSERT INTO self_roles (message_id, custom_id, role_id, mode) VALUES (?, ?, ?, ?)
     ON CONFLICT(message_id, custom_id) DO UPDATE SET role_id = excluded.role_id, mode = excluded.mode`,
  ).run(messageId, customId, roleId, mode);
}

export function getSelfRole(
  db: Database.Database,
  messageId: string,
  customId: string,
): { roleId: string; mode: SelfRoleMode } | null {
  const r = db
    .prepare(`SELECT role_id, mode FROM self_roles WHERE message_id = ? AND custom_id = ?`)
    .get(messageId, customId) as { role_id: string; mode: SelfRoleMode } | undefined;
  return r ? { roleId: r.role_id, mode: r.mode } : null;
}

export function getSelfRolesForMessage(
  db: Database.Database,
  messageId: string,
): Array<{ customId: string; roleId: string; mode: SelfRoleMode }> {
  return db
    .prepare(`SELECT custom_id, role_id, mode FROM self_roles WHERE message_id = ?`)
    .all(messageId)
    .map((r) => {
      const row = r as { custom_id: string; role_id: string; mode: SelfRoleMode };
      return { customId: row.custom_id, roleId: row.role_id, mode: row.mode };
    });
}

// ─── Giveaways ──────────────────────────────────────────────────────────────────
export interface Giveaway {
  id: number;
  guildId: string;
  channelId: string;
  messageId: string | null;
  prize: string;
  winners: number;
  endAt: number;
  ended: boolean;
  requiredRoleId: string | null;
  hostId: string;
  createdAt: number;
}

function rowToGiveaway(r: Record<string, unknown>): Giveaway {
  return {
    id: r.id as number,
    guildId: r.guild_id as string,
    channelId: r.channel_id as string,
    messageId: (r.message_id as string | null) ?? null,
    prize: r.prize as string,
    winners: r.winners as number,
    endAt: r.end_at as number,
    ended: (r.ended as number) === 1,
    requiredRoleId: (r.required_role_id as string | null) ?? null,
    hostId: r.host_id as string,
    createdAt: r.created_at as number,
  };
}

export function createGiveaway(
  db: Database.Database,
  g: Omit<Giveaway, 'id' | 'messageId' | 'ended'>,
): number {
  const info = db
    .prepare(
      `INSERT INTO giveaways (guild_id, channel_id, prize, winners, end_at, required_role_id, host_id, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(g.guildId, g.channelId, g.prize, g.winners, g.endAt, g.requiredRoleId, g.hostId, g.createdAt);
  return Number(info.lastInsertRowid);
}

export function setGiveawayMessage(db: Database.Database, id: number, messageId: string): void {
  db.prepare(`UPDATE giveaways SET message_id = ? WHERE id = ?`).run(messageId, id);
}

export function getGiveaway(db: Database.Database, id: number): Giveaway | null {
  const r = db.prepare(`SELECT * FROM giveaways WHERE id = ?`).get(id) as
    | Record<string, unknown>
    | undefined;
  return r ? rowToGiveaway(r) : null;
}

export function markGiveawayEnded(db: Database.Database, id: number): void {
  db.prepare(`UPDATE giveaways SET ended = 1 WHERE id = ?`).run(id);
}

export function listActiveGiveaways(db: Database.Database, guildId: string): Giveaway[] {
  return db
    .prepare(`SELECT * FROM giveaways WHERE guild_id = ? AND ended = 0 ORDER BY end_at ASC`)
    .all(guildId)
    .map((r) => rowToGiveaway(r as Record<string, unknown>));
}

export function addGiveawayEntry(db: Database.Database, giveawayId: number, userId: string): boolean {
  const info = db
    .prepare(`INSERT OR IGNORE INTO giveaway_entries (giveaway_id, user_id) VALUES (?, ?)`)
    .run(giveawayId, userId);
  return info.changes > 0;
}

export function removeGiveawayEntry(db: Database.Database, giveawayId: number, userId: string): void {
  db.prepare(`DELETE FROM giveaway_entries WHERE giveaway_id = ? AND user_id = ?`).run(giveawayId, userId);
}

export function getGiveawayEntries(db: Database.Database, giveawayId: number): string[] {
  return db
    .prepare(`SELECT user_id FROM giveaway_entries WHERE giveaway_id = ?`)
    .all(giveawayId)
    .map((r) => (r as { user_id: string }).user_id);
}

// ─── Leveling ──────────────────────────────────────────────────────────────────
export function addXp(db: Database.Database, guildId: string, userId: string, amount: number): number {
  db.prepare(
    `INSERT INTO levels (guild_id, user_id, xp) VALUES (?, ?, ?)
     ON CONFLICT(guild_id, user_id) DO UPDATE SET xp = xp + excluded.xp`,
  ).run(guildId, userId, amount);
  return getXp(db, guildId, userId);
}

export function getXp(db: Database.Database, guildId: string, userId: string): number {
  const r = db
    .prepare(`SELECT xp FROM levels WHERE guild_id = ? AND user_id = ?`)
    .get(guildId, userId) as { xp: number } | undefined;
  return r ? r.xp : 0;
}

/** Posição no ranking (1 = mais XP). 0 se não tem XP. */
export function getRank(db: Database.Database, guildId: string, userId: string): number {
  const xp = getXp(db, guildId, userId);
  if (xp === 0) return 0;
  const r = db
    .prepare(`SELECT COUNT(*) AS n FROM levels WHERE guild_id = ? AND xp > ?`)
    .get(guildId, xp) as { n: number };
  return r.n + 1;
}

export function getLeaderboard(
  db: Database.Database,
  guildId: string,
  limit: number,
  offset = 0,
): Array<{ userId: string; xp: number }> {
  return db
    .prepare(`SELECT user_id, xp FROM levels WHERE guild_id = ? ORDER BY xp DESC LIMIT ? OFFSET ?`)
    .all(guildId, limit, offset)
    .map((r) => {
      const row = r as { user_id: string; xp: number };
      return { userId: row.user_id, xp: row.xp };
    });
}

// ─── Tickets ────────────────────────────────────────────────────────────────────
export interface Ticket {
  id: number;
  channelId: string;
  openerId: string;
  status: string;
  claimedBy: string | null;
}

export function createTicket(
  db: Database.Database,
  guildId: string,
  channelId: string,
  openerId: string,
  createdAt: number,
): number {
  const info = db
    .prepare(
      `INSERT INTO tickets (guild_id, channel_id, opener_id, created_at) VALUES (?, ?, ?, ?)`,
    )
    .run(guildId, channelId, openerId, createdAt);
  return Number(info.lastInsertRowid);
}

export function getOpenTicketForUser(
  db: Database.Database,
  guildId: string,
  openerId: string,
): Ticket | null {
  const r = db
    .prepare(
      `SELECT id, channel_id, opener_id, status, claimed_by FROM tickets
       WHERE guild_id = ? AND opener_id = ? AND status = 'open' LIMIT 1`,
    )
    .get(guildId, openerId) as Record<string, unknown> | undefined;
  if (!r) return null;
  return {
    id: r.id as number,
    channelId: r.channel_id as string,
    openerId: r.opener_id as string,
    status: r.status as string,
    claimedBy: (r.claimed_by as string | null) ?? null,
  };
}

export function getTicketByChannel(db: Database.Database, channelId: string): Ticket | null {
  const r = db
    .prepare(`SELECT id, channel_id, opener_id, status, claimed_by FROM tickets WHERE channel_id = ?`)
    .get(channelId) as Record<string, unknown> | undefined;
  if (!r) return null;
  return {
    id: r.id as number,
    channelId: r.channel_id as string,
    openerId: r.opener_id as string,
    status: r.status as string,
    claimedBy: (r.claimed_by as string | null) ?? null,
  };
}

export function setTicketStatus(db: Database.Database, id: number, status: string): void {
  db.prepare(`UPDATE tickets SET status = ? WHERE id = ?`).run(status, id);
}

export function claimTicket(db: Database.Database, id: number, userId: string): void {
  db.prepare(`UPDATE tickets SET claimed_by = ? WHERE id = ?`).run(userId, id);
}

// ─── Starboard ──────────────────────────────────────────────────────────────────
export interface StarEntry {
  starboardMessageId: string;
  starCount: number;
}

export function getStarEntry(
  db: Database.Database,
  guildId: string,
  originalId: string,
): StarEntry | null {
  const r = db
    .prepare(
      `SELECT starboard_message_id, star_count FROM starboard WHERE guild_id = ? AND original_message_id = ?`,
    )
    .get(guildId, originalId) as { starboard_message_id: string; star_count: number } | undefined;
  return r ? { starboardMessageId: r.starboard_message_id, starCount: r.star_count } : null;
}

export function upsertStarEntry(
  db: Database.Database,
  guildId: string,
  originalId: string,
  starboardMessageId: string,
  count: number,
): void {
  db.prepare(
    `INSERT INTO starboard (guild_id, original_message_id, starboard_message_id, star_count)
     VALUES (?, ?, ?, ?)
     ON CONFLICT(guild_id, original_message_id)
       DO UPDATE SET starboard_message_id = excluded.starboard_message_id, star_count = excluded.star_count`,
  ).run(guildId, originalId, starboardMessageId, count);
}

export function deleteStarEntry(db: Database.Database, guildId: string, originalId: string): void {
  db.prepare(`DELETE FROM starboard WHERE guild_id = ? AND original_message_id = ?`).run(
    guildId,
    originalId,
  );
}

// ─── Estatísticas ────────────────────────────────────────────────────────────────
export type StatField = 'messages' | 'joins' | 'leaves';

export function incrStat(
  db: Database.Database,
  guildId: string,
  date: string,
  field: StatField,
): void {
  // O nome da coluna vem de um union fechado (não input externo) → seguro interpolar.
  db.prepare(
    `INSERT INTO stats (guild_id, date, ${field}) VALUES (?, ?, 1)
     ON CONFLICT(guild_id, date) DO UPDATE SET ${field} = ${field} + 1`,
  ).run(guildId, date);
}

export function getStatsTotals(
  db: Database.Database,
  guildId: string,
): { messages: number; joins: number; leaves: number } {
  const r = db
    .prepare(
      `SELECT COALESCE(SUM(messages),0) AS m, COALESCE(SUM(joins),0) AS j, COALESCE(SUM(leaves),0) AS l
       FROM stats WHERE guild_id = ?`,
    )
    .get(guildId) as { m: number; j: number; l: number };
  return { messages: r.m, joins: r.j, leaves: r.l };
}
