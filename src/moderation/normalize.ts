// Normalização de texto para deteção de evasão. Scammers/toxicidade escondem-se com
// acentos, homóglifos (cirílico/grego que parecem latinos), zero-width e "l e t r a s
// espaçadas". Reduzimos tudo a uma forma canónica ANTES de comparar com a blocklist.
//
// Puro e sem dependências.

// Homóglifos comuns → latino. Não exaustivo; cobre os mais usados para disfarçar.
const HOMOGLYPHS: Record<string, string> = {
  а: 'a', // cirílico
  е: 'e',
  о: 'o',
  р: 'p',
  с: 'c',
  х: 'x',
  у: 'y',
  к: 'k',
  м: 'm',
  н: 'h',
  т: 't',
  в: 'b',
  ѕ: 's',
  і: 'i',
  ј: 'j',
  ο: 'o', // grego
  α: 'a',
  ρ: 'p',
  ν: 'v',
  '0': 'o',
  '1': 'i',
  '3': 'e',
  '4': 'a',
  '5': 's',
  '7': 't',
  '@': 'a',
  $: 's',
};

// Zero-width, joiners, variation selectors, combining marks. SEM flag `g` — são
// testadas caráter a caráter (com `g`, o lastIndex partilhado daria resultados errados).
const RE_INVISIBLE = /[​-‍⁠﻿­]/;
const RE_COMBINING = /[̀-ͯ]/;

/**
 * Reduz o texto a uma forma canónica minúscula: NFKD, sem acentos/combinantes, sem
 * invisíveis, homóglifos mapeados, e sem separadores entre letras (espaços,
 * pontuação e repetições que servem para partir palavras).
 */
export function normalizeForMatch(input: string): string {
  const nfkd = input.normalize('NFKD').toLowerCase();
  let out = '';
  for (const ch of nfkd) {
    if (RE_INVISIBLE.test(ch) || RE_COMBINING.test(ch)) continue;
    out += HOMOGLYPHS[ch] ?? ch;
  }
  // Colapsar tudo o que não seja letra/número para nada — apanha "n i g" e "s.c.a.m".
  return out.replace(/[^\p{L}\p{N}]+/gu, '');
}

/**
 * Colapsa runs do mesmo caractere (>2) para 2, para "loooool"/"scaaaam" casarem sem
 * inflacionar a blocklist. Aplica-se DEPOIS de normalizeForMatch quando útil.
 */
export function squashRepeats(input: string): string {
  return input.replace(/(.)\1{2,}/gu, '$1$1');
}
