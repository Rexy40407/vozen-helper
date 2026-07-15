import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  EmbedBuilder,
  ChannelType,
  type ChatInputCommandInteraction,
  type GuildMember,
} from 'discord.js';
import type { Command } from './index.js';
import { canBotActOn, canModeratorActOn } from '../moderation/hierarchy.js';
import { parseDuration, formatDuration, MAX_TIMEOUT_MS } from '../moderation/duration.js';
import { recordCase, dmPunished } from '../moderation/service.js';
import { formatCaseLine } from '../moderation/messages.js';
import { escalateMember } from '../moderation/enforce.js';
import {
  getCasesForUser,
  editCaseReason,
  addNote,
  getNotes,
  scheduleAction,
  cancelScheduled,
  type CaseType,
} from '../store/cases.js';
import { log } from '../log.js';

// Comandos de moderação (Fase 2). Cada um: valida hierarquia → executa a ação no
// Discord (com `reason` para o audit log) → regista o caso → DM ao punido.
// A permissão de acesso é declarada em cada comando (setDefaultMemberPermissions);
// o Diogo pode afinar por-comando nas Server Settings.

function eph(content: string) {
  return { content, flags: MessageFlags.Ephemeral } as const;
}

/** Verifica se o bot E o moderador podem agir sobre o alvo. Devolve erro ou ok. */
function hierarchyGuard(
  interaction: ChatInputCommandInteraction<'cached'>,
  target: GuildMember,
): { ok: true } | { ok: false; reason: string } {
  const guild = interaction.guild;
  const botMember = guild.members.me;
  if (!botMember) return { ok: false, reason: 'Não me encontro como membro do servidor.' };
  const mod = interaction.member as GuildMember;

  const botCheck = canBotActOn({
    botTopPosition: botMember.roles.highest.position,
    targetTopPosition: target.roles.highest.position,
    targetIsOwner: guild.ownerId === target.id,
    targetIsSelf: target.id === botMember.id,
  });
  if (!botCheck.ok) return botCheck;

  return canModeratorActOn(
    mod.roles.highest.position,
    target.roles.highest.position,
    guild.ownerId === mod.id,
  );
}

// ─── /warn ──────────────────────────────────────────────────────────────────────
const warn: Command = {
  data: new SlashCommandBuilder()
    .setName('warn')
    .setDescription('Avisa um membro (conta como strike para a escalação).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro a avisar').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('Esse membro não está no servidor.'));

    const guard = hierarchyGuard(interaction, target);
    if (!guard.ok) return void interaction.reply(eph(guard.reason));

    const now = Date.now();
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'warn',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: now,
    });
    await dmPunished(ctx, user, interaction.guild, 'warn', reason);

    const summary = await escalateMember(ctx, interaction.guild, target, 'warn', now);
    const extra = summary ? ` ${summary}.` : '';

    await interaction.reply(eph(`Avisado ${user.tag} (caso #${caseId}).${extra}`));
  },
};

// ─── /timeout ─────────────────────────────────────────────────────────────────
const timeout: Command = {
  data: new SlashCommandBuilder()
    .setName('timeout')
    .setDescription('Silencia um membro por um período (máx. 28 dias).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true))
    .addStringOption((o) =>
      o.setName('duration').setDescription('Ex.: 10m, 1h, 7d').setRequired(true),
    )
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const durStr = interaction.options.getString('duration', true);
    const durMs = parseDuration(durStr);
    if (durMs === null) return void interaction.reply(eph('Duração inválida. Ex.: `10m`, `1h`, `7d`.'));
    if (durMs > MAX_TIMEOUT_MS)
      return void interaction.reply(eph('O timeout máximo do Discord é 28 dias.'));

    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('Esse membro não está no servidor.'));
    const guard = hierarchyGuard(interaction, target);
    if (!guard.ok) return void interaction.reply(eph(guard.reason));

    try {
      await target.timeout(durMs, reason);
    } catch (err) {
      log.error('Falha no timeout:', err);
      return void interaction.reply(eph('Não consegui silenciar (permissões/hierarquia?).'));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'timeout',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      durationMs: durMs,
      createdAt: Date.now(),
    });
    await dmPunished(ctx, user, interaction.guild, 'timeout', reason, durMs);
    await interaction.reply(eph(`Silenciado ${user.tag} por ${formatDuration(durMs)} (caso #${caseId}).`));
  },
};

