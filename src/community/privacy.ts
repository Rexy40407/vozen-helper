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
    .setName('privacidade')
    .setDescription('Os teus dados e privacidade (RGPD).')
    .addSubcommand((s) =>
      s.setName('dados').setDescription('Recebe por DM tudo o que o bot tem sobre ti.'),
    )
    .addSubcommand((s) =>
      s.setName('apagar').setDescription('Apaga os teus dados voluntários (mantém registos de moderação).'),
    ) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const guildId = ctx.env.guildId;
    const userId = interaction.user.id;
    const sub = interaction.options.getSubcommand();

    if (sub === 'dados') {
      const data = exportUserData(ctx.db, guildId, userId);
      const json = Buffer.from(JSON.stringify(data, null, 2), 'utf8');
      const sent = await interaction.user
        .send({
          content:
            'Aqui estão todos os dados que o Vozen Helper tem associados a ti. ' +
            'Para mais detalhes, vê a política de privacidade do painel.',
          files: [{ attachment: json, name: 'os-meus-dados-vozen.json' }],
        })
        .then(() => true)
        .catch(() => false);

      await interaction.reply({
        content: sent
          ? '📩 Enviei-te os teus dados por mensagem direta.'
          : '⚠️ Não consegui enviar-te DM (tens as DMs fechadas?). Abre as mensagens diretas e tenta outra vez.',
        flags: MessageFlags.Ephemeral,
      });
      return;
    }

    if (sub === 'apagar') {
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
