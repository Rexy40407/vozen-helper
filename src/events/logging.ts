import { Events, EmbedBuilder, AuditLogEvent, type Client, type EmbedField } from 'discord.js';
import type { AppContext } from '../context.js';
import type { LogCategory } from '../config.js';
import { pickLogChannel, shouldIgnore } from '../logging/router.js';
import { resolveChannelId } from '../store/channelSettings.js';
import {
  truncate,
  ageLabel,
  describeAuditAction,
  LOGGED_AUDIT_ACTIONS,
  diffRoles,
} from '../logging/format.js';
import { accountAgeMs } from '../moderation/snowflake.js';
import { log } from '../log.js';

// Logging completo. Cada evento relevante é formatado e enviado para o canal da sua
// categoria (config.logging.channels). Ignora canais/utilizadores da ignore list.

async function sendLog(ctx: AppContext, category: LogCategory, embed: EmbedBuilder): Promise<void> {
  // Override do painel (canal único p/ todas as categorias) ou o default por-categoria.
  const channelId =
    resolveChannelId(ctx.db, ctx.env.guildId, 'log', null) ??
    pickLogChannel(ctx.modConfig.logging, category);
  if (!channelId) return;
  const channel = await ctx.client.channels.fetch(channelId).catch(() => null);
  if (channel && channel.isTextBased() && !channel.isDMBased()) {
    await channel
      .send({ embeds: [embed] })
      .catch((err) => log.debug('log falhou:', (err as Error).message));
  }
}

/** Liga todos os handlers de logging ao cliente. */
export function registerLoggingHandlers(client: Client, ctx: AppContext): void {
  const guildId = ctx.env.guildId;
  const cfg = ctx.modConfig.logging;

  // ── Mensagens ──
  client.on(Events.MessageDelete, (message) => {
    if (!message.guildId || message.guildId !== guildId) return;
    if (shouldIgnore(cfg, { channelId: message.channelId, userId: message.author?.id })) return;
    const embed = new EmbedBuilder()
      .setTitle('Message deleted')
      .setColor(0xed4245)
      .setDescription(truncate(message.content || '*(no cached content)*'))
      .addFields(
        {
          name: 'Author',
          value: message.author ? `<@${message.author.id}>` : 'unknown',
          inline: true,
        },
        { name: 'Channel', value: `<#${message.channelId}>`, inline: true },
      );
    void sendLog(ctx, 'messages', embed);
  });

  client.on(Events.MessageUpdate, (oldMsg, newMsg) => {
    if (!newMsg.guildId || newMsg.guildId !== guildId) return;
    if (oldMsg.content === newMsg.content) return;
    if (shouldIgnore(cfg, { channelId: newMsg.channelId, userId: newMsg.author?.id })) return;
    const embed = new EmbedBuilder()
      .setTitle('Message edited')
      .setColor(0xfee75c)
      .addFields(
        { name: 'Before', value: truncate(oldMsg.content || '*(not cached)*') },
        { name: 'After', value: truncate(newMsg.content || '*(empty)*') },
        { name: 'Author', value: newMsg.author ? `<@${newMsg.author.id}>` : '—', inline: true },
        { name: 'Channel', value: `<#${newMsg.channelId}>`, inline: true },
      );
    void sendLog(ctx, 'messages', embed);
  });

  // ── Membros ──
  client.on(Events.GuildMemberAdd, (member) => {
    if (member.guild.id !== guildId) return;
    const age = accountAgeMs(member.id, Date.now());
    const embed = new EmbedBuilder()
      .setTitle('Member joined')
      .setColor(0x57f287)
      .setDescription(`<@${member.id}> (${member.user.tag})`)
      .addFields({ name: 'Account age', value: ageLabel(age), inline: true });
    void sendLog(ctx, 'members', embed);
  });

  client.on(Events.GuildMemberRemove, (member) => {
    if (member.guild.id !== guildId) return;
    const roles = member.roles?.cache
      ? [...member.roles.cache.keys()].filter((r) => r !== guildId).map((r) => `<@&${r}>`)
      : [];
    const embed = new EmbedBuilder()
      .setTitle('Member left')
      .setColor(0xed4245)
      .setDescription(`<@${member.id}> (${member.user.tag})`);
    if (roles.length)
      embed.addFields({ name: 'Roles they had', value: truncate(roles.join(' '), 900) });
    void sendLog(ctx, 'members', embed);
  });

  client.on(Events.GuildMemberUpdate, (oldM, newM) => {
    if (newM.guild.id !== guildId) return;
    const fields: EmbedField[] = [];
    if (oldM.nickname !== newM.nickname) {
      fields.push({
        name: 'Nickname',
        value: `${oldM.nickname ?? '*(none)*'} → ${newM.nickname ?? '*(none)*'}`,
        inline: false,
      });
    }
    const rd = diffRoles([...oldM.roles.cache.keys()], [...newM.roles.cache.keys()]);
    if (rd.added.length)
      fields.push({
        name: 'Roles +',
        value: rd.added.map((r) => `<@&${r}>`).join(' '),
        inline: true,
      });
    if (rd.removed.length)
      fields.push({
        name: 'Roles −',
        value: rd.removed.map((r) => `<@&${r}>`).join(' '),
        inline: true,
      });
    if (!fields.length) return;
    const embed = new EmbedBuilder()
      .setTitle('Member updated')
      .setColor(0x5865f2)
      .setDescription(`<@${newM.id}>`)
      .addFields(fields);
    void sendLog(ctx, 'members', embed);
  });

  // ── Voz ──
  client.on(Events.VoiceStateUpdate, (oldS, newS) => {
    if (newS.guild.id !== guildId) return;
    const uid = newS.id;
    let text: string | null = null;
    if (!oldS.channelId && newS.channelId) text = `joined <#${newS.channelId}>`;
    else if (oldS.channelId && !newS.channelId) text = `left <#${oldS.channelId}>`;
    else if (oldS.channelId !== newS.channelId)
      text = `moved <#${oldS.channelId}> → <#${newS.channelId}>`;
    if (!text) return;
    const embed = new EmbedBuilder()
      .setTitle('Voice')
      .setColor(0x5865f2)
      .setDescription(`<@${uid}> ${text}`);
    void sendLog(ctx, 'voice', embed);
  });

  // ── Audit log em tempo real (executor + ação) → servidor/mod ──
  client.on(Events.GuildAuditLogEntryCreate, async (entry, guild) => {
    if (guild.id !== guildId) return;
    if (!LOGGED_AUDIT_ACTIONS.includes(entry.action)) return;
    const isMod =
      entry.action === AuditLogEvent.MemberBanAdd ||
      entry.action === AuditLogEvent.MemberBanRemove ||
      entry.action === AuditLogEvent.MemberKick;
    const category: LogCategory = isMod ? 'mod' : 'server';
    const embed = new EmbedBuilder()
      .setTitle('Server action')
      .setColor(isMod ? 0xed4245 : 0xfaa61a)
      .setDescription(
        `<@${entry.executorId ?? '0'}> ${describeAuditAction(entry.action)}` +
          (entry.targetId ? ` — target \`${entry.targetId}\`` : ''),
      );
    if (entry.reason) embed.addFields({ name: 'Reason', value: truncate(entry.reason, 500) });
    void sendLog(ctx, category, embed);
  });
}
