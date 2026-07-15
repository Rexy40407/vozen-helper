import type Database from 'better-sqlite3';
import type { ModConfig } from '../config.js';
import { getAllSettings } from './settings.js';

// Resolver de canais configuráveis pelo painel. Overrides na tabela `settings`
// (chaves `chan.*` e `xp.exclude`) sobrepõem-se aos defaults de `modConfig`. Lido em
// hot-paths (logging, XP) → cache com TTL curto para apanhar mudanças da API (outro
// processo).

/** Chaves de canal que o painel pode escrever (allowlist). */
export const CHANNEL_SETTING_KEYS: ReadonlySet<string> = new Set([
  'chan.log',
  'chan.suggestions',
  'chan.starboard',
  'chan.levelup',
  'xp.exclude',
]);

const TTL_MS = 4000;
const cache = new Map<string, { at: number; map: Record<string, string> }>();

function load(db: Database.Database, guildId: string, now: number): Record<string, string> {
  const hit = cache.get(guildId);
  if (hit && now - hit.at < TTL_MS) return hit.map;
  const map = getAllSettings(db, guildId);
  cache.set(guildId, { at: now, map });
  return map;
}

/** Limpa o cache (testes + após escrita no mesmo processo). */
export function clearChannelCache(): void {
  cache.clear();
}

/** ID de canal override para `key` (log/suggestions/starboard), ou o fallback. */
export function resolveChannelId(
  db: Database.Database,
  guildId: string,
  key: string,
  fallback: string | null,
  now: number = Date.now(),
): string | null {
  const raw = load(db, guildId, now)['chan.' + key];
  return raw && raw.length ? raw : fallback;
}

/**
 * Canal de anúncio de nível: um ID específico, ou `null` = "no canal onde a pessoa
 * falou" (valor especial 'current'). Sem override, usa o fallback do modConfig.
 */
export function resolveLevelupChannel(
  db: Database.Database,
  guildId: string,
  fallback: string | null | undefined,
  now: number = Date.now(),
): string | null {
  const raw = load(db, guildId, now)['chan.levelup'];
  if (raw === 'current') return null;
  if (raw && raw.length) return raw;
  return fallback ?? null;
}

/** Lista de canais onde o XP NÃO conta. Override (`xp.exclude`, CSV) ou fallback. */
export function resolveXpExcluded(
  db: Database.Database,
  guildId: string,
  fallback: readonly string[],
  now: number = Date.now(),
): readonly string[] {
  const raw = load(db, guildId, now)['xp.exclude'];
  if (raw == null) return fallback;
  return raw ? raw.split(',').filter(Boolean) : [];
}

// ── Vista para o painel (valores efetivos atuais) ────────────────────────────
export type ChannelSettingKind = 'channel' | 'channel-or-current' | 'channel-list';
export interface ChannelSettingView {
  key: string;
  label: string;
  kind: ChannelSettingKind;
  value: string; // ID, 'current', CSV de IDs, ou '' (default)
}

/** Estado atual das definições de canal (override ou default do modConfig). */
export function getChannelSettingsView(
  db: Database.Database,
  guildId: string,
  modConfig: ModConfig,
): ChannelSettingView[] {
  const map = getAllSettings(db, guildId);
  const eff = (k: string, def: string): string => (map[k] != null ? map[k] : def);
  const lev = modConfig.community.leveling;
  return [
    { key: 'chan.log', label: 'Canal de logs (moderação)', kind: 'channel', value: eff('chan.log', '') },
    {
      key: 'chan.suggestions',
      label: 'Canal de sugestões',
      kind: 'channel',
      value: eff('chan.suggestions', modConfig.community.suggestions.channelId ?? ''),
    },
    {
      key: 'chan.starboard',
      label: 'Canal do starboard',
      kind: 'channel',
      value: eff('chan.starboard', modConfig.community.starboard.channelId ?? ''),
    },
    {
      key: 'chan.levelup',
      label: 'Anúncios de nível',
      kind: 'channel-or-current',
      value: eff('chan.levelup', lev.announceChannelId || 'current'),
    },
    {
      key: 'xp.exclude',
      label: 'Excluir do XP (canais)',
      kind: 'channel-list',
      value: eff('xp.exclude', (lev.noXpChannelIds ?? []).join(',')),
    },
  ];
}
