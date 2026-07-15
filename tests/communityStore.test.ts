import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import {
  setAfk,
  getAfk,
  clearAfk,
  setSelfRole,
  getSelfRole,
  getSelfRolesForMessage,
  createGiveaway,
  getGiveaway,
  markGiveawayEnded,
  listActiveGiveaways,
  createTicket,
  getOpenTicketForUser,
  getTicketByChannel,
  setTicketStatus,
  claimTicket,
  getStarEntry,
  upsertStarEntry,
  deleteStarEntry,
  incrStat,
  getStatsTotals,
} from '../src/community/store.js';

// Plano 008 — cobrir as funções de community/store.ts sem teste.

const G = '111111111111111111';

let db: Database.Database;
beforeEach(() => {
  db = initDb(':memory:');
});

describe('AFK', () => {
  it('set → get → clear', () => {
    setAfk(db, G, 'u', 'almoço', 1000);
    expect(getAfk(db, G, 'u')).toEqual({ reason: 'almoço', since: 1000 });
    expect(clearAfk(db, G, 'u')).toBe(true);
    expect(getAfk(db, G, 'u')).toBeNull();
    expect(clearAfk(db, G, 'u')).toBe(false); // já não existe
  });
});

describe('self-roles', () => {
  it('set/get e upsert', () => {
    setSelfRole(db, 'msg1', 'role:r1', 'r1', 'normal');
    setSelfRole(db, 'msg1', 'role:r2', 'r2', 'unique');
    expect(getSelfRole(db, 'msg1', 'role:r1')).toEqual({ roleId: 'r1', mode: 'normal' });
    expect(getSelfRolesForMessage(db, 'msg1')).toHaveLength(2);
    setSelfRole(db, 'msg1', 'role:r1', 'r1', 'verify'); // upsert do modo
    expect(getSelfRole(db, 'msg1', 'role:r1')?.mode).toBe('verify');
  });
});

describe('giveaways lifecycle', () => {
  it('create → active → markEnded sai da lista ativa', () => {
    const id = createGiveaway(db, {
      guildId: G,
      channelId: 'c',
      prize: 'Nitro',
      winners: 2,
      endAt: 5000,
      requiredRoleId: null,
      hostId: 'host',
      createdAt: 1,
    });
    expect(getGiveaway(db, id)?.ended).toBe(false);
    expect(listActiveGiveaways(db, G).map((g) => g.id)).toContain(id);
    markGiveawayEnded(db, id);
    expect(getGiveaway(db, id)?.ended).toBe(true);
    expect(listActiveGiveaways(db, G)).toHaveLength(0);
  });
});

describe('tickets', () => {
  it('open → claim → close', () => {
    const id = createTicket(db, G, 'chan', 'opener', 1);
    expect(getOpenTicketForUser(db, G, 'opener')?.id).toBe(id);
    expect(getTicketByChannel(db, 'chan')?.openerId).toBe('opener');
    claimTicket(db, id, 'mod');
    expect(getTicketByChannel(db, 'chan')?.claimedBy).toBe('mod');
    setTicketStatus(db, id, 'closed');
    expect(getOpenTicketForUser(db, G, 'opener')).toBeNull(); // já não está aberto
  });
});

describe('starboard', () => {
  it('upsert → atualiza → delete', () => {
    expect(getStarEntry(db, G, 'orig')).toBeNull();
    upsertStarEntry(db, G, 'orig', 'sb1', 3);
    expect(getStarEntry(db, G, 'orig')).toEqual({ starboardMessageId: 'sb1', starCount: 3 });
    upsertStarEntry(db, G, 'orig', 'sb1', 5); // atualiza o contador
    expect(getStarEntry(db, G, 'orig')?.starCount).toBe(5);
    deleteStarEntry(db, G, 'orig');
    expect(getStarEntry(db, G, 'orig')).toBeNull();
  });
});

describe('stats', () => {
  it('incrementa e soma por campo', () => {
    incrStat(db, G, '2026-07-14', 'messages');
    incrStat(db, G, '2026-07-14', 'messages');
    incrStat(db, G, '2026-07-15', 'messages');
    incrStat(db, G, '2026-07-14', 'joins');
    const t = getStatsTotals(db, G);
    expect(t.messages).toBe(3);
    expect(t.joins).toBe(1);
    expect(t.leaves).toBe(0);
  });
});
