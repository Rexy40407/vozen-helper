// Parser de durações legíveis ("10m", "1h30m", "7d", "2w") para milissegundos.
// Usado por timeout/tempban. Puro e sem dependências.

const UNIT_MS: Record<string, number> = {
  s: 1000,
  m: 60 * 1000,
  h: 60 * 60 * 1000,
  d: 24 * 60 * 60 * 1000,
  w: 7 * 24 * 60 * 60 * 1000,
};

// O timeout nativo do Discord está limitado a 28 dias (ver docs/RESEARCH-DISCORD-API).
export const MAX_TIMEOUT_MS = 28 * UNIT_MS.d;

/**
 * Converte uma string de duração em ms. Aceita vários troços ("1h30m"). Devolve
 * `null` se a string for inválida ou vazia (o chamador decide o erro a mostrar).
 */
export function parseDuration(input: string): number | null {
  const s = input.trim().toLowerCase();
  if (!s) return null;

  const re = /(\d+)\s*([smhdw])/g;
  let total = 0;
  let matched = false;
  let lastIndex = 0;

  for (let m = re.exec(s); m !== null; m = re.exec(s)) {
    // Rejeitar lixo ENTRE os troços (ex.: "1h xyz 2m") — exige que os matches sejam
    // contíguos a menos de espaços.
    const gap = s.slice(lastIndex, m.index).trim();
    if (gap) return null;
    total += Number(m[1]) * UNIT_MS[m[2]];
    lastIndex = m.index + m[0].length;
    matched = true;
  }
  if (!matched) return null;
  const tail = s.slice(lastIndex).trim();
  if (tail) return null;

  return total > 0 ? total : null;
}

/** Formata ms numa string curta legível (aproximada) para DMs/logs. */
export function formatDuration(ms: number): string {
  if (ms <= 0) return '0s';
  const units: Array<[string, number]> = [
    ['w', UNIT_MS.w],
    ['d', UNIT_MS.d],
    ['h', UNIT_MS.h],
    ['m', UNIT_MS.m],
    ['s', UNIT_MS.s],
  ];
  const parts: string[] = [];
  let rest = ms;
  for (const [label, unit] of units) {
    const n = Math.floor(rest / unit);
    if (n > 0) {
      parts.push(`${n}${label}`);
      rest -= n * unit;
    }
  }
  return parts.join('') || '0s';
}
