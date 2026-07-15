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
import { parseDuration } from '../moderation/duration.js';
import {
  createGiveaway,
  setGiveawayMessage,
  getGiveaway,
  markGiveawayEnded,
  listActiveGiveaways,
  addGiveawayEntry,
  removeGiveawayEntry,
  getGiveawayEntries,
} from './store.js';
import { scheduleAction, cancelScheduledByPayload } from '../store/cases.js';
import { log } from '../log.js';

// Giveaways: /gstart, botão de entrada (entries na DB), fim automático via scheduler
// (sobrevive a restart), /gend, /greroll, /glist.

/** Escolhe `count` vencedores de forma aleatória. `rnd` injetável para testes. */
export function pickWinners(entries: readonly string[], count: number, rnd: () => number): string[] {
  const pool = [...entries];
  const winners: string[] = [];
  while (winners.length < count && pool.length > 0) {
    const i = Math.floor(rnd() * pool.length);
    winners.push(pool.splice(i, 1)[0]);
  }
  return winners;
}

function embed(prize: string, winners: number, endAt: number, entries: number, host: string): EmbedBuilder {
  return new EmbedBuilder()
    .setTitle('🎉 Giveaway!')
    .setColor(0x57f287)
    .setDescription(`**${prize}**`)
    .addFields(
      { name: 'Vencedores', value: String(winners), inline: true },
      { name: 'Termina', value: `<t:${Math.floor(endAt / 1000)}:R>`, inline: true },
      { name: 'Participantes', value: String(entries), inline: true },
    )
    .setFooter({ text: `Organizado por ${host}` });
}

function enterRow(id: number, disabled = false): ActionRowBuilder<ButtonBuilder> {
  return new ActionRowBuilder<ButtonBuilder>().addComponents(
    new ButtonBuilder().setCustomId(`gw:enter:${id}`).setLabel('🎉 Entrar').setStyle(ButtonStyle.Primary).setDisabled(disabled),
  );
}

