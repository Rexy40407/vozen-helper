import { normalizeForMatch } from './normalize.js';

// Moderação de nicknames: dehoist (nomes com caracteres à frente para saltar para o
// topo da lista) e decancer (nomes ilegíveis). Puro.

// Caracteres usados para "hoisting" (aparecem antes de letras na ordenação).
const HOIST_CHARS = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~！＂＃";

/** Precisa de dehoist? (começa com um caractere de hoisting). */
export function needsDehoist(name: string): boolean {
  const first = name.trimStart()[0];
  return first !== undefined && HOIST_CHARS.includes(first);
}

/**
 * Nome sanitizado: remove caracteres de hoisting iniciais. Se o resultado ficar
 * ilegível (sem letras/números após normalizar), devolve um nome default.
 */
export function sanitizeName(name: string, fallback = 'Membro'): string {
  let out = name;
  while (out.length > 0 && HOIST_CHARS.includes(out[0])) out = out.slice(1);
  out = out.trim();
  if (!out || normalizeForMatch(out).length === 0) return fallback;
  return out.slice(0, 32); // limite de nickname do Discord
}

/** Nome ilegível (sem qualquer letra/número mesmo após normalização). */
export function isUnreadable(name: string): boolean {
  return normalizeForMatch(name).length === 0;
}

/**
 * Deteta impersonation de staff: nome normalizado igual ao de um membro protegido,
 * sem ser a mesma pessoa. `protectedNames` já normalizados ou não — normalizamos aqui.
 */
export function isImpersonating(name: string, protectedNames: readonly string[]): boolean {
  const n = normalizeForMatch(name);
  if (!n) return false;
  return protectedNames.some((p) => normalizeForMatch(p) === n);
}
