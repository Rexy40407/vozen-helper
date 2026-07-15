import { AuditLogEvent } from 'discord.js';
import { formatDuration } from '../moderation/duration.js';

// Formatadores puros para os embeds de log. Separados dos handlers para serem
// testáveis (recebem dados simples, devolvem texto).

/** Trunca texto para caber num campo de embed. */
export function truncate(text: string, max = 1000): string {
  if (text.length <= max) return text;
  return text.slice(0, max - 1) + '…';
}

/** Idade da conta em texto curto ("3d", "5m") a partir de ms. */
export function ageLabel(ageMs: number): string {
  return formatDuration(ageMs);
}

/** Descreve uma ação do audit log de forma legível (para o canal de mod/server). */
export function describeAuditAction(action: AuditLogEvent): string {
  switch (action) {
    case AuditLogEvent.MemberBanAdd:
      return 'baniu';
    case AuditLogEvent.MemberBanRemove:
      return 'desbaniu';
    case AuditLogEvent.MemberKick:
      return 'expulsou';
    case AuditLogEvent.ChannelCreate:
      return 'criou um canal';
    case AuditLogEvent.ChannelDelete:
      return 'apagou um canal';
    case AuditLogEvent.RoleCreate:
      return 'criou um cargo';
    case AuditLogEvent.RoleDelete:
      return 'apagou um cargo';
    case AuditLogEvent.WebhookCreate:
      return 'criou um webhook';
    case AuditLogEvent.WebhookDelete:
      return 'apagou um webhook';
    case AuditLogEvent.MemberUpdate:
      return 'atualizou um membro';
    default:
      return `ação ${action}`;
  }
}

/** Conjunto de ações do audit log que interessam ao logging. */
export const LOGGED_AUDIT_ACTIONS: readonly AuditLogEvent[] = [
  AuditLogEvent.MemberBanAdd,
  AuditLogEvent.MemberBanRemove,
  AuditLogEvent.MemberKick,
  AuditLogEvent.ChannelCreate,
  AuditLogEvent.ChannelDelete,
  AuditLogEvent.RoleCreate,
  AuditLogEvent.RoleDelete,
  AuditLogEvent.WebhookCreate,
  AuditLogEvent.WebhookDelete,
];

/** Diferença de roles entre dois conjuntos (para o log de member update). */
export function diffRoles(
  before: readonly string[],
  after: readonly string[],
): { added: string[]; removed: string[] } {
  const b = new Set(before);
  const a = new Set(after);
  return {
    added: after.filter((r) => !b.has(r)),
    removed: before.filter((r) => !a.has(r)),
  };
}
