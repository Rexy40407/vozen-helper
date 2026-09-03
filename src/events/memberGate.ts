import {
  EmbedBuilder,
  GuildVerificationLevel,
  type GuildMember,
  type PartialGuildMember,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { RaidDetector } from '../moderation/raidDetector.js';
import { evaluateJoin } from '../moderation/joinGate.js';
import { accountAgeMs } from '../moderation/snowflake.js';
import { recordCase } from '../moderation/service.js';
import { saveStickyRoles, getStickyRoles, clearStickyRoles } from '../store/sticky.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';

// Handlers de entrada/saída de membros: join gate, deteção de raid, sticky roles e
// atribuição do cargo de verificado quando o membro passa o screening nativo.

/** Cargos "reponíveis": exclui @everyone e cargos geridos por integrações. */
function stickyRoleIds(member: GuildMember): string[] {
  return [...member.roles.cache.values()]
    .filter((r) => r.id !== member.guild.id && !r.managed)
    .map((r) => r.id);
}

export async function handleMemberJoin(
  ctx: AppContext,
  member: GuildMember,
  raid: RaidDetector,
  now = Date.now(),
): Promise<void> {
  if (member.guild.id !== ctx.env.guildId) return;

  // 1) Sticky roles — repor cargos guardados de uma passagem anterior.
  if (ctx.modConfig.verification.stickyRolesEnabled) {
    const saved = getStickyRoles(ctx.db, member.guild.id, member.id);
    if (saved.length) {
      const valid = saved.filter((r) => member.guild.roles.cache.has(r) && r !== member.guild.id);
      if (valid.length)
        await member.roles.add(valid, 'Sticky roles (rejoin)').catch(() => undefined);
      clearStickyRoles(ctx.db, member.guild.id, member.id);
    }
  }

  // 2) Auto-role: dar o cargo de membro a quem entra. Se a conta ainda está em
  //    screening (pending), espera-se pelo fim (handleMemberVerify trata disso).
  const autoRoleId = ctx.modConfig.verification.autoRoleId;
  if (autoRoleId && !member.pending && member.guild.roles.cache.has(autoRoleId)) {
    await member.roles
      .add(autoRoleId, 'Entry auto-role')
      .catch((err) => log.error('Falha a dar o auto-role:', (err as Error).message));
  }

  // 3) Join gate.
  if (isFeatureEnabled(ctx.db, ctx.env.guildId, 'joingate', ctx.modConfig.joinGate.enabled)) {
    const verdict = evaluateJoin(
      {
        accountAgeMs: accountAgeMs(member.id, now),
        hasAvatar: member.user.avatar !== null,
        username: member.user.username,
      },
      ctx.modConfig.joinGate,
    );
    if (verdict.action !== 'none') {
      try {
        if (verdict.action === 'kick') await member.kick(verdict.reason);
        else await member.ban({ reason: verdict.reason });
        recordCase(ctx, {
          guildId: member.guild.id,
          type: verdict.action === 'kick' ? 'kick' : 'ban',
          targetId: member.id,
          moderatorId: ctx.client.user?.id ?? 'bot',
          reason: `Join gate: ${verdict.reason}`,
          createdAt: now,
        });
        log.info(`Join gate ${verdict.action} ${member.user.tag}: ${verdict.reason}`);
      } catch (err) {
        log.error('Join gate falhou:', err);
      }
    }
  }

  // 4) Anti-raid por taxa de joins.
  if (isFeatureEnabled(ctx.db, ctx.env.guildId, 'raid', ctx.modConfig.raid.enabled)) {
    const r = raid.record(now);
    if (r.justTriggered) await enterRaidMode(ctx, member, r.count);
  }
}

async function enterRaidMode(ctx: AppContext, member: GuildMember, count: number): Promise<void> {
  const guild = member.guild;
  const cfg = ctx.modConfig.raid;
  log.warn(`RAID detetado: ${count} joins na janela. A ativar modo raid.`);

  if (cfg.raiseVerificationLevel !== null) {
    await guild
      .setVerificationLevel(cfg.raiseVerificationLevel as GuildVerificationLevel, 'Raid mode')
      .catch((err) => log.error('Falha a subir verification level:', (err as Error).message));
  }
  if (cfg.pauseInvites) {
    // Incident actions: pausar convites por 24h (máx. permitido).
    const until = new Date(Date.now() + 24 * 60 * 60 * 1000);
    await guild
      .setIncidentActions({ invitesDisabledUntil: until })
      .catch((err) => log.error('Falha a pausar convites:', (err as Error).message));
  }

  const embed = new EmbedBuilder()
    .setTitle('⚠️ Raid mode enabled')
    .setColor(0xed4245)
    .setDescription(`${count} joins in a short window. Invites paused and verification raised.`);
  const channelId = cfg.alertChannelId ?? ctx.modConfig.logging.channels.mod;
  if (channelId) {
    const ch = await ctx.client.channels.fetch(channelId).catch(() => null);
    if (ch && ch.isTextBased() && !ch.isDMBased())
      await ch.send({ embeds: [embed] }).catch(() => undefined);
  }
}

/** Ao sair, guarda os cargos para repor num rejoin (anti-evasão de mute). */
export function handleMemberLeaveSticky(
  ctx: AppContext,
  member: GuildMember | PartialGuildMember,
  now = Date.now(),
): void {
  if (member.guild.id !== ctx.env.guildId) return;
  if (!ctx.modConfig.verification.stickyRolesEnabled) return;
  if (member.partial) return; // sem cache de cargos não há o que guardar
  const roles = stickyRoleIds(member);
  if (roles.length) saveStickyRoles(ctx.db, member.guild.id, member.id, roles, now);
}

/** Quando o membro passa o screening nativo (pending: true→false), dá o cargo de verificado. */
export async function handleMemberVerify(
  ctx: AppContext,
  oldM: GuildMember | PartialGuildMember,
  newM: GuildMember,
): Promise<void> {
  if (newM.guild.id !== ctx.env.guildId) return;
  const wasPending = oldM.partial ? true : oldM.pending;
  if (!(wasPending && !newM.pending)) return; // só na transição fim-do-screening

  // Dar o auto-role e/ou o cargo de verificado quando o screening termina.
  for (const roleId of [
    ctx.modConfig.verification.autoRoleId,
    ctx.modConfig.verification.verifiedRoleId,
  ]) {
    if (roleId && newM.guild.roles.cache.has(roleId) && !newM.roles.cache.has(roleId)) {
      await newM.roles.add(roleId, 'Screening complete').catch(() => undefined);
    }
  }
}
