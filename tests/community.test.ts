import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import {
  xpForLevelUp,
  totalXpForLevel,
  levelFromXp,
  levelProgress,
  rollXp,
} from '../src/community/leveling.js';
import { renderWelcome, renderCounter, isValidBirthday } from '../src/community/text.js';
import { rolesForLevel } from '../src/community/levels.js';
import { pickWinners } from '../src/community/giveaways.js';
import { dateKey } from '../src/community/stats.js';
import {
  createSuggestion,
  voteSuggestion,
  countSuggestionVotes,
  addXp,
  getRank,
  getLeaderboard,
  addGiveawayEntry,
  getGiveawayEntries,
  setTag,
  getTag,
  getBirthdaysOn,
  setBirthday,
} from '../src/community/store.js';

describe('leveling (fórmula MEE6)', () => {
  it('xpForLevelUp', () => {
    expect(xpForLevelUp(0)).toBe(100);
    expect(xpForLevelUp(1)).toBe(155);
    expect(xpForLevelUp(10)).toBe(1100);
  });
  it('levelFromXp é inverso do totalXpForLevel', () => {
    expect(levelFromXp(0)).toBe(0);
    expect(levelFromXp(totalXpForLevel(5))).toBe(5);
    expect(levelFromXp(totalXpForLevel(5) - 1)).toBe(4);
  });
  it('levelProgress soma bem', () => {
    const p = levelProgress(totalXpForLevel(3) + 40);
    expect(p.level).toBe(3);
    expect(p.current).toBe(40);
    expect(p.needed).toBe(xpForLevelUp(3));
  });
  it('rollXp respeita os limites', () => {
    expect(rollXp(15, 25, 0)).toBe(15);
    expect(rollXp(15, 25, 0.999)).toBe(25);
  });
});

describe('rolesForLevel', () => {
  const ladder = [
    { level: 5, roleId: 'r5' },
    { level: 10, roleId: 'r10' },
    { level: 20, roleId: 'r20' },
  ];
  it('stack acumula', () => {
    expect(rolesForLevel(12, ladder, true).add).toEqual(['r5', 'r10']);
  });
  it('replace dá só o mais alto e remove os menores', () => {
    const r = rolesForLevel(12, ladder, false);
    expect(r.add).toEqual(['r10']);
    expect(r.remove).toEqual(['r5']);
  });
  it('nada abaixo do primeiro marco', () => {
    expect(rolesForLevel(3, ladder, false).add).toEqual([]);
  });
});

describe('text helpers', () => {
  it('renderWelcome substitui variáveis', () => {
    expect(renderWelcome('Olá {user} em {server} (#{membercount})', { userMention: '<@1>', serverName: 'S', memberCount: 42 })).toBe('Olá <@1> em S (#42)');
  });
  it('renderCounter', () => {
    expect(renderCounter('Membros: {count}', 100)).toBe('Membros: 100');
  });
  it('isValidBirthday', () => {
    expect(isValidBirthday(29, 2)).toBe(true);
    expect(isValidBirthday(31, 4)).toBe(false);
    expect(isValidBirthday(0, 1)).toBe(false);
    expect(isValidBirthday(15, 13)).toBe(false);
  });
});

describe('pickWinners', () => {
  it('escolhe sem repetir e respeita o número', () => {
    const w = pickWinners(['a', 'b', 'c', 'd'], 2, () => 0);
    expect(w).toHaveLength(2);
    expect(new Set(w).size).toBe(2);
  });
  it('não passa o nº de participantes', () => {
    expect(pickWinners(['a'], 3, () => 0)).toEqual(['a']);
    expect(pickWinners([], 3, () => 0)).toEqual([]);
  });
});

describe('dateKey', () => {
  it('formata YYYY-MM-DD', () => {
    expect(dateKey(new Date(2026, 0, 5))).toBe('2026-01-05');
  });
});

describe('store de comunidade', () => {
  let db: Database.Database;
  const G = '111111111111111111';
  beforeEach(() => {
    db = initDb(':memory:');
  });

  it('sugestões: votos contam e trocam', () => {
    const id = createSuggestion(db, G, 'author', 'ideia', 1);
    voteSuggestion(db, id, 'u1', 1);
    voteSuggestion(db, id, 'u2', 1);
    voteSuggestion(db, id, 'u3', -1);
    expect(countSuggestionVotes(db, id)).toEqual({ up: 2, down: 1 });
    voteSuggestion(db, id, 'u1', -1); // troca
    expect(countSuggestionVotes(db, id)).toEqual({ up: 1, down: 2 });
  });

  it('XP: rank e leaderboard ordenam', () => {
    addXp(db, G, 'a', 100);
    addXp(db, G, 'b', 300);
    addXp(db, G, 'c', 200);
    expect(getRank(db, G, 'b')).toBe(1);
    expect(getRank(db, G, 'a')).toBe(3);
    expect(getLeaderboard(db, G, 2).map((r) => r.userId)).toEqual(['b', 'c']);
  });

  it('giveaway entries: sem duplicados', () => {
    expect(addGiveawayEntry(db, 1, 'u1')).toBe(true);
    expect(addGiveawayEntry(db, 1, 'u1')).toBe(false);
    addGiveawayEntry(db, 1, 'u2');
    expect(getGiveawayEntries(db, 1).sort()).toEqual(['u1', 'u2']);
  });

  it('tags: upsert e leitura case-insensitive', () => {
    setTag(db, G, 'Regras', 'sê fixe', 'author', 1);
    expect(getTag(db, G, 'regras')).toBe('sê fixe');
    setTag(db, G, 'regras', 'nova', 'author', 2);
    expect(getTag(db, G, 'REGRAS')).toBe('nova');
  });

  it('aniversários: consulta por dia/mês', () => {
    setBirthday(db, G, 'u1', 5, 3);
    setBirthday(db, G, 'u2', 5, 3);
    setBirthday(db, G, 'u3', 6, 3);
    expect(getBirthdaysOn(db, G, 5, 3).sort()).toEqual(['u1', 'u2']);
  });
});
