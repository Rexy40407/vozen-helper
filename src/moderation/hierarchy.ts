// Verificações de hierarquia ANTES de agir sobre um membro. Puro (recebe posições e
// flags) — evita queimar pedidos em 403 (que contam para o limite Cloudflare) e
// impede um mod de atingir alguém igual/acima.

export interface TargetCheckInput {
  /** Posição do role mais alto do BOT. */
  botTopPosition: number;
  /** Posição do role mais alto do ALVO. */
  targetTopPosition: number;
  /** O alvo é o dono do servidor? */
  targetIsOwner: boolean;
  /** O alvo é o próprio bot? */
  targetIsSelf?: boolean;
}

export type CheckResult = { ok: true } | { ok: false; reason: string };

/**
 * O bot pode agir (kick/ban/timeout) sobre o alvo? Regras:
 *  - nunca sobre o dono do servidor;
 *  - nunca sobre o próprio bot;
 *  - só se o role do bot estiver ESTRITAMENTE acima do role do alvo.
 */
export function canBotActOn(input: TargetCheckInput): CheckResult {
  if (input.targetIsSelf) return { ok: false, reason: 'I cannot act on myself.' };
  if (input.targetIsOwner) return { ok: false, reason: 'I cannot act on the server owner.' };
  if (input.botTopPosition <= input.targetTopPosition) {
    return {
      ok: false,
      reason: "The target's role is equal to or higher than mine — give me a higher role.",
    };
  }
  return { ok: true };
}

/**
 * Um moderador pode agir sobre o alvo? (Além do bot poder.) Um mod não age sobre
 * alguém com role igual/superior ao seu, exceto se for o próprio dono a mandar.
 */
export function canModeratorActOn(
  modTopPosition: number,
  targetTopPosition: number,
  modIsOwner: boolean,
): CheckResult {
  if (modIsOwner) return { ok: true };
  if (modTopPosition <= targetTopPosition) {
    return {
      ok: false,
      reason: 'You cannot moderate someone with a role equal to or higher than yours.',
    };
  }
  return { ok: true };
}
