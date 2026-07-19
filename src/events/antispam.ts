import { type Message } from 'discord.js';
import type { AppContext } from '../context.js';
import type { SpamTracker } from '../moderation/spamTracker.js';
import { isExemptMember, isExemptChannel } from '../moderation/exempt.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';
import { applyServerRuleViolation } from '../moderation/policyEnforcement.js';

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

  const member =
    message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
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

  await message.delete().catch(() => undefined);
  const result = await applyServerRuleViolation(
    ctx,
    message.guild,
    message.author.id,
    ctx.client.user?.id ?? 'bot',
    'spam',
    verdict.signals.join(', ') || 'heat',
    now,
  );
  if (!result.ok) log.error(`Falha ao silenciar por spam: ${result.summary}`);
  else log.info(`Anti-spam silenciou ${message.author.tag}: ${result.summary}`);
}
