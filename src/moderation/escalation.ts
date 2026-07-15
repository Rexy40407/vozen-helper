import type { EscalationStep } from '../config.js';

// Decide a punição automática a partir do nº de strikes acumulados na janela.
// Puro — a leitura dos strikes (DB) e a aplicação da punição ficam fora.

/**
 * Escolhe o degrau MAIS ALTO cujo `atStrikes` foi atingido. A escada não precisa de
 * vir ordenada — ordenamos aqui por segurança.
 *
 * @param strikeCount total de strikes na janela (inclui o que acabou de ser dado)
 * @param ladder      escada de escalação (config)
 * @returns o degrau a aplicar, ou `null` se ainda não se atingiu nenhum
 */
export function decideEscalation(
  strikeCount: number,
  ladder: readonly EscalationStep[],
): EscalationStep | null {
  const sorted = [...ladder].sort((a, b) => a.atStrikes - b.atStrikes);
  let chosen: EscalationStep | null = null;
  for (const step of sorted) {
    if (strikeCount >= step.atStrikes) chosen = step;
  }
  return chosen;
}
