import type { CaseType, ModCase } from '../store/cases.js';
import { formatDuration } from './duration.js';

// Construtores de texto (puros) para DMs e resumos de casos. Separados da camada
// discord.js para serem testáveis.

const ACTION_PT: Record<CaseType, string> = {
  warn: 'warned',
  timeout: 'timed out',
  untimeout: 'no longer timed out',
  kick: 'kicked',
  ban: 'banned',
  tempban: 'temporarily banned',
  unban: 'no longer banned',
  softban: 'kicked (softban — messages deleted)',
  quarantine: 'quarantined',
  unquarantine: 'released from quarantine',
};

/**
 * Mensagem de DM a enviar ao membro punido. `guildName` humaniza; `durationMs`
 * acrescenta a duração quando faz sentido (timeout/tempban).
 */
export function buildPunishmentDm(
  type: CaseType,
  guildName: string,
  reason: string,
  durationMs?: number | null,
): string {
  const verb = ACTION_PT[type] ?? 'moderated';
  let msg = `You were ${verb} in **${guildName}**.`;
  if (durationMs && (type === 'timeout' || type === 'tempban')) {
    msg += `\nDuration: ${formatDuration(durationMs)}.`;
  }
  msg += `\nReason: ${reason.trim() || 'no reason given'}`;
  return msg;
}

/** Linha compacta de um caso para o `/modlogs`. */
export function formatCaseLine(c: ModCase): string {
  const when = `<t:${Math.floor(c.createdAt / 1000)}:R>`;
  const dur = c.durationMs ? ` (${formatDuration(c.durationMs)})` : '';
  const reason = c.reason.trim() || 'no reason';
  return `\`#${c.id}\` **${c.type}**${dur} — ${reason} · by <@${c.moderatorId}> ${when}`;
}
