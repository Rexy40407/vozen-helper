import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  ChannelType,
  EmbedBuilder,
  ActionRowBuilder,
  ButtonBuilder,
  ButtonStyle,
  type ChatInputCommandInteraction,
  type ButtonInteraction,
} from 'discord.js';
import type { Command } from './index.js';
import type { AppContext } from '../context.js';
import { unquarantineMember } from '../moderation/quarantineService.js';
import { isQuarantined } from '../store/quarantine.js';
import { log } from '../log.js';

// Comandos de segurança (Fase 6): lockdown/unlock de canais e painel de verificação.

export const VERIFY_BUTTON_ID = 'vh_verify';

function eph(content: string) {
  return { content, flags: MessageFlags.Ephemeral } as const;
}

/** Tranca ou destranca os canais de texto (SendMessages para @everyone). */
async function setLock(interaction: ChatInputCommandInteraction<'cached'>, lock: boolean): Promise<number> {
  const guild = interaction.guild;
  const everyone = guild.roles.everyone;
  let changed = 0;
  for (const channel of guild.channels.cache.values()) {
    if (channel.type !== ChannelType.GuildText) continue;
    try {
      await channel.permissionOverwrites.edit(
        everyone,
        { SendMessages: lock ? false : null },
        { reason: lock ? 'Lockdown' : 'Unlock' },
      );
      changed++;
    } catch (err) {
      log.debug(`lock/unlock falhou em ${channel.id}:`, (err as Error).message);
    }
  }
  return changed;
}

const lockdown: Command = {
  data: new SlashCommandBuilder()
    .setName('lockdown')
    .setDescription('Tranca todos os canais de texto (só staff fala).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageChannels) as SlashCommandBuilder,
  async execute(interaction) {
    if (!interaction.inCachedGuild()) return;
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });
    const n = await setLock(interaction, true);
    await interaction.editReply(`🔒 Lockdown: ${n} canal(is) trancado(s).`);
  },
};

const unlock: Command = {
  data: new SlashCommandBuilder()
    .setName('unlock')
    .setDescription('Destranca todos os canais de texto.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageChannels) as SlashCommandBuilder,
  async execute(interaction) {
    if (!interaction.inCachedGuild()) return;
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });
    const n = await setLock(interaction, false);
    await interaction.editReply(`🔓 Unlock: ${n} canal(is) destrancado(s).`);
  },
};

const verifyPanel: Command = {
  data: new SlashCommandBuilder()
    .setName('verify-panel')
    .setDescription('Publica o painel de verificação (botão) neste canal.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    if (!ctx.modConfig.verification.verifiedRoleId) {
      return void interaction.reply(eph('Define primeiro `verification.verifiedRoleId` na config.'));
    }
    const embed = new EmbedBuilder()
      .setTitle('Verificação')
      .setDescription('Carrega no botão abaixo para teres acesso ao servidor.')
      .setColor(0x57f287);
    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
      new ButtonBuilder().setCustomId(VERIFY_BUTTON_ID).setLabel('Verificar').setStyle(ButtonStyle.Success),
    );
    if (interaction.channel && interaction.channel.type === ChannelType.GuildText) {
      await interaction.channel.send({ embeds: [embed], components: [row] });
      await interaction.reply(eph('Painel publicado.'));
    } else {
      await interaction.reply(eph('Corre isto num canal de texto.'));
    }
  },
};

/** Handler do botão de verificação: atribui o cargo de verificado. */
export async function handleVerifyButton(interaction: ButtonInteraction, ctx: AppContext): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const roleId = ctx.modConfig.verification.verifiedRoleId;
  if (!roleId) return void interaction.reply(eph('Verificação não configurada.'));
  const member = interaction.member;
  if (member.roles.cache.has(roleId)) {
    return void interaction.reply(eph('Já estás verificado. ✅'));
  }
  try {
    await member.roles.add(roleId, 'Verificação por botão');
    await interaction.reply(eph('Verificado! Bem-vindo(a). ✅'));
  } catch (err) {
    log.error('Falha a verificar:', err);
    await interaction.reply(eph('Não consegui atribuir o cargo (permissões/hierarquia?).'));
  }
}

const unquarantine: Command = {
  data: new SlashCommandBuilder()
    .setName('unquarantine')
    .setDescription('Restaura os cargos de um membro em quarentena (anti-nuke).')
    .setDefaultMemberPermissions(PermissionFlagsBits.Administrator)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    if (!isQuarantined(ctx.db, interaction.guildId, user.id)) {
      return void interaction.reply(eph('Esse utilizador não está em quarentena.'));
    }
    const member = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!member) return void interaction.reply(eph('O membro já não está no servidor.'));
    const ok = await unquarantineMember(ctx, interaction.guild, member);
    await interaction.reply(eph(ok ? `Cargos de ${user.tag} restaurados.` : 'Não consegui restaurar.'));
  },
};

export const securityCommands: readonly Command[] = [lockdown, unlock, verifyPanel, unquarantine];
