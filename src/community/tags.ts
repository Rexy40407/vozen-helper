import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
} from 'discord.js';
import type { Command } from '../commands/index.js';
import { setTag, getTag, deleteTag, listTags } from './store.js';

// Tags / custom commands: staff cria respostas prontas (FAQ, regras, links); toda a
// gente as invoca com /tag. Suporta {user} e {server}. Sem TagScript (simples de propósito).

const tag: Command = {
  public: true,
  data: new SlashCommandBuilder()
    .setName('tag')
    .setDescription('Mostra uma tag guardada.')
    .addStringOption((o) => o.setName('nome').setDescription('Nome da tag').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('nome', true);
    const content = getTag(ctx.db, interaction.guildId, name);
    if (!content) {
      return void interaction.reply({ content: `Não existe a tag \`${name}\`.`, flags: MessageFlags.Ephemeral });
    }
    const rendered = content
      .replaceAll('{user}', `<@${interaction.user.id}>`)
      .replaceAll('{server}', interaction.guild.name);
    await interaction.reply(rendered);
  },
};

const tags: Command = {
  public: true,
  data: new SlashCommandBuilder().setName('tags').setDescription('Lista as tags disponíveis.') as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const names = listTags(ctx.db, interaction.guildId);
    await interaction.reply({
      content: names.length ? `Tags: ${names.map((n) => `\`${n}\``).join(', ')}` : 'Ainda não há tags.',
      flags: MessageFlags.Ephemeral,
    });
  },
};

const tagSet: Command = {
  data: new SlashCommandBuilder()
    .setName('tag-set')
    .setDescription('Cria ou atualiza uma tag (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addStringOption((o) => o.setName('nome').setDescription('Nome').setRequired(true))
    .addStringOption((o) => o.setName('conteudo').setDescription('Resposta (aceita {user}/{server})').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('nome', true);
    const content = interaction.options.getString('conteudo', true);
    setTag(ctx.db, interaction.guildId, name, content, interaction.user.id, Date.now());
    await interaction.reply({ content: `Tag \`${name.toLowerCase()}\` guardada.`, flags: MessageFlags.Ephemeral });
  },
};

const tagDelete: Command = {
  data: new SlashCommandBuilder()
    .setName('tag-delete')
    .setDescription('Apaga uma tag (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addStringOption((o) => o.setName('nome').setDescription('Nome').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('nome', true);
    const ok = deleteTag(ctx.db, interaction.guildId, name);
    await interaction.reply({ content: ok ? `Tag \`${name}\` apagada.` : `Tag \`${name}\` não existe.`, flags: MessageFlags.Ephemeral });
  },
};

export const tagCommands: readonly Command[] = [tag, tags, tagSet, tagDelete];
