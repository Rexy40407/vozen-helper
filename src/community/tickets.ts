import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  EmbedBuilder,
  ActionRowBuilder,
  ButtonBuilder,
  ButtonStyle,
  ChannelType,
  AttachmentBuilder,
  type ButtonInteraction,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import {
  createTicket,
  getOpenTicketForUser,
  getTicketByChannel,
  setTicketStatus,
  claimTicket,
} from './store.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';
import { createKeyedLock } from './keyedLock.js';

// Tickets em PRIVATE THREADS (mais barato que canais). Painel com botão → thread
// privada; botões Fechar/Reclamar; transcript .txt ao fechar.

export const TICKET_OPEN_ID = 'ticket:open';

const ticketPanel: Command = {
  data: new SlashCommandBuilder()
    .setName('ticket-panel')
    .setDescription('Publish the ticket panel (staff).')
    .addChannelOption((o) =>
      o
        .setName('channel')
        .setDescription('Channel to post the panel in (default: here)')
        .addChannelTypes(ChannelType.GuildText),
    )
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'tickets', ctx.modConfig.community.tickets.enabled)) {
      return void interaction.reply({ content: 'Tickets are disabled.', flags: MessageFlags.Ephemeral });
    }
    // Canal-alvo: a opção `channel` (permite postar noutro canal a partir do canal de
    // comandos) ou, por omissão, o canal onde o comando foi corrido.
    const target = interaction.options.getChannel('channel') ?? interaction.channel;
    if (!target || target.type !== ChannelType.GuildText) {
      return void interaction.reply({ content: 'Pick a text channel.', flags: MessageFlags.Ephemeral });
    }
    const embed = new EmbedBuilder().setTitle('Support').setDescription('Need help? Open a ticket.').setColor(0x5865f2);
    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
      new ButtonBuilder().setCustomId(TICKET_OPEN_ID).setLabel('Open ticket').setEmoji('🎫').setStyle(ButtonStyle.Primary),
    );
    await target.send({ embeds: [embed], components: [row] });
    await interaction.reply({ content: `Panel published in <#${target.id}>.`, flags: MessageFlags.Ephemeral });
  },
};

function controlRow(): ActionRowBuilder<ButtonBuilder> {
  return new ActionRowBuilder<ButtonBuilder>().addComponents(
    new ButtonBuilder().setCustomId('ticket:claim').setLabel('Claim').setStyle(ButtonStyle.Secondary),
    new ButtonBuilder().setCustomId('ticket:close').setLabel('Close').setStyle(ButtonStyle.Danger),
  );
}

// Lock por (guild,user): um duplo-clique em "Abrir ticket" abria a check-then-create
// duas vezes → duas threads privadas + duas linhas na BD. Serializa por utilizador.
const ticketOpenLock = createKeyedLock();

async function openTicket(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const lockKey = `${interaction.guildId}:${interaction.user.id}`;
  if (!ticketOpenLock.tryAcquire(lockKey)) {
    return void interaction.reply({ content: "Hold on a moment — I'm already creating your ticket.", flags: MessageFlags.Ephemeral });
  }
  try {
    const existing = getOpenTicketForUser(ctx.db, interaction.guildId, interaction.user.id);
    if (existing) {
      return void interaction.reply({ content: `You already have an open ticket: <#${existing.channelId}>`, flags: MessageFlags.Ephemeral });
    }
    const parent = interaction.channel;
    if (!parent || parent.type !== ChannelType.GuildText) {
      return void interaction.reply({ content: "I can't create the ticket here.", flags: MessageFlags.Ephemeral });
    }
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });
    try {
      const thread = await parent.threads.create({
        name: `ticket-${interaction.user.username}`.slice(0, 90),
        type: ChannelType.PrivateThread,
        invitable: false,
      });
      await thread.members.add(interaction.user.id).catch(() => undefined);
      createTicket(ctx.db, interaction.guildId, thread.id, interaction.user.id, Date.now());
      const staffMention = ctx.modConfig.community.tickets.staffRoleId ? `<@&${ctx.modConfig.community.tickets.staffRoleId}> ` : '';
      await thread.send({
        content: `${staffMention}Ticket from <@${interaction.user.id}>. Staff will be with you shortly.`,
        components: [controlRow()],
      });
      await interaction.editReply(`Ticket created: <#${thread.id}>`);
    } catch (err) {
      log.error('Falha a criar ticket:', err);
      await interaction.editReply("I couldn't create the ticket (private thread permissions?).");
    }
  } finally {
    ticketOpenLock.release(lockKey);
  }
}

async function buildTranscript(interaction: ButtonInteraction): Promise<AttachmentBuilder | null> {
  const channel = interaction.channel;
  if (!channel || !channel.isThread()) return null;
  const msgs = await channel.messages.fetch({ limit: 100 }).catch(() => null);
  if (!msgs) return null;
  const lines = [...msgs.values()]
    .reverse()
    .map((m) => `[${new Date(m.createdTimestamp).toISOString()}] ${m.author.tag}: ${m.content}`);
  const buf = Buffer.from(lines.join('\n') || '(empty)', 'utf8');
  return new AttachmentBuilder(buf, { name: `transcript-${channel.id}.txt` });
}

async function closeTicket(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const ticket = getTicketByChannel(ctx.db, interaction.channelId);
  if (!ticket) return void interaction.reply({ content: 'This is not a ticket.', flags: MessageFlags.Ephemeral });
  // Idempotente: um 2º clique em "Fechar" não deve repostar o transcript nem re-arquivar.
  if (ticket.status === 'closed') {
    return void interaction.reply({ content: 'This ticket is already closed.', flags: MessageFlags.Ephemeral });
  }
  await interaction.reply({ content: 'Closing the ticket...' });
  setTicketStatus(ctx.db, ticket.id, 'closed');

  const transcript = await buildTranscript(interaction);
  const transcriptChannelId = ctx.modConfig.community.tickets.transcriptChannelId;
  if (transcript && transcriptChannelId) {
    const ch = await interaction.guild.channels.fetch(transcriptChannelId).catch(() => null);
    if (ch && ch.type === ChannelType.GuildText) {
      await ch.send({ content: `Ticket transcript from <@${ticket.openerId}> (closed by <@${interaction.user.id}>)`, files: [transcript] }).catch(() => undefined);
    }
  }
  if (interaction.channel?.isThread()) {
    await interaction.channel.setLocked(true).catch(() => undefined);
    await interaction.channel.setArchived(true).catch(() => undefined);
  }
}

async function claimTicketHandler(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const ticket = getTicketByChannel(ctx.db, interaction.channelId);
  if (!ticket) return void interaction.reply({ content: 'This is not a ticket.', flags: MessageFlags.Ephemeral });
  claimTicket(ctx.db, ticket.id, interaction.user.id);
  await interaction.reply({ content: `🙋 <@${interaction.user.id}> has claimed this ticket.` });
}

/** Router dos botões de ticket. */
export async function handleTicketButton(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (interaction.customId === TICKET_OPEN_ID) return openTicket(ctx, interaction);
  if (interaction.customId === 'ticket:close') return closeTicket(ctx, interaction);
  if (interaction.customId === 'ticket:claim') return claimTicketHandler(ctx, interaction);
}

export const ticketCommands: readonly Command[] = [ticketPanel];
