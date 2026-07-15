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
    .setDescription('Publica um painel de self-roles neste canal (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageRoles)
    .addStringOption((o) => o.setName('titulo').setDescription('Título do painel').setRequired(true))
    .addRoleOption((o) => o.setName('role1').setDescription('Cargo 1').setRequired(true))
    .addStringOption((o) =>
      o
        .setName('modo')
        .setDescription('Comportamento')
        .addChoices(
          { name: 'Normal (alterna)', value: 'normal' },
          { name: 'Único (só um do painel)', value: 'unique' },
          { name: 'Verificação (só dá)', value: 'verify' },
        ),
    )
    .addRoleOption((o) => o.setName('role2').setDescription('Cargo 2'))
    .addRoleOption((o) => o.setName('role3').setDescription('Cargo 3'))
    .addRoleOption((o) => o.setName('role4').setDescription('Cargo 4'))
    .addRoleOption((o) => o.setName('role5').setDescription('Cargo 5')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    if (!interaction.channel || interaction.channel.type !== ChannelType.GuildText) {
      return void interaction.reply({ content: 'Corre isto num canal de texto.', flags: MessageFlags.Ephemeral });
    }
    const mode = (interaction.options.getString('modo') ?? 'normal') as SelfRoleMode;
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
      return void interaction.reply({ content: 'Nenhum cargo é oferecível — têm de estar abaixo do teu cargo e do meu.', flags: MessageFlags.Ephemeral });
    }

    const row = new ActionRowBuilder<ButtonBuilder>().addComponents(
      usable.map((r) => new ButtonBuilder().setCustomId(`role:${r.id}`).setLabel(r.name).setStyle(ButtonStyle.Secondary)),
    );
    const embed = new EmbedBuilder()
      .setTitle(interaction.options.getString('titulo', true))
      .setColor(0x5865f2)
      .setDescription(usable.map((r) => `• <@&${r.id}>`).join('\n'));
    const msg = await interaction.channel.send({ embeds: [embed], components: [row] });
    for (const r of usable) setSelfRole(ctx.db, msg.id, `role:${r.id}`, r.id, mode);
    await interaction.reply({ content: 'Painel publicado.', flags: MessageFlags.Ephemeral });
  },
};

/** Handler dos botões de self-role (role:<roleId>). */
export async function handleSelfRoleButton(ctx: AppContext, interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const entry = getSelfRole(ctx.db, interaction.message.id, interaction.customId);
  if (!entry) return void interaction.reply({ content: 'Este botão já não é válido.', flags: MessageFlags.Ephemeral });

  const member = interaction.member;
  const has = member.roles.cache.has(entry.roleId);
  try {
    if (entry.mode === 'verify') {
      if (has) return void interaction.reply({ content: 'Já tens esse cargo. ✅', flags: MessageFlags.Ephemeral });
      await member.roles.add(entry.roleId, 'Self-role (verify)');
      return void interaction.reply({ content: 'Cargo atribuído. ✅', flags: MessageFlags.Ephemeral });
    }
    if (entry.mode === 'unique') {
      // Remover as outras roles do mesmo painel, depois dar esta.
      const all = getSelfRolesForMessage(ctx.db, interaction.message.id).map((r) => r.roleId);
      const toRemove = all.filter((rid) => rid !== entry.roleId && member.roles.cache.has(rid));
      if (toRemove.length) await member.roles.remove(toRemove, 'Self-role (unique)');
      if (has) {
        await member.roles.remove(entry.roleId, 'Self-role (unique toggle)');
        return void interaction.reply({ content: 'Cargo removido.', flags: MessageFlags.Ephemeral });
      }
      await member.roles.add(entry.roleId, 'Self-role (unique)');
      return void interaction.reply({ content: 'Cargo trocado. ✅', flags: MessageFlags.Ephemeral });
    }
    // normal: alterna
    if (has) {
      await member.roles.remove(entry.roleId, 'Self-role (toggle off)');
      await interaction.reply({ content: 'Cargo removido.', flags: MessageFlags.Ephemeral });
    } else {
      await member.roles.add(entry.roleId, 'Self-role (toggle on)');
      await interaction.reply({ content: 'Cargo atribuído. ✅', flags: MessageFlags.Ephemeral });
    }
  } catch (err) {
    log.error('Falha no self-role:', err);
    await interaction.reply({ content: 'Não consegui alterar o cargo (permissões/hierarquia?).', flags: MessageFlags.Ephemeral });
  }
}

export const selfRoleCommands: readonly Command[] = [rolepanel];
