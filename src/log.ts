import { LOG_LEVELS, type LogLevel } from './config.js';

// Logger mínimo com filtro por nível. Escreve para a consola com timestamp ISO.
// Sem dependências — chega para um bot self-hosted (os logs vão para o supervisor).

const RANK: Record<LogLevel, number> = { debug: 0, info: 1, warn: 2, error: 3 };

let threshold: LogLevel = 'info';

/** Define o nível mínimo a partir do qual as mensagens são escritas. */
export function setLogLevel(level: LogLevel): void {
  threshold = level;
}

function emit(level: LogLevel, args: unknown[]): void {
  if (RANK[level] < RANK[threshold]) return;
  const line = `[${new Date().toISOString()}] [${level.toUpperCase()}]`;
  const sink = level === 'error' ? console.error : level === 'warn' ? console.warn : console.log;
  sink(line, ...args);
}

export const log = {
  debug: (...a: unknown[]) => emit('debug', a),
  info: (...a: unknown[]) => emit('info', a),
  warn: (...a: unknown[]) => emit('warn', a),
  error: (...a: unknown[]) => emit('error', a),
} as const;

// Reexport para quem precise da lista.
export { LOG_LEVELS };
