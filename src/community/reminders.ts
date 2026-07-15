import {
  SlashCommandBuilder,
  MessageFlags,
} from 'discord.js';
import type { Command } from '../commands/index.js';
import { parseDuration, formatDuration } from '../moderation/duration.js';
import { scheduleAction } from '../store/cases.js';

// Lembretes: /remind <tempo> <texto>. Persistido no scheduler partilhado (sobrevive a
// restart). O disparo é tratado pelo runner de comunidade.

const remind: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('remind')
    .setDescription('Cria um lembrete.')
    .addStringOption((o) => o.setName('tempo').setDescription('Ex.: 10m, 2h, 1d').setRequired(true))
    .addStringOption((o) => o.setName('texto').setDescription('O que lembrar').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const ms = parseDuration(interaction.options.getString('tempo', true));
    if (ms === null) {
      return void interaction.reply({ content: 'Tempo inválido. Ex.: `10m`, `2h`, `1d`.', flags: MessageFlags.Ephemeral });
    }
    const text = interaction.options.getString('texto', true);
    scheduleAction(ctx.db, {
      guildId: interaction.guildId,
      type: 'reminder',
      targetId: interaction.user.id,
      executeAt: Date.now() + ms,
      payload: JSON.stringify({ channelId: interaction.channelId, text }),
      caseId: null,
    });
    await interaction.reply({ content: `Ok! Lembro-te daqui a ${formatDuration(ms)}: "${text}"`, flags: MessageFlags.Ephemeral });
  },
};

export const reminderCommands: readonly Command[] = [remind];
