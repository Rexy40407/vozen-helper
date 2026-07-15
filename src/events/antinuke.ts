import { AuditLogEvent, type Guild, type GuildAuditLogsEntry } from 'discord.js';
import type { AppContext } from '../context.js';
import type { NukeTracker, NukeActionKind } from '../moderation/nukeTracker.js';
import { quarantineMember, alertOwner } from '../moderation/quarantineService.js';
import { log } from '../log.js';
import { isFeatureEnabled } from '../store/flags.js';

// Anti-nuke: consome o audit log em tempo real, conta ações destrutivas por executor
// e, ao exceder o threshold, coloca o executor em quarentena (ou só alerta, em
// alertOnly). Whitelist e dono nunca disparam.

const ACTION_MAP: Partial<Record<AuditLogEvent, NukeActionKind>> = {
  [AuditLogEvent.ChannelDelete]: 'channelDelete',
  [AuditLogEvent.RoleDelete]: 'roleDelete',
  [AuditLogEvent.MemberBanAdd]: 'ban',
  [AuditLogEvent.MemberKick]: 'kick',
  [AuditLogEvent.WebhookCreate]: 'webhookCreate',
};

/** True se o executor é imune ao anti-nuke (dono, whitelist ou o próprio bot). */
function isImmune(ctx: AppContext, guild: Guild, executorId: string): boolean {
  return (
    executorId === guild.ownerId ||
    executorId === ctx.client.user?.id ||
    ctx.modConfig.whitelistIds.includes(executorId)
  );
}

export async function handleAuditForNuke(
  ctx: AppContext,
  entry: GuildAuditLogsEntry,
  guild: Guild,
  tracker: NukeTracker,
  now = Date.now(),
): Promise<void> {
  if (!isFeatureEnabled(ctx.db, ctx.env.guildId, 'antinuke', ctx.modConfig.antiNuke.enabled)) return;
  if (guild.id !== ctx.env.guildId) return;
  const executorId = entry.executorId;
  if (!executorId || isImmune(ctx, guild, executorId)) return;

  // Bot adicionado por não-autorizado → kick do bot + quarentena de quem o adicionou.
  if (entry.action === AuditLogEvent.BotAdd && ctx.modConfig.antiNuke.quarantineBotAdds) {
    await handleBotAdd(ctx, guild, executorId, entry.targetId ?? null, now);
    return;
  }

  const kind = ACTION_MAP[entry.action];
  if (!kind) return;

  const count = tracker.record(executorId, kind, now);
  const threshold = ctx.modConfig.antiNuke.thresholds[kind];
  if (count < threshold) return;

  const reason = `Anti-nuke: ${count}× ${kind} em ${Math.round(ctx.modConfig.antiNuke.windowMs / 1000)}s`;
  if (ctx.modConfig.antiNuke.alertOnly) {
    await alertOwner(ctx, guild, `⚠️ (alerta) <@${executorId}> — ${reason}. Quarentena NÃO aplicada (alertOnly).`);
    log.warn(reason + ' — alertOnly, sem ação.');
    return;
  }

  tracker.reset(executorId);
  const member = await guild.members.fetch(executorId).catch(() => null);
  if (member) await quarantineMember(ctx, guild, member, reason, now);
  else await alertOwner(ctx, guild, `🚨 <@${executorId}> — ${reason}, mas não o encontrei como membro.`);
}

async function handleBotAdd(
  ctx: AppContext,
  guild: Guild,
  adderId: string,
  botId: string | null,
  now: number,
): Promise<void> {
  log.warn(`Bot adicionado por não-autorizado (${adderId}).`);
  if (botId) {
    const bot = await guild.members.fetch(botId).catch(() => null);
    await bot?.kick('Anti-nuke: bot não autorizado').catch(() => undefined);
  }
  if (!ctx.modConfig.antiNuke.alertOnly) {
    const adder = await guild.members.fetch(adderId).catch(() => null);
    if (adder) await quarantineMember(ctx, guild, adder, 'Adicionou um bot não autorizado', now);
  } else {
    await alertOwner(ctx, guild, `⚠️ (alerta) <@${adderId}> adicionou um bot não autorizado.`);
  }
}
