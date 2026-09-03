import type { Message, GuildMember, PartialGuildMember } from 'discord.js';
import type { AppContext } from '../context.js';
import { scanForScam } from '../moderation/phishing.js';
import {
  needsDehoist,
  sanitizeName,
  isUnreadable,
  isImpersonating,
} from '../moderation/nickname.js';
import { isExemptMember, isExemptChannel } from '../moderation/exempt.js';
import { escalateMember } from '../moderation/enforce.js';
import { recordCase } from '../moderation/service.js';
import { alertOwner } from '../moderation/quarantineService.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';

// Anti-scam (phishing por URL) + honeypot + dehoist/decancer + alerta de impersonation.

/** Filtro de scam numa mensagem + honeypot. */
export async function handleMessageScam(ctx: AppContext, message: Message): Promise<void> {
  if (message.author?.bot) return;
  if (!message.inGuild() || message.guildId !== ctx.env.guildId) return;

  // Honeypot: qualquer humano que escreva no canal-armadilha é banido.
  if (ctx.modConfig.honeypotChannelId && message.channelId === ctx.modConfig.honeypotChannelId) {
    await message.delete().catch(() => undefined);
    await message.guild.bans
      .create(message.author.id, { reason: 'Honeypot', deleteMessageSeconds: 24 * 60 * 60 })
      .catch((err) => log.error('Honeypot ban falhou:', err));
    recordCase(ctx, {
      guildId: message.guildId,
      type: 'ban',
      targetId: message.author.id,
      moderatorId: ctx.client.user?.id ?? 'bot',
      reason: 'Honeypot',
      createdAt: Date.now(),
    });
    return;
  }

  if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'antiscam', ctx.modConfig.antiScam.enabled))
    return;
  if (isExemptChannel(message.channelId, ctx.modConfig.automodExemptChannelIds)) return;
  const member =
    message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
  const roleIds = member ? [...member.roles.cache.keys()] : [];
  if (isExemptMember(message.author.id, roleIds, ctx.modConfig)) return;

  const result = scanForScam(
    message.content,
    ctx.modConfig.antiScam.blacklistDomains,
    ctx.modConfig.antiScam.protectedDomains,
  );
  if (!result.hit) return;

  await message.delete().catch(() => undefined);
  if (member) {
    const summary = await escalateMember(ctx, message.guild, member, 'scam');
    log.info(
      `Anti-scam: ${message.author.tag} — ${result.kind} ${result.domain}${summary ? ` ${summary}` : ''}`,
    );
  }
}

/** Dehoist/decancer + alerta de impersonation ao entrar ou mudar de nome. */
export async function handleNickname(
  ctx: AppContext,
  member: GuildMember | PartialGuildMember,
): Promise<void> {
  if (member.guild.id !== ctx.env.guildId) return;
  if (member.partial) return;
  const cfg = ctx.modConfig.nickname;

  const display = member.displayName;
  if (cfg.dehoistEnabled && (needsDehoist(display) || isUnreadable(display))) {
    const clean = sanitizeName(display, cfg.fallbackName);
    if (clean !== display) {
      await member.setNickname(clean, 'Automatic dehoist/decancer').catch(() => undefined);
    }
  }

  if (cfg.protectedNames.length && isImpersonating(display, cfg.protectedNames)) {
    await alertOwner(
      ctx,
      member.guild,
      `⚠️ Possível impersonation de staff: <@${member.id}> ("${display}").`,
    );
  }
}
