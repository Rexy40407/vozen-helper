import type { GuildMember, PartialGuildMember } from 'discord.js';
import type { AppContext } from '../context.js';
import { logActivity } from '../store/activity.js';
import { resolveJoinInvite } from '../community/inviteTracker.js';

// Alimenta o registo de atividade (dashboard do painel) a partir dos eventos de membro.
// Join: descobre o convite usado + idade da conta (sinal anti-fake). Leave: há quanto
// tempo era membro + que cargos tinha. Best-effort — nunca deve partir o fluxo do bot.

/** GuildMemberAdd → evento 'join' com atribuição de convite. */
export async function handleActivityJoin(ctx: AppContext, member: GuildMember): Promise<void> {
  if (member.guild.id !== ctx.env.guildId) return;
  const used = await resolveJoinInvite(member.guild);
  logActivity(ctx.db, {
    guildId: ctx.env.guildId,
    type: 'join',
    userId: member.id,
    userTag: member.user.tag,
    detail: {
      inviteCode: used?.code ?? null,
      inviterId: used?.inviterId ?? null,
      // Idade da conta ao entrar: contas muito novas são sinal de raid/fake.
      accountAgeMs: Date.now() - member.user.createdTimestamp,
      bot: member.user.bot,
    },
    createdAt: Date.now(),
  });
}

/** GuildMemberRemove → evento 'leave' com tempo de permanência + cargos. */
export function handleActivityLeave(ctx: AppContext, member: GuildMember | PartialGuildMember): void {
  if (member.guild.id !== ctx.env.guildId) return;
  const joinedTs = member.joinedTimestamp ?? null;
  // Cargos que o membro tinha (exclui o @everyone). Pode estar indisponível se parcial.
  const roles = member.roles?.cache
    ? [...member.roles.cache.values()].filter((r) => r.id !== member.guild.id).map((r) => r.name)
    : [];
  logActivity(ctx.db, {
    guildId: ctx.env.guildId,
    type: 'leave',
    userId: member.id,
    userTag: member.user?.tag ?? null,
    detail: {
      membershipMs: joinedTs ? Date.now() - joinedTs : null,
      roles,
    },
    createdAt: Date.now(),
  });
}
