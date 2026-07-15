import type { CaseType, ModCase } from '../store/cases.js';
import { formatDuration } from './duration.js';

// Construtores de texto (puros) para DMs e resumos de casos. Separados da camada
// discord.js para serem testáveis.

const ACTION_PT: Record<CaseType, string> = {
  warn: 'avisado(a)',
  timeout: 'silenciado(a) temporariamente',
  untimeout: 'com o silêncio removido',
  kick: 'expulso(a)',
  ban: 'banido(a)',
  tempban: 'banido(a) temporariamente',
  unban: 'com o banimento removido',
  softban: 'expulso(a) (softban — mensagens apagadas)',
  quarantine: 'colocado(a) em quarentena',
  unquarantine: 'com a quarentena removida',
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
  const verb = ACTION_PT[type] ?? 'moderado(a)';
  let msg = `Foste ${verb} em **${guildName}**.`;
  if (durationMs && (type === 'timeout' || type === 'tempban')) {
    msg += `\nDuração: ${formatDuration(durationMs)}.`;
  }
  msg += `\nMotivo: ${reason.trim() || 'sem motivo indicado'}`;
  return msg;
}

/** Linha compacta de um caso para o `/modlogs`. */
export function formatCaseLine(c: ModCase): string {
  const when = `<t:${Math.floor(c.createdAt / 1000)}:R>`;
  const dur = c.durationMs ? ` (${formatDuration(c.durationMs)})` : '';
  const reason = c.reason.trim() || 'sem motivo';
  return `\`#${c.id}\` **${c.type}**${dur} — ${reason} · por <@${c.moderatorId}> ${when}`;
}
