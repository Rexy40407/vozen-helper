import type { User, Guild } from 'discord.js';
import type { AppContext } from '../context.js';
import {
  insertCase,
  addInfraction,
  countInfractions,
  type CaseType,
  type NewCase,
} from '../store/cases.js';
import { decideEscalation } from './escalation.js';
import { buildPunishmentDm } from './messages.js';
import type { EscalationStep } from '../config.js';
import { log } from '../log.js';

// Cola entre a DB, as DMs e a escalação. As decisões (decideEscalation) são puras e
// testadas à parte; aqui orquestra-se o efeito.

/** Insere um caso e devolve o nº. */
export function recordCase(ctx: AppContext, c: NewCase): number {
  return insertCase(ctx.db, c);
}

/** Envia a DM de punição, engolindo falhas (DMs fechadas não devem partir a ação). */
export async function dmPunished(
  ctx: AppContext,
  user: User,
  guild: Guild,
  type: CaseType,
  reason: string,
  durationMs?: number | null,
): Promise<void> {
  if (!ctx.modConfig.dmOnPunish) return;
  try {
    await user.send(buildPunishmentDm(type, guild.name, reason, durationMs));
  } catch {
    log.debug(`Não consegui enviar DM a ${user.id} (DMs fechadas?).`);
  }
}

/**
 * Regista um strike e decide se há escalação. Devolve o degrau a aplicar (ou null).
 * `now` injetável para testes. A aplicação do degrau (timeout/kick/ban) fica a cargo
 * de quem chama, que tem acesso ao membro do Discord.
 */
export function registerStrikeAndDecide(
  ctx: AppContext,
  guildId: string,
  targetId: string,
  now: number,
  source = 'manual',
): EscalationStep | null {
  addInfraction(ctx.db, guildId, targetId, now, 1, source);
  const since = now - ctx.modConfig.strikeWindowMs;
  const total = countInfractions(ctx.db, guildId, targetId, since);
  return decideEscalation(total, ctx.modConfig.escalationLadder);
}
