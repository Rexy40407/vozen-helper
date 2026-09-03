import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import { logActivity, getRecentActivity, countActivity } from '../src/store/activity.js';
import { attributeInvite, type CachedInvite } from '../src/community/inviteTracker.js';

// Feature: dashboard de atividade do painel (audit-log mais rico). Lógica pura de
// atribuição de convite + store do registo de atividade.

const G = '111111111111111111';
let db: Database.Database;
beforeEach(() => {
  db = initDb(':memory:');
});

describe('attributeInvite — descobre por que convite o membro entrou', () => {
  it('convite existente cujo nº de usos aumentou', () => {
    const before = new Map<string, CachedInvite>([
      ['aaa', { uses: 2, inviterId: 'inv1' }],
      ['bbb', { uses: 5, inviterId: 'inv2' }],
    ]);
    const after = [
      { code: 'aaa', uses: 2, inviterId: 'inv1' },
      { code: 'bbb', uses: 6, inviterId: 'inv2' }, // +1
    ];
    expect(attributeInvite(before, after)).toEqual({ code: 'bbb', inviterId: 'inv2' });
  });

  it('convite NOVO criado e usado entre fetches', () => {
    const before = new Map<string, CachedInvite>([['aaa', { uses: 2, inviterId: 'inv1' }]]);
    const after = [
      { code: 'aaa', uses: 2, inviterId: 'inv1' },
      { code: 'ccc', uses: 1, inviterId: 'inv3' }, // novo, já com 1 uso
    ];
    expect(attributeInvite(before, after)).toEqual({ code: 'ccc', inviterId: 'inv3' });
  });

  it('convite de uso único CONSUMIDO (desaparece do after) — recupera o inviter do cache', () => {
    const before = new Map<string, CachedInvite>([
      ['aaa', { uses: 2, inviterId: 'inv1' }],
      ['one', { uses: 0, inviterId: 'inv9' }],
    ]);
    const after = [{ code: 'aaa', uses: 2, inviterId: 'inv1' }]; // 'one' foi consumido e apagado
    expect(attributeInvite(before, after)).toEqual({ code: 'one', inviterId: 'inv9' });
  });

  it('indeterminado (vanity / dois joins simultâneos) → null', () => {
    const before = new Map<string, CachedInvite>([['aaa', { uses: 2, inviterId: 'inv1' }]]);
    const after = [{ code: 'aaa', uses: 2, inviterId: 'inv1' }]; // nada mudou
    expect(attributeInvite(before, after)).toBeNull();
  });
});

describe('activity store — logActivity / getRecentActivity', () => {
  it('regista e devolve do mais recente ao mais antigo, com detail em JSON', () => {
    logActivity(db, {
      guildId: G,
      type: 'join',
      userId: 'u1',
      userTag: 'Zé#1',
      detail: { inviteCode: 'abc', inviterId: 'inv1' },
      createdAt: 100,
    });
    logActivity(db, { guildId: G, type: 'leave', userId: 'u2', userTag: 'Ana#2', createdAt: 200 });

    const rows = getRecentActivity(db, G, 50);
    expect(rows).toHaveLength(2);
    expect(rows[0].type).toBe('leave'); // mais recente primeiro
    expect(rows[1].type).toBe('join');
    expect(rows[1].detail).toEqual({ inviteCode: 'abc', inviterId: 'inv1' }); // parseado
    expect(countActivity(db, G)).toBe(2);
  });

  it('filtra por tipo e respeita o limite', () => {
    for (let i = 0; i < 5; i++)
      logActivity(db, { guildId: G, type: 'join', userId: 'u' + i, createdAt: i });
    logActivity(db, { guildId: G, type: 'leave', userId: 'x', createdAt: 99 });
    expect(getRecentActivity(db, G, 50, 'join')).toHaveLength(5);
    expect(getRecentActivity(db, G, 2)).toHaveLength(2);
  });

  it('não mistura guilds', () => {
    logActivity(db, { guildId: G, type: 'join', userId: 'u1', createdAt: 1 });
    logActivity(db, { guildId: '999', type: 'join', userId: 'u2', createdAt: 2 });
    expect(getRecentActivity(db, G, 50)).toHaveLength(1);
  });
});
