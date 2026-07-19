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
import { applyServerRuleViolation } from '../moderation/policyEnforcement.js';
import { SERVER_RULE_LABELS, type ServerRule } from '../moderation/policy.js';

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
  if (!botMember) return { ok: false, reason: 'I am not a member of this server.' };
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
    .setDescription('Warns a member (counts as a strike for escalation).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member to warn').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('That member is not in this server.'));

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

    await interaction.reply(eph(`Warned ${user.tag} (case #${caseId}).${extra}`));
  },
};

// ─── /violation ──────────────────────────────────────────────────────────────
// Aplica a consequência publicada sem obrigar a staff a memorizar a escada.
const violation: Command = {
  data: new SlashCommandBuilder()
    .setName('violation')
    .setDescription('Applies the correct punishment for a server-rule violation.')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member or user').setRequired(true))
    .addStringOption((o) =>
      o
        .setName('rule')
        .setDescription('Broken server rule')
        .setRequired(true)
        .addChoices(
          ...Object.entries(SERVER_RULE_LABELS).map(([value, name]) => ({ name, value })),
        ),
    )
    .addStringOption((o) =>
      o.setName('details').setDescription('Evidence or extra context').setMaxLength(400),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const rule = interaction.options.getString('rule', true) as ServerRule;
    const details = interaction.options.getString('details') ?? undefined;
    if (!(rule in SERVER_RULE_LABELS)) return void interaction.reply(eph('Unknown server rule.'));

    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (target) {
      const guard = hierarchyGuard(interaction, target);
      if (!guard.ok) return void interaction.reply(eph(guard.reason));
    }

    const result = await applyServerRuleViolation(
      ctx,
      interaction.guild,
      user.id,
      interaction.user.id,
      rule,
      details,
    );
    const message = result.ok
      ? `Applied **${SERVER_RULE_LABELS[rule]}** to ${user.tag}: ${result.summary}.`
      : `Couldn't apply **${SERVER_RULE_LABELS[rule]}** to ${user.tag}: ${result.summary}.`;
    await interaction.reply(eph(message));
  },
};

// ─── /timeout ─────────────────────────────────────────────────────────────────
const timeout: Command = {
  data: new SlashCommandBuilder()
    .setName('timeout')
    .setDescription('Times out a member for a period (max. 28 days).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member').setRequired(true))
    .addStringOption((o) =>
      o.setName('duration').setDescription('E.g.: 10m, 1h, 7d').setRequired(true),
    )
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const durStr = interaction.options.getString('duration', true);
    const durMs = parseDuration(durStr);
    if (durMs === null)
      return void interaction.reply(eph('Invalid duration. E.g.: `10m`, `1h`, `7d`.'));
    if (durMs > MAX_TIMEOUT_MS)
      return void interaction.reply(eph("Discord's maximum timeout is 28 days."));

    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('That member is not in this server.'));
    const guard = hierarchyGuard(interaction, target);
    if (!guard.ok) return void interaction.reply(eph(guard.reason));

    try {
      await target.timeout(durMs, reason);
    } catch (err) {
      log.error('Falha no timeout:', err);
      return void interaction.reply(eph("Couldn't time out (permissions/hierarchy?)."));
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
    await interaction.reply(
      eph(`Timed out ${user.tag} for ${formatDuration(durMs)} (case #${caseId}).`),
    );
  },
};

