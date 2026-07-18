import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  EmbedBuilder,
  ActionRowBuilder,
  ButtonBuilder,
  ButtonStyle,
  ChannelType,
  type ButtonInteraction,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import {
  createSuggestion,
  setSuggestionMessage,
  getSuggestion,
  setSuggestionStatus,
  voteSuggestion,
  countSuggestionVotes,
  type SuggestionStatus,
} from './store.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';
import { resolveChannelId } from '../store/channelSettings.js';

// Sugestões: /suggest cria um embed numerado com votos 👍/👎 por botão; a staff
// aprova/nega/considera com /suggestion. Votos guardados na DB (à prova de restart).

const STATUS_META: Record<SuggestionStatus, { color: number; label: string }> = {
  pending: { color: 0x5865f2, label: '⏳ Pending' },
  approved: { color: 0x57f287, label: '✅ Approved' },
  denied: { color: 0xed4245, label: '❌ Denied' },
  considered: { color: 0xfee75c, label: '🤔 Under consideration' },
};

function buildEmbed(
  id: number,
  content: string,
  authorTag: string,
  status: SuggestionStatus,
  votes: { up: number; down: number },
  reason?: string,
): EmbedBuilder {
  const meta = STATUS_META[status];
  const embed = new EmbedBuilder()
    .setTitle(`Suggestion #${id}`)
    .setColor(meta.color)
    .setDescription(content)
    .addFields(
      { name: 'Status', value: meta.label, inline: true },
      { name: 'Votes', value: `👍 ${votes.up} · 👎 ${votes.down}`, inline: true },
    )
    .setFooter({ text: `By ${authorTag}` });
  if (reason) embed.addFields({ name: 'Staff response', value: reason });
  return embed;
}

function voteRow(id: number): ActionRowBuilder<ButtonBuilder> {
  return new ActionRowBuilder<ButtonBuilder>().addComponents(
    new ButtonBuilder().setCustomId(`sug:up:${id}`).setLabel('👍').setStyle(ButtonStyle.Success),
    new ButtonBuilder().setCustomId(`sug:down:${id}`).setLabel('👎').setStyle(ButtonStyle.Danger),
  );
}

const suggest: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('suggest')
    .setDescription('Send a suggestion to the server.')
    .addStringOption((o) => o.setName('text').setDescription('Your suggestion').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const cfg = ctx.modConfig.community.suggestions;
    const chanId = resolveChannelId(ctx.db, ctx.env.guildId, 'suggestions', cfg.channelId);
    if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'suggestions', cfg.enabled) || !chanId) {
      return void interaction.reply({ content: 'Suggestions are not configured.', flags: MessageFlags.Ephemeral });
    }
    const channel = await interaction.guild.channels.fetch(chanId).catch(() => null);
    if (!channel || channel.type !== ChannelType.GuildText) {
      return void interaction.reply({ content: 'Invalid suggestions channel.', flags: MessageFlags.Ephemeral });
    }
    const content = interaction.options.getString('text', true);
    const id = createSuggestion(ctx.db, interaction.guildId, interaction.user.id, content, Date.now());
    const msg = await channel.send({
      embeds: [buildEmbed(id, content, interaction.user.tag, 'pending', { up: 0, down: 0 })],
      components: [voteRow(id)],
    });
    setSuggestionMessage(ctx.db, id, msg.id);
    await interaction.reply({ content: `Suggestion #${id} sent! ${channel}`, flags: MessageFlags.Ephemeral });
  },
};

const suggestion: Command = {
  data: new SlashCommandBuilder()
    .setName('suggestion')
    .setDescription('Reply to a suggestion (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addStringOption((o) =>
      o
        .setName('decision')
        .setDescription('Decision')
        .setRequired(true)
        .addChoices(
          { name: 'Approve', value: 'approved' },
          { name: 'Deny', value: 'denied' },
          { name: 'Consider', value: 'considered' },
        ),
    )
    .addIntegerOption((o) => o.setName('id').setDescription('Suggestion number').setRequired(true).setMinValue(1))
    .addStringOption((o) => o.setName('reason').setDescription('Reason (optional)')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const id = interaction.options.getInteger('id', true);
    const status = interaction.options.getString('decision', true) as SuggestionStatus;
    const reason = interaction.options.getString('reason') ?? undefined;
    const sug = getSuggestion(ctx.db, id);
    if (!sug || sug.guildId !== interaction.guildId) {
      return void interaction.reply({ content: `Suggestion #${id} not found.`, flags: MessageFlags.Ephemeral });
    }
    // As chamadas seguintes (fetch canal/msg/autor + edit + DM) podem passar a janela de 3s
    // do ack — defer primeiro para o token não expirar ("Unknown interaction").
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });
    setSuggestionStatus(ctx.db, id, status);
    const cfg = ctx.modConfig.community.suggestions;
    const chanId = resolveChannelId(ctx.db, ctx.env.guildId, 'suggestions', cfg.channelId);
    if (chanId && sug.messageId) {
      const channel = await interaction.guild.channels.fetch(chanId).catch(() => null);
      if (channel && channel.type === ChannelType.GuildText) {
        const msg = await channel.messages.fetch(sug.messageId).catch(() => null);
        const author = await interaction.client.users.fetch(sug.authorId).catch(() => null);
        if (msg) {
          await msg
            .edit({
              embeds: [buildEmbed(id, sug.content, author?.tag ?? 'unknown', status, countSuggestionVotes(ctx.db, id), reason)],
              components: status === 'considered' ? [voteRow(id)] : [],
            })
            .catch(() => undefined);
        }
        // DM ao autor (falha silenciosa).
        await author?.send(`Your suggestion #${id} was ${STATUS_META[status].label}.${reason ? `\nReason: ${reason}` : ''}`).catch(() => undefined);
      }
    }
    await interaction.editReply({ content: `Suggestion #${id} → ${STATUS_META[status].label}.` });
  },
};

/** Handler dos botões de voto (sug:up:<id> / sug:down:<id>). */
export async function handleSuggestionVote(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const [, dir, idStr] = interaction.customId.split(':');
  const id = Number(idStr);
  const sug = getSuggestion(ctx.db, id);
  if (!sug) return void interaction.reply({ content: 'Suggestion not found.', flags: MessageFlags.Ephemeral });

  voteSuggestion(ctx.db, id, interaction.user.id, dir === 'up' ? 1 : -1);
  const votes = countSuggestionVotes(ctx.db, id);
  const author = await interaction.client.users.fetch(sug.authorId).catch(() => null);
  // O motivo da staff não é persistido na BD — recupera-se do embed atual para não
  // desaparecer ao re-renderizar depois de um voto numa sugestão "em consideração".
  const staffReason = interaction.message.embeds[0]?.fields?.find((f) => f.name === 'Staff response')?.value;
  try {
    await interaction.update({
      embeds: [buildEmbed(id, sug.content, author?.tag ?? 'unknown', sug.status, votes, staffReason)],
    });
  } catch (err) {
    log.debug('Falha a atualizar voto:', (err as Error).message);
  }
}

export const suggestionCommands: readonly Command[] = [suggest, suggestion];
