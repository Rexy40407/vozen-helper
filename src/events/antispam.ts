import { type Message } from 'discord.js';
import type { AppContext } from '../context.js';
import type { SpamTracker } from '../moderation/spamTracker.js';
import { isExemptMember, isExemptChannel } from '../moderation/exempt.js';
import { recordCase, dmPunished } from '../moderation/service.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';

// Handler de anti-spam: alimenta o SpamTracker com cada mensagem e, se o calor passar
// o limite, silencia o autor + apaga a mensagem + regista caso.

export async function handleMessageSpam(
  ctx: AppContext,
  tracker: SpamTracker,
  message: Message,
  now = Date.now(),
): Promise<void> {
  if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'antispam', ctx.modConfig.spam.enabled)) return;
  if (message.author?.bot) return;
  if (!message.inGuild() || message.guildId !== ctx.env.guildId) return;
  if (isExemptChannel(message.channelId, ctx.modConfig.automodExemptChannelIds)) return;

  const member = message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
  const roleIds = member ? [...member.roles.cache.keys()] : [];
  if (isExemptMember(message.author.id, roleIds, ctx.modConfig)) return;

  const verdict = tracker.record(
    {
      userId: message.author.id,
      channelId: message.channelId,
      content: message.content,
      mentionCount: message.mentions.users.size + message.mentions.roles.size,
    },
    now,
  );

  if (!verdict.punish || !member) return;

  const reason = `Spam (${verdict.signals.join(', ') || 'heat'})`;
  await message.delete().catch(() => undefined);
  try {
    await member.timeout(ctx.modConfig.spam.timeoutMs, reason);
  } catch (err) {
    log.error('Falha ao silenciar por spam:', err);
    return;
  }
  recordCase(ctx, {
    guildId: message.guildId,
    type: 'timeout',
    targetId: message.author.id,
    moderatorId: ctx.client.user?.id ?? 'bot',
    reason,
    durationMs: ctx.modConfig.spam.timeoutMs,
    createdAt: now,
  });
  await dmPunished(ctx, message.author, message.guild, 'timeout', reason, ctx.modConfig.spam.timeoutMs);
  log.info(`Anti-spam silenciou ${message.author.tag}: ${reason}`);
}
