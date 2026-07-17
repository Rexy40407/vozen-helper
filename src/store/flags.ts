import type Database from 'better-sqlite3';
import type { ModConfig } from '../config.js';
import { getAllSettings } from './settings.js';

// Resolver de "feature flags": on/off de cada subsistema. O valor efetivo é o
// override na tabela `settings` (chave `flag.<key>`), ou, se não houver, o default
// vindo de `modConfig` (config-as-code). O painel só escreve estas chaves (allowlist).

export interface FlagDef {
  key: string;
  label: string;
  /** Default vindo do modConfig quando não há override. */
  def: (c: ModConfig) => boolean;
}

/** Registo dos toggles expostos ao painel (allowlist). */
export const FLAG_DEFS: readonly FlagDef[] = [
  { key: 'antispam', label: 'Anti-spam', def: (c) => c.spam.enabled },
  { key: 'antiscam', label: 'Anti-scam', def: (c) => c.antiScam.enabled },
  { key: 'antinuke', label: 'Anti-nuke', def: (c) => c.antiNuke.enabled },
  { key: 'joingate', label: 'Join gate', def: (c) => c.joinGate.enabled },
  { key: 'raid', label: 'Anti-raid', def: (c) => c.raid.enabled },
  { key: 'leveling', label: 'Níveis / XP', def: (c) => c.community.leveling.enabled },
  { key: 'starboard', label: 'Starboard', def: (c) => c.community.starboard.enabled },
  { key: 'suggestions', label: 'Sugestões', def: (c) => c.community.suggestions.enabled },
  { key: 'tickets', label: 'Tickets', def: (c) => c.community.tickets.enabled },
  { key: 'bumpreminder', label: 'Bump reminder', def: (c) => c.community.bumpReminder.enabled },
  { key: 'welcomedm', label: 'DM de boas-vindas', def: (c) => c.community.welcomeDm.enabled },
];

/** Conjunto de chaves válidas (para a allowlist na escrita). */
export const FLAG_KEYS: ReadonlySet<string> = new Set(FLAG_DEFS.map((f) => f.key));

const SETTING_PREFIX = 'flag.';

// ── Cache do hot-path ────────────────────────────────────────────────────────
// `isFeatureEnabled` corre por-mensagem (anti-spam, leveling). Como a API escreve
// noutro processo, o bot relê as settings a cada TTL (curto) para apanhar mudanças.
const CACHE_TTL_MS = 4000;
const cache = new Map<string, { at: number; map: Record<string, string> }>();

function loadCached(db: Database.Database, guildId: string, now: number): Record<string, string> {
  const hit = cache.get(guildId);
  if (hit && now - hit.at < CACHE_TTL_MS) return hit.map;
  const map = getAllSettings(db, guildId);
  cache.set(guildId, { at: now, map });
  return map;
}

/** Limpa o cache (usar nos testes e após escrita no mesmo processo). */
export function clearFlagCache(): void {
  cache.clear();
}

function resolve(raw: string | undefined, fallback: boolean): boolean {
  if (raw === 'true') return true;
  if (raw === 'false') return false;
  return fallback;
}

/** Estado efetivo de um subsistema (hot-path, com cache). `fallback` = default do modConfig. */
export function isFeatureEnabled(
  db: Database.Database,
  guildId: string,
  key: string,
  fallback: boolean,
  now: number = Date.now(),
): boolean {
  const map = loadCached(db, guildId, now);
  return resolve(map[SETTING_PREFIX + key], fallback);
}

export interface EffectiveFlag {
  key: string;
  label: string;
  enabled: boolean;
  default: boolean;
}

/** Estado efetivo de TODAS as flags (para o painel). Leitura direta (sem cache). */
export function getEffectiveFlags(
  db: Database.Database,
  guildId: string,
  modConfig: ModConfig,
): EffectiveFlag[] {
  const map = getAllSettings(db, guildId);
  return FLAG_DEFS.map((f) => {
    const def = f.def(modConfig);
    return {
      key: f.key,
      label: f.label,
      enabled: resolve(map[SETTING_PREFIX + f.key], def),
      default: def,
    };
  });
}

/** Chave completa na tabela settings para um flag (ex.: `flag.antispam`). */
export function flagSettingKey(key: string): string {
  return SETTING_PREFIX + key;
}
