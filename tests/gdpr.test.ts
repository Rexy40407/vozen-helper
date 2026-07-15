import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import {
  DAY_MS,
  RETENTION,
  purgeExpired,
  exportUserData,
  deleteUserData,
  deleteLevelsForUser,
  summarizeDeletion,
} from '../src/store/gdpr.js';

// RGPD (Fase 3+4): retenção por prazo, exportação (art. 15.º) e apagamento (art. 17.º)
// com preservação fundamentada dos registos de moderação.

const G = '111111111111111111';
const U = '222222222222222222';
const OTHER = '333333333333333333';
let db: Database.Database;

beforeEach(() => {
  db = initDb(':memory:');
});

function dateStr(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

describe('purgeExpired — retenção por prazo', () => {
  it('apaga casos além de 2 anos e mantém os recentes', () => {
    const now = 1_800_000_000_000;
    const old = now - RETENTION.casesMs - DAY_MS;
    const recent = now - 10 * DAY_MS;
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'warn', U, OTHER, 'antigo', old);
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'warn', U, OTHER, 'recente', recent);

    const counts = purgeExpired(db, G, now);

    expect(counts.cases).toBe(1);
    const left = db.prepare(`SELECT reason FROM cases`).all() as { reason: string }[];
    expect(left).toEqual([{ reason: 'recente' }]);
  });

  it('apaga stats agregados além de 1 ano (comparação por data)', () => {
    const now = 1_800_000_000_000;
    const oldDate = dateStr(now - RETENTION.statsMs - 5 * DAY_MS);
    const freshDate = dateStr(now - 10 * DAY_MS);
    db.prepare(`INSERT INTO stats (guild_id, date, messages) VALUES (?,?,?)`).run(G, oldDate, 5);
    db.prepare(`INSERT INTO stats (guild_id, date, messages) VALUES (?,?,?)`).run(G, freshDate, 9);

    const counts = purgeExpired(db, G, now);

    expect(counts.stats).toBe(1);
    expect(db.prepare(`SELECT date FROM stats`).all()).toEqual([{ date: freshDate }]);
  });

  it('apaga giveaways terminados > 90d e as suas entradas órfãs', () => {
    const now = 1_800_000_000_000;
    const info = db
      .prepare(
        `INSERT INTO giveaways (guild_id, channel_id, prize, end_at, ended, host_id, created_at) VALUES (?,?,?,?,?,?,?)`,
      )
      .run(G, 'c', 'premio', now - RETENTION.giveawaysMs - DAY_MS, 1, OTHER, now - 200 * DAY_MS);
    const gid = Number(info.lastInsertRowid);
    db.prepare(`INSERT INTO giveaway_entries (giveaway_id, user_id) VALUES (?,?)`).run(gid, U);

    const counts = purgeExpired(db, G, now);

    expect(counts.giveaways).toBe(1);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM giveaway_entries`).get()).toEqual({ n: 0 });
  });
});

describe('exportUserData — direito de acesso (art. 15.º)', () => {
  it('devolve tudo o que está indexado ao utilizador e nada de terceiros', () => {
    const now = 1_800_000_000_000;
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'warn', U, OTHER, 'meu caso', now);
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'warn', OTHER, OTHER, 'caso de outro', now);
    db.prepare(`INSERT INTO birthdays (guild_id, user_id, day, month) VALUES (?,?,?,?)`).run(G, U, 3, 4);
    db.prepare(`INSERT INTO levels (guild_id, user_id, xp) VALUES (?,?,?)`).run(G, U, 1234);

    const data = exportUserData(db, G, U);

    expect(data.userId).toBe(U);
    expect(data.cases).toHaveLength(1);
    expect(data.cases[0].reason).toBe('meu caso');
    expect(data.birthdays).toHaveLength(1);
    expect(data.levels?.xp).toBe(1234);
  });

  it('NÃO expõe notas de staff nem o id do moderador (dados de terceiros, art. 15.º/4)', () => {
    const now = 1_800_000_000_000;
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'ban', U, OTHER, 'spam', now);
    db.prepare(
      `INSERT INTO notes (guild_id, target_id, author_id, content, created_at) VALUES (?,?,?,?,?)`,
    ).run(G, U, OTHER, 'nota interna sobre o membro', now);

    const data = exportUserData(db, G, U) as unknown as Record<string, unknown>;

    // Notas de staff são invisíveis ao membro — não podem constar da exportação.
    expect(data.notes).toBeUndefined();
    // O id de quem moderou é dado de terceiro — redigido.
    expect(data.cases as Record<string, unknown>[]).toHaveLength(1);
    expect((data.cases as Record<string, unknown>[])[0].moderator_id).toBeUndefined();
    expect((data.cases as Record<string, unknown>[])[0].reason).toBe('spam');
  });
});

describe('deleteUserData — direito ao apagamento (art. 17.º)', () => {
  it('apaga dados voluntários e PRESERVA os de moderação, reportando ambos', () => {
    const now = 1_800_000_000_000;
    // Voluntário
    db.prepare(`INSERT INTO birthdays (guild_id, user_id, day, month) VALUES (?,?,?,?)`).run(G, U, 3, 4);
    db.prepare(`INSERT INTO afk (guild_id, user_id, reason, since) VALUES (?,?,?,?)`).run(G, U, 'brb', now);
    db.prepare(`INSERT INTO levels (guild_id, user_id, xp) VALUES (?,?,?)`).run(G, U, 50);
    // Moderação (deve manter-se)
    db.prepare(
      `INSERT INTO cases (guild_id, type, target_id, moderator_id, reason, created_at) VALUES (?,?,?,?,?,?)`,
    ).run(G, 'ban', U, OTHER, 'spam', now);

    const res = deleteUserData(db, G, U, now);

    expect(res.deleted.birthdays).toBe(1);
    expect(res.deleted.afk).toBe(1);
    expect(res.deleted.levels).toBe(1);
    expect(res.kept.cases).toBe(1);
    // Verifica no disco: voluntário fora, moderação dentro.
    expect(db.prepare(`SELECT COUNT(*) AS n FROM birthdays`).get()).toEqual({ n: 0 });
    expect(db.prepare(`SELECT COUNT(*) AS n FROM cases`).get()).toEqual({ n: 1 });
  });

  it('não toca em dados de outro utilizador', () => {
    const now = 1_800_000_000_000;
    db.prepare(`INSERT INTO birthdays (guild_id, user_id, day, month) VALUES (?,?,?,?)`).run(G, OTHER, 1, 1);
    deleteUserData(db, G, U, now);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM birthdays`).get()).toEqual({ n: 1 });
  });
});

describe('summarizeDeletion — resposta ao utilizador', () => {
  it('lista o que foi apagado e explica os registos de moderação mantidos', () => {
    const msg = summarizeDeletion({
      deleted: { birthdays: 1, levels: 1 },
      kept: { cases: 2, notes: 0, infractions: 0, quarantine: 0 },
    });
    expect(msg).toMatch(/apagad/i);
    expect(msg).toContain('birthdays');
    // Menciona que ficaram 2 casos de moderação e porquê.
    expect(msg).toMatch(/moderaç/i);
    expect(msg).toContain('2');
  });

  it('quando não havia nada voluntário, di-lo claramente', () => {
    const msg = summarizeDeletion({
      deleted: {},
      kept: { cases: 0, notes: 0, infractions: 0, quarantine: 0 },
    });
    expect(msg).toMatch(/nada|nenhum/i);
  });
});

describe('deleteLevelsForUser — minimização ao sair do servidor', () => {
  it('apaga o XP de quem saiu', () => {
    db.prepare(`INSERT INTO levels (guild_id, user_id, xp) VALUES (?,?,?)`).run(G, U, 999);
    deleteLevelsForUser(db, G, U);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM levels`).get()).toEqual({ n: 0 });
  });
});
