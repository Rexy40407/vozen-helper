import { type ButtonInteraction, type Message } from 'discord.js';
import type { AppContext } from '../context.js';
import { handleSuggestionVote } from './suggestions.js';
import { handleGiveawayButton } from './giveaways.js';
import { handleSelfRoleButton } from './selfroles.js';
import { handleTicketButton, TICKET_OPEN_ID } from './tickets.js';
import { handleAfkMessage } from './afk.js';
import { handleXpMessage } from './levels.js';
import { handleStatsMessage, handleBumpDetection } from './stats.js';
import { scheduleAction } from '../store/cases.js';

// Encaminhamento das interações e mensagens das features de comunidade. Mantém o
// index.ts enxuto; devolve true se tratou o botão.

export async function routeCommunityButton(
  ctx: AppContext,
  interaction: ButtonInteraction,
): Promise<boolean> {
  const id = interaction.customId;
  if (id.startsWith('sug:')) {
    await handleSuggestionVote(ctx, interaction);
    return true;
  }
  if (id.startsWith('gw:enter:')) {
    await handleGiveawayButton(ctx, interaction);
    return true;
  }
  if (id.startsWith('role:')) {
    await handleSelfRoleButton(ctx, interaction);
    return true;
  }
  if (id === TICKET_OPEN_ID || id.startsWith('ticket:')) {
    await handleTicketButton(ctx, interaction);
    return true;
  }
  return false;
}

/** Corre todos os hooks de mensagem de comunidade (AFK, XP, stats, bump). */
export async function handleCommunityMessage(ctx: AppContext, message: Message): Promise<void> {
  handleStatsMessage(ctx, message);
  void handleAfkMessage(ctx, message);
  void handleXpMessage(ctx, message);

  if (handleBumpDetection(ctx, message) && message.inGuild()) {
    scheduleAction(ctx.db, {
      guildId: message.guildId,
      type: 'bump_reminder',
      targetId: message.author?.id ?? '0',
      executeAt: Date.now() + 2 * 60 * 60 * 1000,
      payload: JSON.stringify({
        channelId: ctx.modConfig.community.bumpReminder.channelId ?? message.channelId,
      }),
      caseId: null,
    });
  }
}
