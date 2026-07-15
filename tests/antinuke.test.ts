import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { NukeTracker } from '../src/moderation/nukeTracker.js';
import { initDb } from '../src/store/db.js';
import {
  saveQuarantine,
  getQuarantine,
  clearQuarantine,
  isQuarantined,
} from '../src/store/quarantine.js';

describe('NukeTracker', () => {
  it('conta ações do mesmo executor+tipo na janela', () => {
    const t = new NukeTracker(10_000);
    expect(t.record('mod', 'channelDelete', 0)).toBe(1);
    expect(t.record('mod', 'channelDelete', 1000)).toBe(2);
    expect(t.record('mod', 'channelDelete', 2000)).toBe(3);
  });

  it('separa por tipo e por executor', () => {
    const t = new NukeTracker(10_000);
    t.record('mod', 'channelDelete', 0);
    expect(t.record('mod', 'roleDelete', 0)).toBe(1);
    expect(t.record('outro', 'channelDelete', 0)).toBe(1);
  });

  it('esquece ações fora da janela', () => {
    const t = new NukeTracker(10_000);
    t.record('mod', 'ban', 0);
    t.record('mod', 'ban', 5000);
    expect(t.record('mod', 'ban', 20_000)).toBe(1); // as duas primeiras saíram da janela
  });

  it('reset limpa o executor', () => {
    const t = new NukeTracker(10_000);
    t.record('mod', 'kick', 0);
    t.reset('mod');
    expect(t.record('mod', 'kick', 100)).toBe(1);
  });
});

describe('quarantine store', () => {
  let db: Database.Database;
  beforeEach(() => {
    db = initDb(':memory:');
  });
  it('guarda, lê e limpa cargos', () => {
    saveQuarantine(db, 'g', 'u', ['r1', 'r2'], 'nuke', 1);
    expect(isQuarantined(db, 'g', 'u')).toBe(true);
    expect(getQuarantine(db, 'g', 'u')?.roleIds).toEqual(['r1', 'r2']);
    expect(getQuarantine(db, 'g', 'u')?.reason).toBe('nuke');
    clearQuarantine(db, 'g', 'u');
    expect(isQuarantined(db, 'g', 'u')).toBe(false);
  });
});
