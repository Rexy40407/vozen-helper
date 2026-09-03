import { SlashCommandBuilder, MessageFlags, type Message } from 'discord.js';
import type { AppContext } from '../context.js';
import type { Command } from '../commands/index.js';
import { setAfk, getAfk, clearAfk } from './store.js';

// AFK (modelo Dyno): /afk marca ausência; ao mencionar um AFK, o bot avisa; a 1ª
// mensagem do próprio remove o AFK (com 3s de tolerância para não se auto-remover).

const afk: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('afk')
    .setDescription('Mark yourself as away (AFK).')
    .addStringOption((o) => o.setName('reason').setDescription('Reason')) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const reason = interaction.options.getString('reason') ?? 'AFK';
    setAfk(ctx.db, interaction.guildId, interaction.user.id, reason, Date.now());
    await interaction.reply({
      content: `Marked you as AFK: ${reason}`,
      flags: MessageFlags.Ephemeral,
    });
  },
};

/** Processa mensagens para o sistema AFK. */
export async function handleAfkMessage(ctx: AppContext, message: Message): Promise<void> {
  if (message.author?.bot || !message.inGuild() || message.guildId !== ctx.env.guildId) return;

  // 1) O próprio autor volta do AFK (com tolerância de 3s desde que o marcou).
  const own = getAfk(ctx.db, message.guildId, message.author.id);
  if (own && Date.now() - own.since > 3000) {
    clearAfk(ctx.db, message.guildId, message.author.id);
    await message
      .reply({
        content: `Welcome back! I've removed your AFK.`,
        allowedMentions: { repliedUser: false },
      })
      .catch(() => undefined);
  }

  // 2) Avisar se mencionou alguém que está AFK.
  const mentioned = [...message.mentions.users.values()].filter((u) => u.id !== message.author.id);
  const notes: string[] = [];
  for (const u of mentioned) {
    const afkState = getAfk(ctx.db, message.guildId, u.id);
    if (afkState) notes.push(`${u.username} is AFK: ${afkState.reason}`);
  }
  if (notes.length) {
    await message
      .reply({ content: notes.join('\n'), allowedMentions: { repliedUser: false } })
      .catch(() => undefined);
  }
}

export const afkCommands: readonly Command[] = [afk];
