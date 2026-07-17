import { type Client, type GuildMember } from 'discord.js';
import type { AppContext } from '../context.js';
import type { RaidDetector } from '../moderation/raidDetector.js';
import { renderCounter } from './text.js';
import { isFeatureEnabled } from '../store/flags.js';
import { getTextSetting } from '../store/textSettings.js';
import { shouldSendWelcomeDm, buildWelcomeDm } from './welcomeDm.js';
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

/**
 * DM de boas-vindas + mini-tour ao membro novo (estilo Welcomer). Best-effort:
 * nunca deve partir o fluxo de entrada. Não envia a bots nem durante um raid, e
 * ignora em silêncio quem tem as DMs fechadas (erro 50007), sem retry.
 */
export async function handleWelcomeDm(
  ctx: AppContext,
  member: GuildMember,
  raid: RaidDetector,
  now = Date.now(),
): Promise<void> {
  if (member.guild.id !== ctx.env.guildId) return;

  const enabled = isFeatureEnabled(
    ctx.db,
    ctx.env.guildId,
    'welcomedm',
    ctx.modConfig.community.welcomeDm.enabled,
    now,
  );
  if (!shouldSendWelcomeDm({ enabled, isBot: member.user.bot, isRaiding: raid.isRaiding(now) })) return;

  const template = getTextSetting(
    ctx.db,
    ctx.env.guildId,
    'welcomedm.message',
    ctx.modConfig.community.welcomeDm.message,
    now,
  );
  const content = buildWelcomeDm(template, {
    userMention: `<@${member.id}>`,
    serverName: member.guild.name,
    memberCount: member.guild.memberCount,
  });

  try {
    await member.send({ content });
  } catch (err) {
    // 50007 = "Cannot send messages to this user" (DMs fechadas). Silencioso.
    const code = (err as { code?: number }).code;
    if (code !== 50007) log.debug('DM de boas-vindas falhou:', (err as Error).message);
  }
}

/** Tick periódico do counter (a cada 10 min) — apanha mudanças mesmo sem eventos. */
export function startCounterJob(client: Client, ctx: AppContext): NodeJS.Timeout {
  const timer = setInterval(() => void updateMemberCounter(ctx), COUNTER_MIN_INTERVAL);
  timer.unref?.();
  return timer;
}
