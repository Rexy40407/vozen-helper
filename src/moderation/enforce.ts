import type { Guild, GuildMember } from 'discord.js';
import type { AppContext } from '../context.js';
import type { EscalationStep } from '../config.js';
import { registerStrikeAndDecide, recordCase } from './service.js';
import { formatDuration, MAX_TIMEOUT_MS } from './duration.js';
import { canBotActOn } from './hierarchy.js';
import { log } from '../log.js';

// Aplicação de escalação a um MEMBRO (não a uma interação). Partilhado pelos comandos
// (ex.: /warn) e pelos gatilhos automáticos (automod, anti-spam) das fases seguintes.

/** Aplica um degrau (timeout/kick/ban) a um membro. Best-effort; devolve resumo. */
export async function applyEscalationStep(
  ctx: AppContext,
  guild: Guild,
  member: GuildMember,
  step: EscalationStep,
): Promise<string> {
  const botMember = guild.members.me;
  if (botMember) {
    const ok = canBotActOn({
      botTopPosition: botMember.roles.highest.position,
      targetTopPosition: member.roles.highest.position,
      targetIsOwner: guild.ownerId === member.id,
      targetIsSelf: member.id === botMember.id,
    });
    if (!ok.ok) return `→ escalação impossível: ${ok.reason}`;
  }

  const reason = `Escalação automática (${step.atStrikes} strikes)`;
  const now = Date.now();
  try {
    if (step.action === 'timeout') {
      const dur = Math.min(step.durationMs ?? MAX_TIMEOUT_MS, MAX_TIMEOUT_MS);
      await member.timeout(dur, reason);
      recordCase(ctx, {
        guildId: guild.id,
        type: 'timeout',
        targetId: member.id,
        moderatorId: ctx.client.user?.id ?? 'bot',
        reason,
        durationMs: dur,
        createdAt: now,
      });
      return `→ timeout ${formatDuration(dur)}`;
    }
    if (step.action === 'kick') {
      await member.kick(reason);
      recordCase(ctx, {
        guildId: guild.id,
        type: 'kick',
        targetId: member.id,
        moderatorId: ctx.client.user?.id ?? 'bot',
        reason,
        createdAt: now,
      });
      return '→ kick';
    }
    await member.ban({ reason });
    recordCase(ctx, {
      guildId: guild.id,
      type: 'ban',
      targetId: member.id,
      moderatorId: ctx.client.user?.id ?? 'bot',
      reason,
      createdAt: now,
    });
    return '→ ban';
  } catch (err) {
    log.error('Falha ao aplicar escalação:', err);
    return '→ (escalação falhou — ver logs)';
  }
}

/**
 * Regista um strike ao membro e, se atingir um degrau, aplica-o. Devolve o resumo da
 * escalação (ou null se ainda não escalou).
 */
export async function escalateMember(
  ctx: AppContext,
  guild: Guild,
  member: GuildMember,
  source: string,
  now = Date.now(),
): Promise<string | null> {
  const step = registerStrikeAndDecide(ctx, guild.id, member.id, now, source);
  if (!step) return null;
  return applyEscalationStep(ctx, guild, member, step);
}
