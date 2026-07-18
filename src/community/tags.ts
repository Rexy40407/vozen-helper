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
    .setDescription('Show a saved tag.')
    .addStringOption((o) => o.setName('name').setDescription('Tag name').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('name', true);
    const content = getTag(ctx.db, interaction.guildId, name);
    if (!content) {
      return void interaction.reply({ content: `The tag \`${name}\` doesn't exist.`, flags: MessageFlags.Ephemeral });
    }
    const rendered = content
      .replaceAll('{user}', `<@${interaction.user.id}>`)
      .replaceAll('{server}', interaction.guild.name);
    await interaction.reply(rendered);
  },
};

const tags: Command = {
  public: true,
  data: new SlashCommandBuilder().setName('tags').setDescription('List available tags.') as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const names = listTags(ctx.db, interaction.guildId);
    await interaction.reply({
      content: names.length ? `Tags: ${names.map((n) => `\`${n}\``).join(', ')}` : 'No tags yet.',
      flags: MessageFlags.Ephemeral,
    });
  },
};

const tagSet: Command = {
  data: new SlashCommandBuilder()
    .setName('tag-set')
    .setDescription('Create or update a tag (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addStringOption((o) => o.setName('name').setDescription('Name').setRequired(true))
    .addStringOption((o) => o.setName('content').setDescription('Response (accepts {user}/{server})').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('name', true);
    const content = interaction.options.getString('content', true);
    setTag(ctx.db, interaction.guildId, name, content, interaction.user.id, Date.now());
    await interaction.reply({ content: `Tag \`${name.toLowerCase()}\` saved.`, flags: MessageFlags.Ephemeral });
  },
};

const tagDelete: Command = {
  data: new SlashCommandBuilder()
    .setName('tag-delete')
    .setDescription('Delete a tag (staff).')
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages)
    .addStringOption((o) => o.setName('name').setDescription('Name').setRequired(true)) as SlashCommandBuilder,
  async execute(interaction, ctx) {
    if (!interaction.inCachedGuild()) return;
    const name = interaction.options.getString('name', true);
    const ok = deleteTag(ctx.db, interaction.guildId, name);
    await interaction.reply({ content: ok ? `Tag \`${name}\` deleted.` : `Tag \`${name}\` doesn't exist.`, flags: MessageFlags.Ephemeral });
  },
};

export const tagCommands: readonly Command[] = [tag, tags, tagSet, tagDelete];
