import {
  SlashCommandBuilder,
  MessageFlags,
  EmbedBuilder,
  type Message,
  type GuildMember,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import type { LevelRole } from '../config.js';
import { addXp, getXp, getRank, getLeaderboard } from './store.js';
import { levelFromXp, levelProgress, rollXp } from './leveling.js';
import { isExemptChannel } from '../moderation/exempt.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';
import { resolveXpExcluded, resolveLevelupChannel } from '../store/channelSettings.js';

// Leveling/XP (modelo MEE6). XP por mensagem com cooldown; level roles; /rank e
// /leaderboard. Cooldown em memória (não na DB).

const cooldowns = new Map<string, number>();

/**
 * Poda entradas de cooldown já expiradas quando o Map cresce demasiado — evita que
 * cresça sem limite num processo 24/7. Pura (recebe o Map e o now).
 */
export function pruneCooldowns(map: Map<string, number>, now: number, cooldownMs: number): void {
  if (map.size <= 10_000) return;
  for (const [k, t] of map) if (now - t >= cooldownMs) map.delete(k);
}

/** Escolhe o cargo (ou cargos) a aplicar num dado nível. */
export function rolesForLevel(level: number, ladder: readonly LevelRole[], stack: boolean): { add: string[]; remove: string[] } {
  const earned = ladder.filter((r) => r.level <= level).sort((a, b) => a.level - b.level);
  if (!earned.length) return { add: [], remove: [] };
  if (stack) return { add: earned.map((r) => r.roleId), remove: [] };
  const top = earned[earned.length - 1];
  return { add: [top.roleId], remove: earned.slice(0, -1).map((r) => r.roleId) };
}

async function applyLevelRoles(ctx: AppContext, member: GuildMember, level: number): Promise<void> {
  const { add, remove } = rolesForLevel(level, ctx.modConfig.community.leveling.levelRoles, ctx.modConfig.community.leveling.stackRoles);
  const botTop = member.guild.members.me?.roles.highest.position ?? 0;
  const guildHas = (id: string) => {
    const role = member.guild.roles.cache.get(id);
    return role && role.position < botTop;
  };
  const toAdd = add.filter((id) => guildHas(id) && !member.roles.cache.has(id));
  const toRemove = remove.filter((id) => guildHas(id) && member.roles.cache.has(id));
  if (toAdd.length) await member.roles.add(toAdd, 'Level role').catch((e) => log.debug('level role add:', (e as Error).message));
  if (toRemove.length) await member.roles.remove(toRemove, 'Level role (replace)').catch(() => undefined);
}

/** Processa uma mensagem para XP. */
export async function handleXpMessage(ctx: AppContext, message: Message, now = Date.now()): Promise<void> {
  const cfg = ctx.modConfig.community.leveling;
  if (
    !isFeatureEnabled(ctx.db, ctx.env.guildId, 'leveling', cfg.enabled) ||
    message.author?.bot ||
    !message.inGuild() ||
    message.guildId !== ctx.env.guildId
  )
    return;
  if (isExemptChannel(message.channelId, resolveXpExcluded(ctx.db, ctx.env.guildId, cfg.noXpChannelIds, now)))
    return;

  const key = `${message.guildId}:${message.author.id}`;
  const last = cooldowns.get(key) ?? 0;
  if (now - last < cfg.cooldownMs) return;
  cooldowns.set(key, now);
  pruneCooldowns(cooldowns, now, cfg.cooldownMs);

  const before = getXp(ctx.db, message.guildId, message.author.id);
  const gained = rollXp(cfg.minXp, cfg.maxXp, Math.random());
  const after = addXp(ctx.db, message.guildId, message.author.id, gained);
  const oldLevel = levelFromXp(before);
  const newLevel = levelFromXp(after);
  if (newLevel > oldLevel) {
    const member = message.member ?? (await message.guild.members.fetch(message.author.id).catch(() => null));
    if (member) await applyLevelRoles(ctx, member, newLevel);
    const announceId = resolveLevelupChannel(ctx.db, ctx.env.guildId, cfg.announceChannelId, now);
    const text = `🎉 <@${message.author.id}> subiu ao **nível ${newLevel}**!`;
    if (announceId) {
      const ch = await message.guild.channels.fetch(announceId).catch(() => null);
      if (ch && ch.isTextBased() && !ch.isDMBased()) await ch.send(text).catch(() => undefined);
    } else {
      await message.channel.send(text).catch(() => undefined);
    }
  }
}

const rank: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('rank')
    .setDescription('Mostra o teu nível e XP.')
    .addUserOption((o) => o.setName('user').setDescription('Outro membro')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser('user') ?? interaction.user;
    const xp = getXp(ctx.db, interaction.guildId, user.id);
    const { level, current, needed } = levelProgress(xp);
    const rankPos = getRank(ctx.db, interaction.guildId, user.id);
    const embed = new EmbedBuilder()
      .setTitle(`Rank de ${user.username}`)
      .setThumbnail(user.displayAvatarURL())
      .setColor(0x5865f2)
      .addFields(
        { name: 'Nível', value: String(level), inline: true },
        { name: 'XP', value: `${current}/${needed}`, inline: true },
        { name: 'Posição', value: rankPos ? `#${rankPos}` : '—', inline: true },
      );
    await interaction.reply({ embeds: [embed] });
  },
};

const leaderboard: Command = {
  public: true,
  data: new SlashCommandBuilder().setName('leaderboard').setDescription('Top 10 de XP do servidor.') as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const top = getLeaderboard(ctx.db, interaction.guildId, 10);
    if (!top.length) return void interaction.reply({ content: 'Ainda não há XP registado.', flags: MessageFlags.Ephemeral });
    const lines = top.map((row, i) => `**${i + 1}.** <@${row.userId}> — nível ${levelFromXp(row.xp)} (${row.xp} XP)`);
    const embed = new EmbedBuilder().setTitle('🏆 Leaderboard').setColor(0xfee75c).setDescription(lines.join('\n'));
    await interaction.reply({ embeds: [embed] });
  },
};

export const levelCommands: readonly Command[] = [rank, leaderboard];
