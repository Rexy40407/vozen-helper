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
  type Role,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import { setSelfRole, getSelfRole, getSelfRolesForMessage, type SelfRoleMode } from './store.js';
import { log } from '../log.js';

// Self-roles por BOTÕES (mais simples/robusto que reações). Modos:
//  - normal: alterna (dá/tira)
//  - unique: só uma role do painel (trocar remove as outras)
//  - verify: dá e nunca tira

/**
 * Cargos oferecíveis num painel: têm de estar abaixo do teto (o menor entre o cargo do
 * bot e o do moderador que cria o painel; o dono usa só o teto do bot) e não serem
 * `managed`. Impede um mod baixo de oferecer cargos acima do dele. Pura, testável.
 */
export function pickOfferableRoles<T extends { position: number; managed: boolean }>(
  roles: readonly T[],
  botTop: number,
  modTop: number,
  isOwner: boolean,
): T[] {
  const ceiling = isOwner ? botTop : Math.min(botTop, modTop);
  return roles.filter((r) => r.position < ceiling && !r.managed);
}

const rolepanel: Command = {
  data: new SlashCommandBuilder()
    .setName('rolepanel')
    .setDescription('Publish a self-roles panel in this channel (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageRoles)
    .addStringOption((o) => o.setName('title').setDescription('Panel title').setRequired(true))
    .addRoleOption((o) => o.setName('role1').setDescription('Role 1').setRequired(true))
    .addStringOption((o) =>
      o
        .setName('mode')
        .setDescription('Behavior')
        .addChoices(
          { name: 'Normal (toggle)', value: 'normal' },
          { name: 'Unique (only one from panel)', value: 'unique' },
          { name: 'Verification (add only)', value: 'verify' },
        ),
    )
    .addRoleOption((o) => o.setName('role2').setDescription('Role 2'))
    .addRoleOption((o) => o.setName('role3').setDescription('Role 3'))
    .addRoleOption((o) => o.setName('role4').setDescription('Role 4'))
    .addRoleOption((o) => o.setName('role5').setDescription('Role 5')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    if (!interaction.channel || interaction.channel.type !== ChannelType.GuildText) {
      return void interaction.reply({
        content: 'Run this in a text channel.',
        flags: MessageFlags.Ephemeral,
      });
    }
    const mode = (interaction.options.getString('mode') ?? 'normal') as SelfRoleMode;
    const roles: Role[] = [];
    for (const key of ['role1', 'role2', 'role3', 'role4', 'role5']) {
      const r = interaction.options.getRole(key);
      if (r) roles.push(r as Role);
    }
    const botTop = interaction.guild.members.me?.roles.highest.position ?? 0;
    const isOwner = interaction.guild.ownerId === interaction.member.id;
    const modTop = interaction.member.roles.highest.position;
    const usable = pickOfferableRoles(roles, botTop, modTop, isOwner);
    if (!usable.length) {
      return void interaction.reply({
        content: 'No role can be offered — they must be below your role and mine.',
        flags: MessageFlags.Ephemeral,
      });
    }

    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
      usable.map((r) =>
        new ButtonBuilder()
          .setCustomId(`role:${r.id}`)
          .setLabel(r.name)
          .setStyle(ButtonStyle.Secondary),
      ),
    );
    const embed = new EmbedBuilder()
      .setTitle(interaction.options.getString('title', true))
      .setColor(0x5865f2)
      .setDescription(usable.map((r) => `• <@&${r.id}>`).join('\n'));
    const msg = await interaction.channel.send({ embeds: [embed], components: [row] });
    for (const r of usable) setSelfRole(ctx.db, msg.id, `role:${r.id}`, r.id, mode);
    await interaction.reply({ content: 'Panel published.', flags: MessageFlags.Ephemeral });
  },
};

/** Handler dos botões de self-role (role:<roleId>). */
export async function handleSelfRoleButton(
  ctx: AppContext,
  interaction: ButtonInteraction,
): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const entry = getSelfRole(ctx.db, interaction.message.id, interaction.customId);
  if (!entry)
    return void interaction.reply({
      content: 'This button is no longer valid.',
      flags: MessageFlags.Ephemeral,
    });

  const member = interaction.member;
  const has = member.roles.cache.has(entry.roleId);
  try {
    if (entry.mode === 'verify') {
      if (has)
        return void interaction.reply({
          content: 'You already have that role. ✅',
          flags: MessageFlags.Ephemeral,
        });
      await member.roles.add(entry.roleId, 'Self-role (verify)');
      return void interaction.reply({
        content: 'Role assigned. ✅',
        flags: MessageFlags.Ephemeral,
      });
    }
    if (entry.mode === 'unique') {
      // Remover as outras roles do mesmo painel, depois dar esta.
      const all = getSelfRolesForMessage(ctx.db, interaction.message.id).map((r) => r.roleId);
      const toRemove = all.filter((rid) => rid !== entry.roleId && member.roles.cache.has(rid));
      if (toRemove.length) await member.roles.remove(toRemove, 'Self-role (unique)');
      if (has) {
        await member.roles.remove(entry.roleId, 'Self-role (unique toggle)');
        return void interaction.reply({ content: 'Role removed.', flags: MessageFlags.Ephemeral });
      }
      await member.roles.add(entry.roleId, 'Self-role (unique)');
      return void interaction.reply({
        content: 'Role switched. ✅',
        flags: MessageFlags.Ephemeral,
      });
    }
    // normal: alterna
    if (has) {
      await member.roles.remove(entry.roleId, 'Self-role (toggle off)');
      await interaction.reply({ content: 'Role removed.', flags: MessageFlags.Ephemeral });
    } else {
      await member.roles.add(entry.roleId, 'Self-role (toggle on)');
      await interaction.reply({ content: 'Role assigned. ✅', flags: MessageFlags.Ephemeral });
    }
  } catch (err) {
    log.error('Falha no self-role:', err);
    await interaction.reply({
      content: "I couldn't change the role (permissions/hierarchy?).",
      flags: MessageFlags.Ephemeral,
    });
  }
}

export const selfRoleCommands: readonly Command[] = [rolepanel];