// ─── /untimeout ───────────────────────────────────────────────────────────────
const untimeout: Command = {
  data: new SlashCommandBuilder()
    .setName('untimeout')
    .setDescription('Remove o silêncio (timeout) de um membro.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('Esse membro não está no servidor.'));
    try {
      await target.timeout(null, reason);
    } catch (err) {
      log.error('Falha no untimeout:', err);
      return void interaction.reply(eph('Não consegui remover o silêncio.'));
    }
    cancelScheduled(ctx.db, interaction.guildId, 'untimeout', user.id);
    recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'untimeout',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Silêncio removido a ${user.tag}.`));
  },
};

// ─── /kick ────────────────────────────────────────────────────────────────────
const kick: Command = {
  data: new SlashCommandBuilder()
    .setName('kick')
    .setDescription('Expulsa um membro.')
    .setDefaultMemberPermissions(PermissionFlagsBits.KickMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('Esse membro não está no servidor.'));
    const guard = hierarchyGuard(interaction, target);
    if (!guard.ok) return void interaction.reply(eph(guard.reason));

    await dmPunished(ctx, user, interaction.guild, 'kick', reason);
    try {
      await target.kick(reason);
    } catch (err) {
      log.error('Falha no kick:', err);
      return void interaction.reply(eph('Não consegui expulsar.'));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'kick',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Expulso ${user.tag} (caso #${caseId}).`));
  },
};

// ─── /ban ─────────────────────────────────────────────────────────────────────
const ban: Command = {
  data: new SlashCommandBuilder()
    .setName('ban')
    .setDescription('Bane um utilizador (mesmo que não esteja no servidor).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('Utilizador').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo'))
    .addIntegerOption((o) =>
      o
        .setName('delete_days')
        .setDescription('Dias de mensagens a apagar (0-7)')
        .setMinValue(0)
        .setMaxValue(7),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const deleteDays = interaction.options.getInteger('delete_days') ?? 0;

    // Se estiver no servidor, valida hierarquia; se não, bane à mesma (ban por ID).
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (target) {
      const guard = hierarchyGuard(interaction, target);
      if (!guard.ok) return void interaction.reply(eph(guard.reason));
    }
    await dmPunished(ctx, user, interaction.guild, 'ban', reason);
    try {
      await interaction.guild.bans.create(user.id, {
        reason,
        deleteMessageSeconds: deleteDays * 24 * 60 * 60,
      });
    } catch (err) {
      log.error('Falha no ban:', err);
      return void interaction.reply(eph('Não consegui banir.'));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'ban',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Banido ${user.tag} (caso #${caseId}).`));
  },
};

// ─── /tempban ─────────────────────────────────────────────────────────────────
const tempban: Command = {
  data: new SlashCommandBuilder()
    .setName('tempban')
    .setDescription('Bane temporariamente (auto-unban no fim, sobrevive a restart).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('Utilizador').setRequired(true))
    .addStringOption((o) =>
      o.setName('duration').setDescription('Ex.: 1d, 12h, 1w').setRequired(true),
    )
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const durMs = parseDuration(interaction.options.getString('duration', true));
    if (durMs === null) return void interaction.reply(eph('Duração inválida. Ex.: `1d`, `12h`, `1w`.'));

    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (target) {
      const guard = hierarchyGuard(interaction, target);
      if (!guard.ok) return void interaction.reply(eph(guard.reason));
    }
    await dmPunished(ctx, user, interaction.guild, 'tempban', reason, durMs);
    try {
      await interaction.guild.bans.create(user.id, { reason });
    } catch (err) {
      log.error('Falha no tempban:', err);
      return void interaction.reply(eph('Não consegui banir.'));
    }
    const now = Date.now();
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'tempban',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      durationMs: durMs,
      createdAt: now,
    });
    scheduleAction(ctx.db, {
      guildId: interaction.guildId,
      type: 'unban',
      targetId: user.id,
      executeAt: now + durMs,
      payload: '',
      caseId,
    });
    await interaction.reply(eph(`Banido ${user.tag} por ${formatDuration(durMs)} (caso #${caseId}).`));
  },
};

// ─── /softban ─────────────────────────────────────────────────────────────────
const softban: Command = {
  data: new SlashCommandBuilder()
    .setName('softban')
    .setDescription('Ban + unban imediato (apaga mensagens sem expulsar em definitivo).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (target) {
      const guard = hierarchyGuard(interaction, target);
      if (!guard.ok) return void interaction.reply(eph(guard.reason));
    }
    await dmPunished(ctx, user, interaction.guild, 'softban', reason);
    try {
      await interaction.guild.bans.create(user.id, {
        reason: `softban: ${reason}`,
        deleteMessageSeconds: 24 * 60 * 60,
      });
      await interaction.guild.bans.remove(user.id, 'softban (unban imediato)');
    } catch (err) {
      log.error('Falha no softban:', err);
      return void interaction.reply(eph('Não consegui fazer softban.'));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'softban',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Softban a ${user.tag} (caso #${caseId}).`));
  },
};

// ─── /unban ───────────────────────────────────────────────────────────────────
const unban: Command = {
  data: new SlashCommandBuilder()
    .setName('unban')
    .setDescription('Remove o banimento de um utilizador (por ID).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addStringOption((o) => o.setName('user_id').setDescription('ID do utilizador').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Motivo')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const userId = interaction.options.getString('user_id', true).trim();
    const reason = interaction.options.getString('reason') ?? '';
    if (!/^\d{17,20}$/.test(userId)) return void interaction.reply(eph('ID inválido.'));
    try {
      await interaction.guild.bans.remove(userId, reason);
    } catch (err) {
      log.error('Falha no unban:', err);
      return void interaction.reply(eph('Não consegui desbanir (talvez não esteja banido).'));
    }
    cancelScheduled(ctx.db, interaction.guildId, 'unban', userId);
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'unban',
      targetId: userId,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Desbanido \`${userId}\` (caso #${caseId}).`));
  },
};

