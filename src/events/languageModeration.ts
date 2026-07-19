import type { Message } from 'discord.js';
import type { AppContext } from '../context.js';
import type { LanguageTracker } from '../moderation/languagePolicy.js';
import { isExemptChannel, isExemptMember } from '../moderation/exempt.js';
import { applyServerRuleViolation } from '../moderation/policyEnforcement.js';
import { log } from '../log.js';

/** Modera abuso de linguagem sem punir palavrões casuais e isolados. */
export async function handleMessageLanguage(
  ctx: AppContext,
  tracker: LanguageTracker,
  message: Message,
  now = Date.now(),
): Promise<void> {
  if (!ctx.modConfig.language.enabled || message.author?.bot) return;
  if (!message.inGuild() || message.guildId !== ctx.env.guildId) return;
  if (isExemptChannel(message.channelId, ctx.modConfig.automodExemptChannelIds)) return;

  const member =
    message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
  const roleIds = member ? [...member.roles.cache.keys()] : [];
  if (isExemptMember(message.author.id, roleIds, ctx.modConfig)) return;

  const verdict = tracker.record(
    message.author.id,
    message.content,
    message.mentions.users.size + message.mentions.roles.size,
    now,
  );
  if (!verdict.moderate) return;

  await message.delete().catch(() => undefined);
  const result = await applyServerRuleViolation(
    ctx,
    message.guild,
    message.author.id,
    ctx.client.user?.id ?? 'bot',
    'language',
    verdict.reason ?? undefined,
    now,
  );
  log.info(`Linguagem moderada: ${message.author.tag} — ${verdict.reason} (${result.summary})`);
}
