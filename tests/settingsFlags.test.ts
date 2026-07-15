import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import { getSetting, setSetting, getAllSettings, deleteSetting } from '../src/store/settings.js';
import {
  isFeatureEnabled,
  getEffectiveFlags,
  clearFlagCache,
  FLAG_DEFS,
  flagSettingKey,
} from '../src/store/flags.js';
import {
  resolveChannelId,
  resolveLevelupChannel,
  resolveXpExcluded,
  getChannelSettingsView,
  clearChannelCache,
} from '../src/store/channelSettings.js';
import { modConfig } from '../src/config.js';

// Fase 3 — settings de runtime + resolver de flags (overrides ao modConfig).

const G = '111111111111111111';
let db: Database.Database;
beforeEach(() => {
  db = initDb(':memory:');
  clearFlagCache();
  clearChannelCache();
});

describe('settings store', () => {
  it('set / get / getAll / delete (com upsert)', () => {
    expect(getSetting(db, G, 'k')).toBeNull();
    setSetting(db, G, 'k', 'v', 1);
    expect(getSetting(db, G, 'k')).toBe('v');
    setSetting(db, G, 'k', 'v2', 2);
    expect(getSetting(db, G, 'k')).toBe('v2');
    expect(getAllSettings(db, G)).toEqual({ k: 'v2' });
    expect(deleteSetting(db, G, 'k')).toBe(true);
    expect(deleteSetting(db, G, 'k')).toBe(false);
    expect(getSetting(db, G, 'k')).toBeNull();
  });
});

describe('resolver de flags', () => {
  it('sem override devolve o default do modConfig', () => {
    const def = FLAG_DEFS[0].def(modConfig);
    expect(isFeatureEnabled(db, G, FLAG_DEFS[0].key, def, 1000)).toBe(def);
  });

  it('override false/true vence o default', () => {
    setSetting(db, G, flagSettingKey('antispam'), 'false', 1000);
    clearFlagCache();
    expect(isFeatureEnabled(db, G, 'antispam', true, 1000)).toBe(false);
    setSetting(db, G, flagSettingKey('antispam'), 'true', 1000);
    clearFlagCache();
    expect(isFeatureEnabled(db, G, 'antispam', false, 1000)).toBe(true);
  });

  it('cache: mudança só é vista após o TTL', () => {
    isFeatureEnabled(db, G, 'antispam', true, 1000); // popula cache (sem override)
    setSetting(db, G, flagSettingKey('antispam'), 'false', 1000);
    expect(isFeatureEnabled(db, G, 'antispam', true, 1000)).toBe(true); // ainda cacheado
    expect(isFeatureEnabled(db, G, 'antispam', true, 1000 + 5000)).toBe(false); // relê
  });

  it('getEffectiveFlags devolve todas com default e efetivo', () => {
    setSetting(db, G, flagSettingKey('tickets'), 'false', 1);
    const flags = getEffectiveFlags(db, G, modConfig);
    expect(flags).toHaveLength(FLAG_DEFS.length);
    const t = flags.find((f) => f.key === 'tickets');
    expect(t?.enabled).toBe(false);
    expect(typeof t?.default).toBe('boolean');
  });
});

describe('resolver de canais', () => {
  it('resolveChannelId: override ou fallback', () => {
    expect(resolveChannelId(db, G, 'log', null, 1000)).toBeNull();
    setSetting(db, G, 'chan.log', '123', 1000);
    clearChannelCache();
    expect(resolveChannelId(db, G, 'log', null, 1000)).toBe('123');
  });

  it('resolveLevelupChannel: current → null, id → id, senão fallback', () => {
    expect(resolveLevelupChannel(db, G, '999', 1000)).toBe('999');
    setSetting(db, G, 'chan.levelup', 'current', 1000);
    clearChannelCache();
    expect(resolveLevelupChannel(db, G, '999', 1000)).toBeNull();
    setSetting(db, G, 'chan.levelup', '555', 1000);
    clearChannelCache();
    expect(resolveLevelupChannel(db, G, '999', 1000)).toBe('555');
  });

  it('resolveXpExcluded: CSV override ou fallback', () => {
    expect(resolveXpExcluded(db, G, ['a', 'b'], 1000)).toEqual(['a', 'b']);
    setSetting(db, G, 'xp.exclude', 'x,y,z', 1000);
    clearChannelCache();
    expect(resolveXpExcluded(db, G, ['a'], 1000)).toEqual(['x', 'y', 'z']);
    setSetting(db, G, 'xp.exclude', '', 1000); // vazio = não excluir nada
    clearChannelCache();
    expect(resolveXpExcluded(db, G, ['a'], 1000)).toEqual([]);
  });

  it('getChannelSettingsView devolve as 5 definições', () => {
    const view = getChannelSettingsView(db, G, modConfig);
    expect(view.map((v) => v.key)).toEqual([
      'chan.log',
      'chan.suggestions',
      'chan.starboard',
      'chan.levelup',
      'xp.exclude',
    ]);
  });
});