const gstart: Command = {
  data: new SlashCommandBuilder()
    .setName('gstart')
    .setDescription('Inicia um giveaway (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
    .addStringOption((o) => o.setName('duracao').setDescription('Ex.: 1h, 1d').setRequired(true))
    .addIntegerOption((o) => o.setName('vencedores').setDescription('Nº de vencedores').setRequired(true).setMinValue(1).setMaxValue(20))
    .addStringOption((o) => o.setName('premio').setDescription('O prémio').setRequired(true))
    .addRoleOption((o) => o.setName('cargo_obrigatorio').setDescription('Cargo necessário para entrar')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    if (!interaction.channel || interaction.channel.type !== ChannelType.GuildText) {
      return void interaction.reply({ content: 'Corre isto num canal de texto.', flags: MessageFlags.Ephemeral });
    }
    const ms = parseDuration(interaction.options.getString('duracao', true));
    if (ms === null) return void interaction.reply({ content: 'Duração inválida.', flags: MessageFlags.Ephemeral });
    const winners = interaction.options.getInteger('vencedores', true);
    const prize = interaction.options.getString('premio', true);
    const requiredRole = interaction.options.getRole('cargo_obrigatorio');
    const now = Date.now();
    const endAt = now + ms;
    const id = createGiveaway(ctx.db, {
      guildId: interaction.guildId,
      channelId: interaction.channelId,
      prize,
      winners,
      endAt,
      requiredRoleId: requiredRole?.id ?? null,
      hostId: interaction.user.id,
      createdAt: now,
    });
    const msg = await interaction.channel.send({ embeds: [embed(prize, winners, endAt, 0, interaction.user.tag)], components: [enterRow(id)] });
    setGiveawayMessage(ctx.db, id, msg.id);
    scheduleAction(ctx.db, { guildId: interaction.guildId, type: 'giveaway_end', targetId: interaction.user.id, executeAt: endAt, payload: String(id), caseId: null });
    await interaction.reply({ content: `Giveaway #${id} criado!`, flags: MessageFlags.Ephemeral });
  },
};

const gend: Command = {
  data: new SlashCommandBuilder()
    .setName('gend')
    .setDescription('Termina já um giveaway (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
    .addIntegerOption((o) => o.setName('id').setDescription('Nº do giveaway').setRequired(true).setMinValue(1)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const id = interaction.options.getInteger('id', true);
    const gw = getGiveaway(ctx.db, id);
    if (!gw || gw.guildId !== interaction.guildId || gw.ended) {
      return void interaction.reply({ content: 'Giveaway não encontrado ou já terminado.', flags: MessageFlags.Ephemeral });
    }
    cancelScheduledByPayload(ctx.db, interaction.guildId, 'giveaway_end', String(id));
    await endGiveaway(ctx, id);
    await interaction.reply({ content: `Giveaway #${id} terminado.`, flags: MessageFlags.Ephemeral });
  },
};

const greroll: Command = {
  data: new SlashCommandBuilder()
    .setName('greroll')
    .setDescription('Re-sorteia um giveaway terminado (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
    .addIntegerOption((o) => o.setName('id').setDescription('Nº do giveaway').setRequired(true).setMinValue(1)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const id = interaction.options.getInteger('id', true);
    const gw = getGiveaway(ctx.db, id);
    if (!gw || gw.guildId !== interaction.guildId) {
      return void interaction.reply({ content: 'Giveaway não encontrado.', flags: MessageFlags.Ephemeral });
    }
    // Só re-sortear giveaways JÁ terminados: re-sortear um ativo anunciava vencedores
    // agora e o `giveaway_end` agendado voltava a anunciar depois (vencedores a dobrar).
    if (!gw.ended) {
      return void interaction.reply({ content: 'Esse giveaway ainda está a decorrer. Usa `/gend` para o terminar primeiro.', flags: MessageFlags.Ephemeral });
    }
    const entries = getGiveawayEntries(ctx.db, id);
    const winners = pickWinners(entries, gw.winners, Math.random);
    if (!winners.length) return void interaction.reply({ content: 'Sem participantes para re-sortear.', flags: MessageFlags.Ephemeral });
    const channel = await interaction.guild.channels.fetch(gw.channelId).catch(() => null);
    if (channel && channel.type === ChannelType.GuildText) {
      await channel.send(`🎉 Novo sorteio de **${gw.prize}**: ${winners.map((w) => `<@${w}>`).join(', ')}!`);
    }
    await interaction.reply({ content: 'Re-sorteado.', flags: MessageFlags.Ephemeral });
  },
};

const glist: Command = {
  data: new SlashCommandBuilder()
    .setName('glist')
    .setDescription('Lista os giveaways ativos (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const active = listActiveGiveaways(ctx.db, interaction.guildId);
    const lines = active.map((g) => `#${g.id} — **${g.prize}** (termina <t:${Math.floor(g.endAt / 1000)}:R>)`);
    await interaction.reply({ content: lines.length ? lines.join('\n') : 'Sem giveaways ativos.', flags: MessageFlags.Ephemeral });
  },
};

/** Handler do botão de entrada (gw:enter:<id>) — alterna a participação. */
export async function handleGiveawayButton(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const id = Number(interaction.customId.split(':')[2]);
  const gw = getGiveaway(ctx.db, id);
  if (!gw || gw.ended) return void interaction.reply({ content: 'Este giveaway já terminou.', flags: MessageFlags.Ephemeral });
  if (gw.requiredRoleId && !interaction.member.roles.cache.has(gw.requiredRoleId)) {
    return void interaction.reply({ content: `Precisas do cargo <@&${gw.requiredRoleId}> para entrar.`, flags: MessageFlags.Ephemeral });
  }
  const added = addGiveawayEntry(ctx.db, id, interaction.user.id);
  if (!added) {
    removeGiveawayEntry(ctx.db, id, interaction.user.id);
    await interaction.reply({ content: 'Saíste do giveaway.', flags: MessageFlags.Ephemeral });
  } else {
    await interaction.reply({ content: 'Entraste no giveaway! 🎉', flags: MessageFlags.Ephemeral });
  }
  // Atualizar o contador de participantes no embed.
  const count = getGiveawayEntries(ctx.db, id).length;
  const author = await ctx.client.users.fetch(gw.hostId).catch(() => null);
  await interaction.message
    .edit({ embeds: [embed(gw.prize, gw.winners, gw.endAt, count, author?.tag ?? 'staff')], components: [enterRow(id)] })
    .catch(() => undefined);
}

/** Termina o giveaway: escolhe vencedores, anuncia, marca terminado. */
export async function endGiveaway(ctx: AppContext, id: number): Promise<void> {
  const gw = getGiveaway(ctx.db, id);
  if (!gw || gw.ended) return;
  markGiveawayEnded(ctx.db, id);
  const entries = getGiveawayEntries(ctx.db, id);
  const winners = pickWinners(entries, gw.winners, Math.random);
  const guild = await ctx.client.guilds.fetch(gw.guildId).catch(() => null);
  const channel = guild ? await guild.channels.fetch(gw.channelId).catch(() => null) : null;
  if (!channel || channel.type !== ChannelType.GuildText) return;

  if (gw.messageId) {
    const msg = await channel.messages.fetch(gw.messageId).catch(() => null);
    const author = await ctx.client.users.fetch(gw.hostId).catch(() => null);
    await msg
      ?.edit({
        embeds: [embed(gw.prize, gw.winners, gw.endAt, entries.length, author?.tag ?? 'staff').setTitle('🎉 Giveaway terminado')],
        components: [enterRow(id, true)],
      })
      .catch(() => undefined);
  }
  if (winners.length) {
    await channel.send(`🎉 Parabéns ${winners.map((w) => `<@${w}>`).join(', ')}! Ganharam **${gw.prize}**!`).catch(() => undefined);
  } else {
    await channel.send(`Ninguém participou no giveaway de **${gw.prize}**. 😢`).catch(() => undefined);
  }
  log.info(`Giveaway #${id} terminado (${winners.length} vencedor(es)).`);
}

export const giveawayCommands: readonly Command[] = [gstart, gend, greroll, glist];
