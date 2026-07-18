import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { evaluateJoin } from '../src/moderation/joinGate.js';
import { RaidDetector } from '../src/moderation/raidDetector.js';
import { initDb } from '../src/store/db.js';
import { saveStickyRoles, getStickyRoles, clearStickyRoles } from '../src/store/sticky.js';
import type { JoinGateConfig, RaidConfig } from '../src/config.js';

const DAY = 86_400_000;

const gate: JoinGateConfig = {
  enabled: true,
  minAccountAgeMs: 3 * DAY,
  newAccountAction: 'kick',
  requireAvatar: true,
  noAvatarAction: 'kick',
  blockedUsernameSubstrings: ['free nitro'],
  badUsernameAction: 'ban',
};

describe('evaluateJoin', () => {
  it('deixa passar conta normal', () => {
    expect(
      evaluateJoin({ accountAgeMs: 30 * DAY, hasAvatar: true, username: 'diogo' }, gate).action,
    ).toBe('none');
  });
  it('apanha conta demasiado recente', () => {
    const v = evaluateJoin({ accountAgeMs: DAY, hasAvatar: true, username: 'x' }, gate);
    expect(v.action).toBe('kick');
    expect(v.reason).toContain('too new');
  });
  it('apanha sem avatar', () => {
    expect(evaluateJoin({ accountAgeMs: 30 * DAY, hasAvatar: false, username: 'x' }, gate).action).toBe('kick');
  });
  it('bane username com padrão bloqueado', () => {
    const v = evaluateJoin({ accountAgeMs: 30 * DAY, hasAvatar: true, username: 'FREE NITRO here' }, gate);
    expect(v.action).toBe('ban');
  });
});

const raidCfg: RaidConfig = {
  enabled: true,
  joinWindowMs: 10_000,
  joinThreshold: 5,
  raidModeMs: 60_000,
  raiseVerificationLevel: 3,
  pauseInvites: true,
  alertChannelId: null,
};

describe('RaidDetector', () => {
  it('dispara ao atingir o threshold na janela', () => {
    const d = new RaidDetector(raidCfg);
    let triggered = false;
    for (let i = 0; i < 5; i++) {
      const r = d.record(i * 1000);
      triggered ||= r.justTriggered;
    }
    expect(triggered).toBe(true);
  });
  it('não dispara com joins espaçados', () => {
    const d = new RaidDetector(raidCfg);
    let triggered = false;
    for (let i = 0; i < 5; i++) {
      const r = d.record(i * 5000); // 5s de intervalo → janela nunca acumula 5
      triggered ||= r.justTriggered;
    }
    expect(triggered).toBe(false);
  });
  it('só dispara uma vez (justTriggered) na transição', () => {
    const d = new RaidDetector(raidCfg);
    const flags = [0, 1, 2, 3, 4, 5, 6].map((i) => d.record(i * 500).justTriggered);
    expect(flags.filter(Boolean)).toHaveLength(1);
  });
});

describe('sticky roles store', () => {
  let db: Database.Database;
  beforeEach(() => {
    db = initDb(':memory:');
  });
  it('guarda, lê e limpa', () => {
    saveStickyRoles(db, 'g', 'u', ['r1', 'r2'], 1);
    expect(getStickyRoles(db, 'g', 'u')).toEqual(['r1', 'r2']);
    clearStickyRoles(db, 'g', 'u');
    expect(getStickyRoles(db, 'g', 'u')).toEqual([]);
  });
  it('upsert substitui o conjunto', () => {
    saveStickyRoles(db, 'g', 'u', ['r1'], 1);
    saveStickyRoles(db, 'g', 'u', ['r2', 'r3'], 2);
    expect(getStickyRoles(db, 'g', 'u')).toEqual(['r2', 'r3']);
  });
});
