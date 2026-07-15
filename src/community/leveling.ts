// Matemática de leveling (modelo MEE6). Puro e testável.
//
// XP para subir do nível `lvl` para `lvl+1`: 5·lvl² + 50·lvl + 100.
// O XP total acumulado é a soma desses incrementos.

/** XP necessário para passar do nível `lvl` ao seguinte. */
export function xpForLevelUp(lvl: number): number {
  return 5 * lvl * lvl + 50 * lvl + 100;
}

/** XP total acumulado necessário para ATINGIR o nível `level`. */
export function totalXpForLevel(level: number): number {
  let total = 0;
  for (let i = 0; i < level; i++) total += xpForLevelUp(i);
  return total;
}

/** Nível correspondente a um total de XP. */
export function levelFromXp(xp: number): number {
  let level = 0;
  let remaining = xp;
  while (remaining >= xpForLevelUp(level)) {
    remaining -= xpForLevelUp(level);
    level++;
  }
  return level;
}

/** Progresso dentro do nível atual: {level, current, needed}. */
export function levelProgress(xp: number): { level: number; current: number; needed: number } {
  const level = levelFromXp(xp);
  const current = xp - totalXpForLevel(level);
  const needed = xpForLevelUp(level);
  return { level, current, needed };
}

/** XP aleatório entre min e max (inclusive). `rnd` injetável para testes. */
export function rollXp(min: number, max: number, rnd: number): number {
  return min + Math.floor(rnd * (max - min + 1));
}
