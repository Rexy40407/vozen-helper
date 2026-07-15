import { type Client } from 'discord.js';
import type { AppContext } from '../context.js';
import { renderCounter } from './text.js';
import { log } from '../log.js';

// Contador de membros: canal de voz cujo nome mostra a contagem. Respeita o rate
// limit de rename (2/10min) com debounce: só chama a API se o valor mudou e passou
// tempo suficiente.

let lastCounterValue = -1;
let lastCounterAt = 0;
const COUNTER_MIN_INTERVAL = 10 * 60 * 1000; // 10 min (rate limit de rename)

/** Atualiza o canal-contador se o número mudou e já passou o intervalo mínimo. */
export async function updateMemberCounter(ctx: AppContext, force = false): Promise<void> {
  const cfg = ctx.modConfig.community.memberCounter;
  if (!cfg.channelId) return;
  const guild = ctx.client.guilds.cache.get(ctx.env.guildId);
  if (!guild) return;
  const count = guild.memberCount;
  const now = Date.now();
  if (!force && (count === lastCounterValue || now - lastCounterAt < COUNTER_MIN_INTERVAL)) return;

  const channel = await guild.channels.fetch(cfg.channelId).catch(() => null);
  if (!channel || !('setName' in channel)) return;
  const name = renderCounter(cfg.template, count);
  if (channel.name === name) {
    lastCounterValue = count;
    return;
  }
  try {
    await channel.setName(name);
    lastCounterValue = count;
    lastCounterAt = now;
  } catch (err) {
    log.debug('counter rename falhou (rate limit?):', (err as Error).message);
  }
}

/** Tick periódico do counter (a cada 10 min) — apanha mudanças mesmo sem eventos. */
export function startCounterJob(client: Client, ctx: AppContext): NodeJS.Timeout {
  const timer = setInterval(() => void updateMemberCounter(ctx), COUNTER_MIN_INTERVAL);
  timer.unref?.();
  return timer;
}
