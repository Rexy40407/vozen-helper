// Utilidades de snowflake do Discord. O ID codifica o timestamp de criação, o que
// permite saber a idade de uma conta SEM chamadas à API — sinal-chave do anti-raid
// (contas acabadas de criar) e do logging (idade da conta no join). Puro.

/** Epoch do Discord (2015-01-01T00:00:00Z) em ms. */
export const DISCORD_EPOCH = 1_420_070_400_000;

/** Timestamp de criação (ms) codificado num snowflake. */
export function snowflakeToTimestamp(id: string): number {
  return Number(BigInt(id) >> 22n) + DISCORD_EPOCH;
}

/** Idade da conta em ms, dado "agora". Nunca negativa. */
export function accountAgeMs(id: string, now: number): number {
  return Math.max(0, now - snowflakeToTimestamp(id));
}
