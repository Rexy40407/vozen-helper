import type { Client } from 'discord.js';
import type { AppContext } from '../context.js';
import type { ScheduledAction } from '../store/cases.js';
import { endGiveaway } from './giveaways.js';
import { log } from '../log.js';

// Dispatcher das ações agendadas de COMUNIDADE (lembretes, fim de giveaway, remover
// role de aniversário, bump reminder). Partilha a tabela scheduled_actions e o
// ExpiryScheduler com a moderação; o index.ts encaminha por tipo.

export async function runCommunityScheduled(
  ctx: AppContext,
  _client: Client,
  action: ScheduledAction,
): Promise<void> {
  if (action.guildId !== ctx.env.guildId) return;
  switch (action.type) {
    case 'reminder': {
      const { channelId, text } = safeJson(action.payload);
      const channel = channelId ? await ctx.client.channels.fetch(channelId).catch(() => null) : null;
      // O texto vem do utilizador (/remind é público) — só se permite mencionar o
      // próprio autor do lembrete; qualquer @everyone/menção de cargo no texto NÃO
      // notifica (ver buildReminderSend).
      const opts = buildReminderSend(action.targetId, text ?? '');
      if (channel && channel.isTextBased() && !channel.isDMBased()) {
        await channel.send(opts).catch(() => sendDm(ctx, action.targetId, opts.content));
      } else {
        await sendDm(ctx, action.targetId, opts.content);
      }
      break;
    }
    case 'birthday_role_remove': {
      const guild = await ctx.client.guilds.fetch(action.guildId).catch(() => null);
      const member = guild ? await guild.members.fetch(action.targetId).catch(() => null) : null;
      if (member && action.payload) await member.roles.remove(action.payload, 'Fim do aniversário').catch(() => undefined);
      break;
    }
    case 'bump_reminder': {
      const { channelId } = safeJson(action.payload);
      const channel = channelId ? await ctx.client.channels.fetch(channelId).catch(() => null) : null;
      if (channel && channel.isTextBased() && !channel.isDMBased()) {
        await channel.send('🔔 Já passaram 2 horas — podem fazer `/bump` outra vez para subir o servidor no DISBOARD!').catch(() => undefined);
      }
      break;
    }
    case 'giveaway_end': {
      await endGiveaway(ctx, Number(action.payload)).catch((err) => log.error('Falha a terminar giveaway:', err));
      break;
    }
    default:
      break;
  }
}

/**
 * Constrói as opções de envio de um lembrete. `allowedMentions.users` restringe as
 * menções ativas ao próprio destinatário — `@everyone`/menções de cargo dentro do
 * texto do utilizador não notificam. Pura, testável.
 */
export function buildReminderSend(
  targetId: string,
  text: string,
): { content: string; allowedMentions: { users: string[] } } {
  return {
    content: `⏰ <@${targetId}>, lembrete: ${text}`,
    allowedMentions: { users: [targetId] },
  };
}

function safeJson(s: string): { channelId?: string; text?: string } {
  try {
    return JSON.parse(s);
  } catch {
    return {};
  }
}

async function sendDm(ctx: AppContext, userId: string, msg: string): Promise<void> {
  const user = await ctx.client.users.fetch(userId).catch(() => null);
  await user?.send(msg).catch(() => undefined);
}
