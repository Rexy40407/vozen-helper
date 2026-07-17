import type Database from 'better-sqlite3';
import type { ModConfig } from '../config.js';
import { getAllSettings } from './settings.js';

// Resolver de "settings de texto": mensagens editáveis no painel (ex.: a DM de
// boas-vindas). O valor efetivo é o override na tabela `settings` (chave
// `text.<key>`), ou, se não houver, o default vindo de `modConfig` (config-as-code).
// O painel só escreve estas chaves (allowlist). Mesmo padrão dos flags.

export interface TextDef {
  key: string;
  label: string;
  /** Default vindo do modConfig quando não há override. */
  def: (c: ModConfig) => string;
  /** Limite de tamanho (validação na escrita da API). */
  maxLen: number;
}

/** Registo dos textos editáveis expostos ao painel (allowlist). */
export const TEXT_DEFS: readonly TextDef[] = [
  {
    key: 'welcomedm.message',
    label: 'Mensagem de boas-vindas (DM)',
    def: (c) => c.community.welcomeDm.message,
    maxLen: 1500,
  },
];

/** Conjunto de chaves válidas (para a allowlist na escrita). */
export const TEXT_SETTING_KEYS: ReadonlySet<string> = new Set(TEXT_DEFS.map((t) => t.key));

const SETTING_PREFIX = 'text.';

// ── Cache curto ──────────────────────────────────────────────────────────────
// A API escreve noutro processo; o bot relê a cada TTL (curto) para apanhar edições.
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
export function clearTextCache(): void {
  cache.clear();
}

/** Valor efetivo de um texto (override na BD ou `fallback` = default do modConfig). */
export function getTextSetting(
  db: Database.Database,
  guildId: string,
  key: string,
  fallback: string,
  now: number = Date.now(),
): string {
  const map = loadCached(db, guildId, now);
  const raw = map[SETTING_PREFIX + key];
  return raw !== undefined ? raw : fallback;
}

export interface EffectiveText {
  key: string;
  label: string;
  value: string;
  default: string;
  maxLen: number;
}

/** Estado efetivo de TODOS os textos (para o painel). Leitura direta (sem cache). */
export function getEffectiveTexts(
  db: Database.Database,
  guildId: string,
  modConfig: ModConfig,
): EffectiveText[] {
  const map = getAllSettings(db, guildId);
  return TEXT_DEFS.map((t) => {
    const def = t.def(modConfig);
    const raw = map[SETTING_PREFIX + t.key];
    return {
      key: t.key,
      label: t.label,
      value: raw !== undefined ? raw : def,
      default: def,
      maxLen: t.maxLen,
    };
  });
}

/** Chave completa na tabela settings para um texto (ex.: `text.welcomedm.message`). */
export function textSettingKey(key: string): string {
  return SETTING_PREFIX + key;
}