// ─── /untimeout ───────────────────────────────────────────────────────────────
const untimeout: Command = {
  data: new SlashCommandBuilder()
    .setName('untimeout')
    .setDescription('Removes the timeout from a member.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('That member is not in this server.'));
    try {
      await target.timeout(null, reason);
    } catch (err) {
      log.error('Falha no untimeout:', err);
      return void interaction.reply(eph("Couldn't remove the timeout."));
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
    await interaction.reply(eph(`Timeout removed from ${user.tag}.`));
  },
};

// ─── /kick ────────────────────────────────────────────────────────────────────
const kick: Command = {
  data: new SlashCommandBuilder()
    .setName('kick')
    .setDescription('Kicks a member.')
    .setDefaultMemberPermissions(PermissionFlagsBits.KickMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const target = await interaction.guild.members.fetch(user.id).catch(() => null);
    if (!target) return void interaction.reply(eph('That member is not in this server.'));
    const guard = hierarchyGuard(interaction, target);
    if (!guard.ok) return void interaction.reply(eph(guard.reason));

    await dmPunished(ctx, user, interaction.guild, 'kick', reason);
    try {
      await target.kick(reason);
    } catch (err) {
      log.error('Falha no kick:', err);
      return void interaction.reply(eph("Couldn't kick."));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'kick',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Kicked ${user.tag} (case #${caseId}).`));
  },
};

// ─── /ban ─────────────────────────────────────────────────────────────────────
const ban: Command = {
  data: new SlashCommandBuilder()
    .setName('ban')
    .setDescription('Bans a user (even if not in the server).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('User').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason'))
    .addIntegerOption((o) =>
      o
        .setName('delete_days')
        .setDescription('Days of messages to delete (0-7)')
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
      return void interaction.reply(eph("Couldn't ban."));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'ban',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Banned ${user.tag} (case #${caseId}).`));
  },
};

// ─── /tempban ─────────────────────────────────────────────────────────────────
const tempban: Command = {
  data: new SlashCommandBuilder()
    .setName('tempban')
    .setDescription('Temporarily bans (auto-unban at the end, survives restart).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('User').setRequired(true))
    .addStringOption((o) =>
      o.setName('duration').setDescription('E.g.: 1d, 12h, 1w').setRequired(true),
    )
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const reason = interaction.options.getString('reason') ?? '';
    const durMs = parseDuration(interaction.options.getString('duration', true));
    if (durMs === null)
      return void interaction.reply(eph('Invalid duration. E.g.: `1d`, `12h`, `1w`.'));

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
      return void interaction.reply(eph("Couldn't ban."));
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
    await interaction.reply(
      eph(`Banned ${user.tag} for ${formatDuration(durMs)} (case #${caseId}).`),
    );
  },
};

// ─── /softban ─────────────────────────────────────────────────────────────────
const softban: Command = {
  data: new SlashCommandBuilder()
    .setName('softban')
    .setDescription('Ban + instant unban (deletes messages without a permanent ban).')
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
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
      await interaction.guild.bans.remove(user.id, 'softban (immediate unban)');
    } catch (err) {
      log.error('Falha no softban:', err);
      return void interaction.reply(eph("Couldn't softban."));
    }
    const caseId = recordCase(ctx, {
      guildId: interaction.guildId,
      type: 'softban',
      targetId: user.id,
      moderatorId: interaction.user.id,
      reason,
      createdAt: Date.now(),
    });
    await interaction.reply(eph(`Softbanned ${user.tag} (case #${caseId}).`));
  },
};

// ─── /unban ───────────────────────────────────────────────────────────────────
const unban: Command = {
  data: new SlashCommandBuilder()
    .setName('unban')
    .setDescription("Removes a user's ban (by ID).")
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .addStringOption((o) => o.setName('user_id').setDescription('User ID').setRequired(true))
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const userId = interaction.options.getString('user_id', true).trim();
    const reason = interaction.options.getString('reason') ?? '';
    if (!/^\d{17,20}$/.test(userId)) return void interaction.reply(eph('Invalid ID.'));
    try {
      await interaction.guild.bans.remove(userId, reason);
    } catch (err) {
      log.error('Falha no unban:', err);
      return void interaction.reply(eph("Couldn't unban (maybe not banned)."));
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
    await interaction.reply(eph(`Unbanned \`${userId}\` (case #${caseId}).`));
  },
};

// ─── /purge ───────────────────────────────────────────────────────────────────
const purge: Command = {
  data: new SlashCommandBuilder()
    .setName('purge')
    .setDescription('Deletes the last N messages in the channel (only <14 days).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addIntegerOption((o) =>
      o
        .setName('count')
        .setDescription('How many (1-100)')
        .setRequired(true)
        .setMinValue(1)
        .setMaxValue(100),
    )
    .addUserOption((o) =>
      o.setName('user').setDescription('Only messages from this user'),
    ) as SlashCommandBuilder,
  async execute(interaction) {
    if (!interaction.inCachedGuild()) return;
    const count = interaction.options.getInteger('count', true);
    const onlyUser = interaction.options.getUser('user');
    const channel = interaction.channel;
    if (!channel || channel.type !== ChannelType.GuildText) {
      return void interaction.reply(eph('Only works in text channels.'));
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
      await interaction.editReply(`Deleted ${deleted} message(s).`);
    } catch (err) {
      log.error('Falha no purge:', err);
      await interaction.editReply("Couldn't delete (messages older than 14 days don't count).");
    }
  },
};

// ─── /modlogs ─────────────────────────────────────────────────────────────────
const modlogs: Command = {
  data: new SlashCommandBuilder()
    .setName('modlogs')
    .setDescription("Shows a user's case history.")
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) =>
      o.setName('user').setDescription('User').setRequired(true),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const cases = getCasesForUser(ctx.db, interaction.guildId, user.id);
    const notes = getNotes(ctx.db, interaction.guildId, user.id);
    const embed = new EmbedBuilder()
      .setTitle(`History of ${user.tag}`)
      .setDescription(
        cases.length ? cases.map(formatCaseLine).join('\n').slice(0, 4000) : 'No cases.',
      );
    if (notes.length) {
      embed.addFields({
        name: `Notes (${notes.length})`,
        value: notes
          .map((n) => `• ${n.content} — <@${n.authorId}>`)
          .join('\n')
          .slice(0, 1000),
      });
    }
    await interaction.reply({ embeds: [embed], flags: MessageFlags.Ephemeral });
  },
};

// ─── /note ────────────────────────────────────────────────────────────────────
const note: Command = {
  data: new SlashCommandBuilder()
    .setName('note')
    .setDescription('Adds a staff note about a member (invisible to the member).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addUserOption((o) => o.setName('user').setDescription('Member').setRequired(true))
    .addStringOption((o) =>
      o.setName('content').setDescription('Note').setRequired(true),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user', true);
    const content = interaction.options.getString('content', true);
    addNote(ctx.db, interaction.guildId, user.id, interaction.user.id, content, Date.now());
    await interaction.reply(eph(`Note added about ${user.tag}.`));
  },
};

// ─── /reason ──────────────────────────────────────────────────────────────────
const reasonCmd: Command = {
  data: new SlashCommandBuilder()
    .setName('reason')
    .setDescription('Edits the reason of an existing case.')
    .setDefaultMemberPermissions(PermissionFlagsBits.ModerateMembers)
    .addIntegerOption((o) =>
      o.setName('case').setDescription('Case number').setRequired(true).setMinValue(1),
    )
    .addStringOption((o) =>
      o.setName('reason').setDescription('New reason').setRequired(true),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const caseId = interaction.options.getInteger('case', true);
    const newReason = interaction.options.getString('reason', true);
    const ok = editCaseReason(ctx.db, interaction.guildId, caseId, newReason);
    await interaction.reply(
      eph(ok ? `Case #${caseId} reason updated.` : `Case #${caseId} not found.`),
    );
  },
};

export const modCommands: readonly Command[] = [
  violation,
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
