import type { Guild } from 'discord.js';
import type { AppContext } from '../context.js';
import { addInfraction, countInfractionsBySource } from '../store/cases.js';
import { dmPunished, recordCase } from './service.js';
import { escalateMember } from './enforce.js';
import {
  decidePolicyAction,
  SERVER_RULE_LABELS,
  type PolicyAction,
  type ServerRule,
} from './policy.js';

export interface PolicyEnforcementResult {
  ok: boolean;
  action: PolicyAction['action'];
  durationMs?: number;
  summary: string;
}

function sourceFor(rule: ServerRule): string {
  return `policy:${rule}`;
}

function reasonFor(rule: ServerRule, detail?: string): string {
  const base = `Server rule: ${SERVER_RULE_LABELS[rule]}`;
  return detail?.trim() ? `${base} — ${detail.trim()}` : base;
}

/** Ban imediato partilhado pelos presets severos e pelo comando de política. */
export async function applyImmediateBan(
  ctx: AppContext,
  guild: Guild,
  userId: string,
  moderatorId: string,
  reason: string,
  now = Date.now(),
): Promise<PolicyEnforcementResult> {
  const user = await ctx.client.users.fetch(userId).catch(() => null);
  if (user) await dmPunished(ctx, user, guild, 'ban', reason);
  try {
    await guild.bans.create(userId, {
      reason,
      deleteMessageSeconds: 7 * 24 * 60 * 60,
    });
  } catch {
    return { ok: false, action: 'ban', summary: 'ban failed (permissions/hierarchy)' };
  }
  recordCase(ctx, {
    guildId: guild.id,
    type: 'ban',
    targetId: userId,
    moderatorId,
    reason,
    createdAt: now,
  });
  return { ok: true, action: 'ban', summary: 'instant ban' };
}

/** Aplica a consequência exata publicada para uma infração confirmada. */
export async function applyServerRuleViolation(
  ctx: AppContext,
  guild: Guild,
  userId: string,
  moderatorId: string,
  rule: ServerRule,
  detail?: string,
  now = Date.now(),
): Promise<PolicyEnforcementResult> {
  const source = sourceFor(rule);
  const since = rule === 'spam' ? now - ctx.modConfig.spam.repeatWindowMs : 0;
  const priorOffenses = countInfractionsBySource(ctx.db, guild.id, userId, source, since);
  const decision = decidePolicyAction(
    rule,
    priorOffenses,
    ctx.modConfig.spam.timeoutMs,
    ctx.modConfig.spam.maxTimeoutMs,
  );
  const reason = reasonFor(rule, detail);

  if (decision.action === 'ban') {
    const result = await applyImmediateBan(ctx, guild, userId, moderatorId, reason, now);
    if (result.ok && (rule === 'advertising' || rule === 'spam')) {
      addInfraction(ctx.db, guild.id, userId, now, 1, source);
    }
    return result;
  }

  const member = await guild.members.fetch(userId).catch(() => null);
  if (!member) {
    return { ok: false, action: decision.action, summary: 'member is no longer in the server' };
  }
  const user = member.user;

  if (decision.action === 'timeout') {
    try {
      await member.timeout(decision.durationMs, reason);
    } catch {
      return {
        ok: false,
        action: 'timeout',
        durationMs: decision.durationMs,
        summary: 'timeout failed (permissions/hierarchy)',
      };
    }
    addInfraction(ctx.db, guild.id, userId, now, 1, source);
    recordCase(ctx, {
      guildId: guild.id,
      type: 'timeout',
      targetId: userId,
      moderatorId,
      reason,
      durationMs: decision.durationMs,
      createdAt: now,
    });
    await dmPunished(ctx, user, guild, 'timeout', reason, decision.durationMs);
    return {
      ok: true,
      action: 'timeout',
      durationMs: decision.durationMs,
      summary: `timeout ${decision.durationMs}ms`,
    };
  }

  recordCase(ctx, {
    guildId: guild.id,
    type: 'warn',
    targetId: userId,
    moderatorId,
    reason,
    createdAt: now,
  });
  await dmPunished(ctx, user, guild, 'warn', reason);
  const escalation = await escalateMember(ctx, guild, member, source, now);
  return {
    ok: true,
    action: 'strike',
    summary: escalation ? `strike ${escalation}` : 'warn/strike',
  };
}
