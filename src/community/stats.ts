import {
  SlashCommandBuilder,
  MessageFlags,
  EmbedBuilder,
  type Message,
} from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import { incrStat, getStatsTotals, type StatField } from './store.js';
import { isFeatureEnabled } from '../store/flags.js';

// Estatísticas básicas do servidor + bump reminder do DISBOARD.

export const DISBOARD_ID = '302050872383242240';

/** Chave de data local YYYY-MM-DD. */
export function dateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Incrementa um contador do dia. */
export function bumpStat(ctx: AppContext, field: StatField, now = new Date()): void {
  incrStat(ctx.db, ctx.env.guildId, dateKey(now), field);
}

/** Conta mensagens (chamado no MessageCreate). */
export function handleStatsMessage(ctx: AppContext, message: Message): void {
  if (message.author?.bot || !message.inGuild() || message.guildId !== ctx.env.guildId) return;
  bumpStat(ctx, 'messages');
}

/** Deteta o /bump do DISBOARD e agenda um lembrete 2h depois. */
export function handleBumpDetection(ctx: AppContext, message: Message): boolean {
  if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'bumpreminder', ctx.modConfig.community.bumpReminder.enabled))
    return false;
  if (message.author?.id !== DISBOARD_ID || !message.inGuild()) return false;
  const desc = message.embeds[0]?.description ?? '';
  if (!/bump done|bump feito|👍/i.test(desc)) return false;
  return true;
}

const serverstats: Command = {
  public: true,
  data: new SlashCommandBuilder().setName('serverstats').setDescription('Estatísticas básicas do servidor.') as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const totals = getStatsTotals(ctx.db, interaction.guildId);
    const embed = new EmbedBuilder()
      .setTitle(`Estatísticas de ${interaction.guild.name}`)
      .setColor(0x5865f2)
      .addFields(
        { name: 'Membros', value: String(interaction.guild.memberCount), inline: true },
        { name: 'Mensagens (registadas)', value: String(totals.messages), inline: true },
        { name: 'Entradas', value: String(totals.joins), inline: true },
        { name: 'Saídas', value: String(totals.leaves), inline: true },
      );
    await interaction.reply({ embeds: [embed], flags: MessageFlags.Ephemeral });
  },
};

export const statsCommands: readonly Command[] = [serverstats];
