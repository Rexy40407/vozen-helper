// Métricas por-mensagem para o anti-spam. Todas puras. Servem de sinais ao sistema
// de "heat" (spamTracker), em vez de regras isoladas com thresholds rígidos.

const RE_CUSTOM_EMOJI = /<a?:\w+:\d+>/g;
const RE_UNICODE_EMOJI = /\p{Extended_Pictographic}/gu;
const RE_INVITE = /(discord\.(gg|io|me|li)|discord(app)?\.com\/invite)\/\S+/i;
const RE_URL = /https?:\/\/\S+/gi;

/** Rácio de maiúsculas entre os caracteres alfabéticos (0..1). 0 se não houver letras. */
export function capsRatio(text: string): number {
  const letters = text.match(/\p{L}/gu) ?? [];
  if (letters.length === 0) return 0;
  const upper = letters.filter((c) => c === c.toUpperCase() && c !== c.toLowerCase()).length;
  return upper / letters.length;
}

/** Conta emojis (unicode + custom do Discord). */
export function countEmojis(text: string): number {
  const custom = text.match(RE_CUSTOM_EMOJI)?.length ?? 0;
  const unicode = text.match(RE_UNICODE_EMOJI)?.length ?? 0;
  return custom + unicode;
}

export function countNewlines(text: string): number {
  return (text.match(/\n/g) ?? []).length;
}

export function countLinks(text: string): number {
  return (text.match(RE_URL) ?? []).length;
}

export function hasInvite(text: string): boolean {
  return RE_INVITE.test(text);
}

/** Hash barato e estável do conteúdo normalizado, para detetar duplicados. */
export function contentKey(text: string): string {
  return text.trim().toLowerCase().replace(/\s+/g, ' ');
}
