import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  EmbedBuilder,
  ChannelType,
} from 'discord.js';
import type { Command } from './index.js';
import { getCasesForUser } from '../store/cases.js';
import { snowflakeToTimestamp } from '../moderation/snowflake.js';
import { log } from '../log.js';

// Utilitários de moderação (Fase 8): /slowmode e /userinfo.

function eph(content: string) {
  return { content, flags: MessageFlags.Ephemeral } as const;
}

const slowmode: Command = {
  data: new SlashCommandBuilder()
    .setName('slowmode')
    .setDescription("Sets the channel's slowmode (0 = off).")
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageChannels)
    .addIntegerOption((o) =>
      o.setName('seconds').setDescription('Seconds (0-21600)').setRequired(true).setMinValue(0).setMaxValue(21600),
    ) as SlashCommandBuilder,
  async execute(interaction) {
    if (!interaction.inCachedGuild()) return;
    const seconds = interaction.options.getInteger('seconds', true);
    const channel = interaction.channel;
    if (!channel || channel.type !== ChannelType.GuildText) {
      return void interaction.reply(eph('Only works in text channels.'));
    }
    try {
      await channel.setRateLimitPerUser(seconds, 'Slowmode via /slowmode');
      await interaction.reply(eph(seconds === 0 ? 'Slowmode off.' : `Slowmode: ${seconds}s.`));
    } catch (err) {
      log.error('Falha no slowmode:', err);
      await interaction.reply(eph("Couldn't change the slowmode."));
    }
  },
};

const userinfo: Command = {
  data: new SlashCommandBuilder()
    .setName('userinfo')
    .setDescription('Shows triage information about a user.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('User').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const member = await interaction.guild.members.fetch(user.id).catch(() => null);
    const created = Math.floor(snowflakeToTimestamp(user.id) / 1000);
    const caseCount = getCasesForUser(ctx.db, interaction.guildId, user.id, 100).length;
    const embed = new EmbedBuilder()
      .setTitle(`Info for ${user.tag}`)
      .setThumbnail(user.displayAvatarURL())
      .addFields(
        { name: 'ID', value: user.id, inline: true },
        { name: 'Account created', value: `<t:${created}:R>`, inline: true },
        { name: 'Cases', value: String(caseCount), inline: true },
      );
    if (member?.joinedTimestamp) {
      embed.addFields({ name: 'Joined', value: `<t:${Math.floor(member.joinedTimestamp / 1000)}:R>`, inline: true });
      const roles = [...member.roles.cache.keys()].filter((r) => r !== interaction.guildId);
      if (roles.length) embed.addFields({ name: 'Roles', value: roles.map((r) => `<@&${r}>`).join(' ').slice(0, 900) });
    } else {
      embed.addFields({ name: 'In server?', value: 'No', inline: true });
    }
    await interaction.reply({ embeds: [embed], flags: MessageFlags.Ephemeral });
  },
};

export const utilCommands: readonly Command[] = [slowmode, userinfo];
