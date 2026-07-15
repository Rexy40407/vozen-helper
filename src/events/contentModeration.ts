import { type Message, type AutoModerationActionExecution } from 'discord.js';
import type { AppContext } from '../context.js';
import { findBannedWord, findDangerousAttachment } from '../moderation/contentFilter.js';
import { isExemptMember, isExemptChannel } from '../moderation/exempt.js';
import { escalateMember } from '../moderation/enforce.js';
import { log } from '../log.js';

// Filtro de conteúdo PRÓPRIO (pós-publicação) + consumo dos triggers do AutoMod
// nativo. Complementa o AutoMod: o nativo bloqueia keywords/slurs antes de publicar;
// aqui apanhamos evasão por homóglifos, anexos perigosos, e alimentamos a escalação.

/** Processa uma mensagem no filtro próprio. Só age no guild permitido. */
export async function handleMessageContent(ctx: AppContext, message: Message): Promise<void> {
  if (message.author?.bot) return;
  if (!message.inGuild() || message.guildId !== ctx.env.guildId) return;
  if (isExemptChannel(message.channelId, ctx.modConfig.automodExemptChannelIds)) return;

  const member = message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
  const roleIds = member ? [...member.roles.cache.keys()] : [];
  if (isExemptMember(message.author.id, roleIds, ctx.modConfig)) return;

  const banned = findBannedWord(message.content, ctx.modConfig.bannedWords);
  const attachmentNames = [...message.attachments.values()].map((a) => a.name ?? '');
  const badFile = findDangerousAttachment(attachmentNames, ctx.modConfig.dangerousExtensions);

  if (!banned && !badFile) return;

  const reason = banned ? `Palavra proibida: ${banned}` : `Anexo perigoso: ${badFile}`;
  await message.delete().catch((err) => log.debug('Não apaguei a mensagem:', (err as Error).message));

  if (member) {
    const summary = await escalateMember(ctx, message.guild, member, 'filter');
    log.info(`Filtro: ${message.author.tag} — ${reason}${summary ? ` ${summary}` : ''}`);
  }
}

/**
 * Consome uma execução do AutoMod NATIVO (ex.: bloqueou um slur): regista um strike e
 * escala se atingir um degrau. O bloqueio da mensagem já foi feito pelo Discord.
 */
export async function handleAutomodExecution(
  ctx: AppContext,
  exec: AutoModerationActionExecution,
): Promise<void> {
  if (exec.guild.id !== ctx.env.guildId) return;
  const member = await exec.guild.members.fetch(exec.userId).catch(() => null);
  if (member) {
    // escalateMember já regista o strike (source 'automod') e escala se atingir degrau.
    const summary = await escalateMember(ctx, exec.guild, member, 'automod');
    if (summary) log.info(`AutoMod strike → ${member.user.tag} ${summary}`);
  }
}
