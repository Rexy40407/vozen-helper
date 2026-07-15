import { normalizeForMatch, squashRepeats } from './normalize.js';

// Filtro de conteúdo PRÓPRIO (pós-publicação), complementar ao AutoMod nativo. Cobre
// o que o nativo não faz: matching sobre texto normalizado (anti-evasão) e anexos
// perigosos. Slurs/keywords "duras" ficam para o AutoMod nativo (bloqueio ANTES de
// publicar). Puro — testável.

/**
 * Procura a primeira palavra banida no texto. Compara em forma normalizada (sem
 * acentos/homóglifos/invisíveis, repetições colapsadas) por SUBSTRING — apanha
 * "n​i​g" e "$cam". Devolve a palavra que casou (a original da lista) ou null.
 */
export function findBannedWord(text: string, bannedWords: readonly string[]): string | null {
  if (bannedWords.length === 0) return null;
  const haystack = squashRepeats(normalizeForMatch(text));
  if (!haystack) return null;
  for (const word of bannedWords) {
    const needle = squashRepeats(normalizeForMatch(word));
    if (needle && haystack.includes(needle)) return word;
  }
  return null;
}

/**
 * Deteta um anexo com extensão perigosa. `extensions` sem ponto e minúsculas
 * (ex.: ['exe','bat','scr']). Devolve o nome do ficheiro problemático ou null.
 */
export function findDangerousAttachment(
  filenames: readonly string[],
  extensions: readonly string[],
): string | null {
  const set = new Set(extensions.map((e) => e.toLowerCase().replace(/^\./, '')));
  for (const name of filenames) {
    const ext = name.split('.').pop()?.toLowerCase() ?? '';
    if (set.has(ext)) return name;
  }
  return null;
}
