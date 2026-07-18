import { SlashCommandBuilder, MessageFlags } from 'discord.js';
import type { Command } from '../commands/index.js';
import { exportUserData, deleteUserData, summarizeDeletion } from '../store/gdpr.js';
import { log } from '../log.js';

// RGPD (Fase 4): direitos dos titulares no próprio Discord.
//  - /privacidade dados  → exportação (art. 15.º): DM com um JSON de tudo o que a BD tem.
//  - /privacidade apagar → apagamento (art. 17.º): remove dados voluntários; os registos
//    de moderação são conservados por interesse legítimo, e o utilizador é informado.
// Público: qualquer membro pode exercer os seus direitos em qualquer canal.

const privacidade: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('privacy')
    .setDescription('Your data and privacy (GDPR).')
    .addSubcommand((s) =>
      s.setName('data').setDescription('Get everything the bot has about you via DM.'),
    )
    .addSubcommand((s) =>
      s.setName('erase').setDescription('Erase your voluntary data (keeps moderation records).'),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const guildId = ctx.env.guildId;
    const userId = interaction.user.id;
    const sub = interaction.options.getSubcommand();

    if (sub === 'data') {
      const data = exportUserData(ctx.db, guildId, userId);
      const json = Buffer.from(JSON.stringify(data, null, 2), 'utf8');
      const sent = await interaction.user
        .send({
          content:
            'Here is all the data Vozen Helper has associated with you. ' +
            'For more details, see the panel privacy policy.',
          files: [{ attachment: json, name: 'my-vozen-data.json' }],
        })
        .then(() => true)
        .catch(() => false);

      await interaction.reply({
        content: sent
          ? '📩 I sent your data to your DMs.'
          : "⚠️ I couldn't DM you (are your DMs closed?). Open your direct messages and try again.",
        flags: MessageFlags.Ephemeral,
      });
      return;
    }

    if (sub === 'erase') {
      const res = deleteUserData(ctx.db, guildId, userId, Date.now());
      // Auditoria do pedido (prova de cumprimento). Sem dados sensíveis além do id.
      log.info(
        `[rgpd] apagamento pedido por ${userId} @ ${new Date().toISOString()}: ` +
          `apagou=${JSON.stringify(res.deleted)} manteve=${JSON.stringify(res.kept)}`,
      );
      await interaction.reply({
        content: summarizeDeletion(res),
        flags: MessageFlags.Ephemeral,
      });
      return;
    }
  },
};

export const privacyCommands: readonly Command[] = [privacidade];
