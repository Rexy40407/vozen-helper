import {
  EmbedBuilder,
  type MessageReaction,
  type PartialMessageReaction,
  type User,
  type PartialUser,
} from 'discord.js';
import type { AppContext } from '../context.js';
import { getStarEntry, upsertStarEntry, deleteStarEntry } from './store.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';
import { resolveChannelId } from '../store/channelSettings.js';
import { createKeyedLock } from './keyedLock.js';

// Starboard: mensagens que juntam N ⭐ vão para um canal de destaque. Anti-self-star
// (a estrela do autor não conta). Atualiza o contador; remove se cair a zero.

// Lock por mensagem: serializa o read-modify-write. Sem isto, duas ⭐ quase simultâneas
// na mesma mensagem criavam dois posts (BD só referencia um → o outro órfão para sempre).
const starboardLock = createKeyedLock();

export async function handleStarReaction(
  ctx: AppContext,
  reaction: MessageReaction | PartialMessageReaction,
  _user: User | PartialUser,
): Promise<void> {
  const cfg = ctx.modConfig.community.starboard;
  const boardId = resolveChannelId(ctx.db, ctx.env.guildId, 'starboard', cfg.channelId);
  if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'starboard', cfg.enabled) || !boardId) return;
  if (reaction.emoji.name !== cfg.emoji) return;

  // Resolver parciais.
  if (reaction.partial) {
    const full = await reaction.fetch().catch(() => null);
    if (!full) return;
    reaction = full;
  }
  const message = reaction.message.partial ? await reaction.message.fetch().catch(() => null) : reaction.message;
  if (!message || !message.guild || message.guildId !== ctx.env.guildId) return;
  if (message.channelId === boardId) return; // não fazer starboard do próprio starboard

  // Lock por-mensagem: serializa o read-modify-write. Sem isto, duas ⭐ concorrentes na
  // mesma mensagem criavam dois posts no starboard (BD só referencia um → o outro órfão).
  // Se já há uma reação da mesma mensagem em processamento, ignora esta (a que está em
  // curso já busca a contagem atual do Discord, por isso não se perde correção).
  const lockKey = `${message.guildId}:${message.id}`;
  if (!starboardLock.tryAcquire(lockKey)) return;
  try {
    // Contar estrelas de humanos que não o autor.
    const users = await reaction.users.fetch().catch(() => null);
    if (!users) return;
    const count = users.filter((u) => !u.bot && u.id !== message.author?.id).size;

    const guild = message.guild;
    const board = await guild.channels.fetch(boardId).catch(() => null);
    if (!board || !board.isTextBased() || board.isDMBased()) return;

    const existing = getStarEntry(ctx.db, guild.id, message.id);

    if (count < cfg.threshold) {
      // Abaixo do threshold: se já lá estava, apaga.
      if (existing) {
        const old = await board.messages.fetch(existing.starboardMessageId).catch(() => null);
        await old?.delete().catch(() => undefined);
        deleteStarEntry(ctx.db, guild.id, message.id);
      }
      return;
    }

    const embed = new EmbedBuilder()
      .setColor(0xffac33)
      .setAuthor({ name: message.author?.tag ?? 'desconhecido', iconURL: message.author?.displayAvatarURL() })
      .setDescription(message.content || '*(sem texto)*')
      .addFields({ name: 'Original', value: `[ir para a mensagem](${message.url})` })
      .setFooter({ text: `⭐ ${count}` });
    const image = message.attachments.find((a) => a.contentType?.startsWith('image/'));
    if (image) embed.setImage(image.url);

    try {
      if (existing) {
        const msg = await board.messages.fetch(existing.starboardMessageId).catch(() => null);
        if (msg) await msg.edit({ content: `⭐ **${count}** <#${message.channelId}>`, embeds: [embed] });
        upsertStarEntry(ctx.db, guild.id, message.id, existing.starboardMessageId, count);
      } else {
        const posted = await board.send({ content: `⭐ **${count}** <#${message.channelId}>`, embeds: [embed] });
        upsertStarEntry(ctx.db, guild.id, message.id, posted.id, count);
      }
    } catch (err) {
      log.debug('starboard falhou:', (err as Error).message);
    }
  } finally {
    starboardLock.release(lockKey);
  }
}
