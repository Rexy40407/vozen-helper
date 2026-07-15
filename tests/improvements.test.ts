import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import { scheduleAction, getDueActions, cancelScheduledByPayload } from '../src/store/cases.js';
import { buildReminderSend } from '../src/community/scheduler.js';
import { pruneCooldowns } from '../src/community/levels.js';
import { pickOfferableRoles } from '../src/community/selfroles.js';
import { ExpiryScheduler } from '../src/moderation/scheduler.js';

// Testes dos planos 001, 003, 004, 005, 006.

describe('001 — buildReminderSend restringe menções', () => {
  it('só permite mencionar o destinatário', () => {
    const out = buildReminderSend('123', 'olá @everyone <@&999>');
    expect(out.allowedMentions).toEqual({ users: ['123'] });
    expect('parse' in out.allowedMentions).toBe(false); // @everyone/cargos não parseados
    expect(out.content).toContain('<@123>');
    expect(out.content).toContain('@everyone'); // aparece como texto, mas não notifica
  });
});

describe('003 — cancelScheduledByPayload só apaga o payload certo', () => {
  let db: Database.Database;
  beforeEach(() => {
    db = initDb(':memory:');
  });
  it('cancela o giveaway 1 e preserva o 2 (mesmo host)', () => {
    scheduleAction(db, { guildId: 'g', type: 'giveaway_end', targetId: 'host', executeAt: 100, payload: '1', caseId: null });
    scheduleAction(db, { guildId: 'g', type: 'giveaway_end', targetId: 'host', executeAt: 200, payload: '2', caseId: null });
    cancelScheduledByPayload(db, 'g', 'giveaway_end', '1');
    const left = getDueActions(db, 10_000);
    expect(left).toHaveLength(1);
    expect(left[0].payload).toBe('2');
  });
});

describe('004 — ExpiryScheduler tem guard de reentrância', () => {
  it('passagens sobrepostas não reprocessam a mesma ação', async () => {
    const db = initDb(':memory:');
    scheduleAction(db, { guildId: 'g', type: 'reminder', targetId: 'u', executeAt: 0, payload: '{}', caseId: null });
    let calls = 0;
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const runner = async () => {
      calls++;
      await gate; // segura a 1ª passagem
    };
    const sched = new ExpiryScheduler(db, {} as never, runner, 999_999);
    const p1 = sched.runDue(1000); // não await — fica pendurada no gate
    await sched.runDue(1000); // 2ª passagem concorrente → sai pelo guard
    release();
    await p1;
    expect(calls).toBe(1);
  });
});

describe('005 — pruneCooldowns', () => {
  it('não poda abaixo do teto', () => {
    const m = new Map([['a', 0]]);
    pruneCooldowns(m, 1_000_000, 60_000);
    expect(m.size).toBe(1);
  });
  it('acima do teto remove as expiradas e mantém as recentes', () => {
    const m = new Map<string, number>();
    for (let i = 0; i < 10_001; i++) m.set('old' + i, 0); // todas expiradas
    m.set('recent', 1_000_000);
    pruneCooldowns(m, 1_000_000, 60_000);
    expect(m.has('recent')).toBe(true);
    expect(m.has('old0')).toBe(false);
  });
});

describe('006 — pickOfferableRoles respeita a hierarquia', () => {
  const roles = [
    { position: 6, managed: false, id: 'acima-do-mod' },
    { position: 3, managed: false, id: 'ok' },
    { position: 2, managed: true, id: 'bot-role' },
  ];
  it('exclui cargos acima do moderador', () => {
    const out = pickOfferableRoles(roles, 8, 5, false).map((r) => r.id);
    expect(out).toEqual(['ok']);
  });
  it('o dono usa o teto do bot', () => {
    const out = pickOfferableRoles(roles, 8, 5, true).map((r) => r.id);
    expect(out).toEqual(['acima-do-mod', 'ok']);
  });
  it('cargos managed são sempre excluídos', () => {
    expect(pickOfferableRoles(roles, 8, 5, true).some((r) => r.managed)).toBe(false);
  });
});
