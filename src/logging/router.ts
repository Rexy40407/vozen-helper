import type { LogConfig, LogCategory } from '../config.js';

// Encaminhamento de logs (puro): que canal recebe cada categoria e o que ignorar.

/** Canal de destino para uma categoria, ou null se desligada. */
export function pickLogChannel(cfg: LogConfig, category: LogCategory): string | null {
  return cfg.channels[category] ?? null;
}

/** Deve ignorar-se este evento? (canal de staff ou utilizador na ignore list). */
export function shouldIgnore(
  cfg: LogConfig,
  opts: { channelId?: string | null; userId?: string | null },
): boolean {
  if (opts.channelId && cfg.ignoreChannelIds.includes(opts.channelId)) return true;
  if (opts.userId && cfg.ignoreUserIds.includes(opts.userId)) return true;
  return false;
}
