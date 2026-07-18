import {
  SlashCommandBuilder,
  MessageFlags,
  type ChatInputCommandInteraction,
} from 'discord.js';

// Comando /ping: prova de vida. Responde efémero com a latência do gateway.
// Serve de esqueleto para o padrão de comandos (data + execute) das fases seguintes.

export const data = new SlashCommandBuilder()
  .setName('ping')
  .setDescription('Checks if the bot is alive and shows the latency.');

/** Formata a resposta do /ping. Pura — testável sem uma interação real. */
export function formatPing(wsPingMs: number): string {
  // wsPing pode ser -1 antes do primeiro heartbeat; mostrar "a medir" nesse caso.
  if (wsPingMs < 0) return 'Pong! 🏓 (latency still measuring)';
  return `Pong! 🏓 Gateway latency: ${wsPingMs} ms`;
}

export async function execute(interaction: ChatInputCommandInteraction): Promise<void> {
  await interaction.reply({
    content: formatPing(Math.round(interaction.client.ws.ping)),
    flags: MessageFlags.Ephemeral,
  });
}