// ─── /purge ───────────────────────────────────────────────────────────────────
const purge: Command = {
  data: new SlashCommandBuilder()
    .setName('purge')
    .setDescription('Apaga as últimas N mensagens do canal (só <14 dias).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addIntegerOption((o) =>
      o.setName('count').setDescription('Quantas (1-100)').setRequired(true).setMinValue(1).setMaxValue(100),
    )
    .addUserOption((o) => o.setName('user').setDescription('Só mensagens deste utilizador')) as SlashCommandBuilder,
  async execute(interaction) {
    if (!interaction.inCachedGuild()) return;
    const count = interaction.options.getInteger('count', true);
    const onlyUser = interaction.options.getUser('user');
    const channel = interaction.channel;
    if (!channel || channel.type !== ChannelType.GuildText) {
      return void interaction.reply(eph('Só funciona em canais de texto.'));
    }
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });
    try {
      let deleted: number;
      if (onlyUser) {
        // Buscar um lote e filtrar pelo autor antes de apagar em bloco.
        const fetched = await channel.messages.fetch({ limit: 100 });
        const mine = fetched.filter((m) => m.author.id === onlyUser.id).first(count);
        const res = await channel.bulkDelete(mine, true);
        deleted = res.size;
      } else {
        const res = await channel.bulkDelete(count, true);
        deleted = res.size;
      }
      await interaction.editReply(`Apagadas ${deleted} mensagem(ns).`);
    } catch (err) {
      log.error('Falha no purge:', err);
      await interaction.editReply('Não consegui apagar (mensagens com mais de 14 dias não contam).');
    }
  },
};

// ─── /modlogs ─────────────────────────────────────────────────────────────────
const modlogs: Command = {
  data: new SlashCommandBuilder()
    .setName('modlogs')
    .setDescription('Mostra o histórico de casos de um utilizador.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Utilizador').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const cases = getCasesForUser(ctx.db, interaction.guildId, user.id);
    const notes = getNotes(ctx.db, interaction.guildId, user.id);
    const embed = new EmbedBuilder()
      .setTitle(`Histórico de ${user.tag}`)
      .setDescription(
        cases.length ? cases.map(formatCaseLine).join('\n').slice(0, 4000) : 'Sem casos.',
      );
    if (notes.length) {
      embed.addFields({
        name: `Notas (${notes.length})`,
        value: notes.map((n) => `• ${n.content} — <@${n.authorId}>`).join('\n').slice(0, 1000),
      });
    }
    await interaction.reply({ embeds: [embed], flags: MessageFlags.Ephemeral });
  },
};

// ─── /note ────────────────────────────────────────────────────────────────────
const note: Command = {
  data: new SlashCommandBuilder()
    .setName('note')
    .setDescription('Adiciona uma nota de staff sobre um membro (invisível ao membro).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Membro').setRequired(true))
    .addStringOption((o) => o.setName('content').setDescription('Nota').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const content = interaction.options.getString('content', true);
    addNote(ctx.db, interaction.guildId, user.id, interaction.user.id, content, Date.now());
    await interaction.reply(eph(`Nota adicionada sobre ${user.tag}.`));
  },
};

// ─── /reason ──────────────────────────────────────────────────────────────────
const reasonCmd: Command = {
  data: new SlashCommandBuilder()
    .setName('reason')
    .setDescription('Edita o motivo de um caso existente.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addIntegerOption((o) => o.setName('case').setDescription('Nº do caso').setRequired(true).setMinValue(1))
    .addStringOption((o) => o.setName('reason').setDescription('Novo motivo').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const caseId = interaction.options.getInteger('case', true);
    const newReason = interaction.options.getString('reason', true);
    const ok = editCaseReason(ctx.db, interaction.guildId, caseId, newReason);
    await interaction.reply(eph(ok ? `Motivo do caso #${caseId} atualizado.` : `Caso #${caseId} não encontrado.`));
  },
};

export const modCommands: readonly Command[] = [
  warn,
  timeout,
  untimeout,
  kick,
  ban,
  tempban,
  softban,
  unban,
  purge,
  modlogs,
  note,
  reasonCmd,
];

// Reexport de tipos úteis a outras fases.
export type { CaseType };
