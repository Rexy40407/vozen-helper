import { EmbedBuilder, type Guild, type GuildMember } from 'discord.js';
import type { AppContext } from '../context.js';
import { saveQuarantine, getQuarantine, clearQuarantine } from '../store/quarantine.js';
import { recordCase } from './service.js';
import { log } from '../log.js';

// Serviço de quarentena: remove TODOS os cargos removíveis de uma conta suspeita
// (timeout não funciona em admins — a única forma de "desarmar" um mod comprometido é
// tirar-lhe os cargos) e guarda-os para restauro manual. Reversível de propósito.

/** Cargos que o bot consegue e deve remover (exclui @everiquém e cargos geridos). */
function removableRoles(member: GuildMember): string[] {
  return [...member.roles.cache.values()]
    .filter((r) => r.id !== member.guild.id && !r.managed)
    .map((r) => r.id);
}

/** Coloca um membro em quarentena. Best-effort; devolve true se removeu cargos. */
export async function quarantineMember(
  ctx: AppContext,
  guild: Guild,
  member: GuildMember,
  reason: string,
  now = Date.now(),
): Promise<boolean> {
  const roles = removableRoles(member);
  saveQuarantine(ctx.db, guild.id, member.id, roles, reason, now);
  try {
    if (roles.length) await member.roles.remove(roles, `Quarentena: ${reason}`);
  } catch (err) {
    log.error('Falha a remover cargos na quarentena:', err);
    return false;
  }
  recordCase(ctx, {
    guildId: guild.id,
    type: 'quarantine',
    targetId: member.id,
    moderatorId: ctx.client.user?.id ?? 'bot',
    reason,
    createdAt: now,
  });
  await alertOwner(ctx, guild, `🚨 **Quarentena**: <@${member.id}> — ${reason}`);
  log.warn(`Quarentena aplicada a ${member.user.tag}: ${reason}`);
  return true;
}

/** Restaura os cargos guardados e limpa a quarentena. */
export async function unquarantineMember(
  ctx: AppContext,
  guild: Guild,
  member: GuildMember,
): Promise<boolean> {
  const saved = getQuarantine(ctx.db, guild.id, member.id);
  if (!saved) return false;
  const valid = saved.roleIds.filter((r) => guild.roles.cache.has(r) && r !== guild.id);
  // Só limpar o registo se a reposição teve sucesso — senão os cargos guardados
  // perder-se-iam para sempre (o design é reversível de propósito).
  if (valid.length) {
    try {
      await member.roles.add(valid, 'Fim da quarentena');
    } catch (err) {
      log.error('Falha a repor cargos na unquarantine:', err);
      await alertOwner(ctx, guild, `⚠️ Não consegui repor os cargos de <@${member.id}> — quarentena mantida.`);
      return false;
    }
  }
  clearQuarantine(ctx.db, guild.id, member.id);
  recordCase(ctx, {
    guildId: guild.id,
    type: 'unquarantine',
    targetId: member.id,
    moderatorId: ctx.client.user?.id ?? 'bot',
    reason: 'Restauro manual',
    createdAt: Date.now(),
  });
  return true;
}

/** Alerta urgente ao dono do servidor: DM + canal de mod (o que funcionar). */
export async function alertOwner(ctx: AppContext, guild: Guild, message: string): Promise<void> {
  const embed = new EmbedBuilder().setColor(0xed4245).setDescription(message);
  const channelId = ctx.modConfig.logging.channels.mod;
  if (channelId) {
    const ch = await ctx.client.channels.fetch(channelId).catch(() => null);
    if (ch && ch.isTextBased() && !ch.isDMBased()) await ch.send({ embeds: [embed] }).catch(() => undefined);
  }
  const owner = await guild.fetchOwner().catch(() => null);
  await owner?.send({ embeds: [embed] }).catch(() => undefined);
}
