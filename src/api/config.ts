// Configuração da API do painel (processo SEPARADO do bot).
//
// O painel web (site em GitHub Pages) fala com esta API na VPS. Os segredos/IDs
// vivem no ambiente e são validados no arranque — se faltar algo, falha rápido com
// uma mensagem clara. NÃO reutiliza o loadEnv do bot para não obrigar o bot a ter
// estas variáveis (são só da API).

/** Configuração da API já validada. */
export interface ApiConfig {
  /** Application ID Discord esperada no token OAuth do painel. */
  clientId: string;
  /** Única conta Discord autorizada a usar o painel. */
  allowedUserId: string;
  /** ÚNICO servidor cujos dados a API serve (single-guild). */
  guildId: string;
  /** Token do bot — usado para LISTAR canais do servidor (dropdown do painel). */
  botToken: string;
  /** Segredo para assinar o cookie de sessão (>= 32 chars). */
  sessionSecret: string;
  /** Porta local onde a API ouve (só o túnel lhe acede). */
  port: number;
  /** Origin do site autorizado no CORS. */
  allowedOrigin: string;
  /** Caminho da SQLite do bot (partilhada, aberta em leitura). */
  dbPath: string;
}

/** Lançada quando falta/está inválida a configuração da API. */
export class ApiConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ApiConfigError';
  }
}

/**
 * Valida e normaliza o ambiente da API. Função PURA (recebe o `env`, não lê
 * `process` diretamente) — testável sem tocar no ambiente real.
 */
export function loadApiEnv(env: Record<string, string | undefined>): ApiConfig {
  const missing: string[] = [];

  const allowedUserId = (env.PANEL_ALLOWED_USER_ID ?? '').trim();
  const clientId = (env.CLIENT_ID ?? '').trim();
  const sessionSecret = (env.PANEL_SESSION_SECRET ?? '').trim();
  const guildId = (env.GUILD_ID ?? '').trim();
  const botToken = (env.DISCORD_TOKEN ?? '').trim();

  if (!allowedUserId) missing.push('PANEL_ALLOWED_USER_ID');
  if (!clientId) missing.push('CLIENT_ID');
  if (!sessionSecret) missing.push('PANEL_SESSION_SECRET');
  if (!guildId) missing.push('GUILD_ID');
  if (!botToken) missing.push('DISCORD_TOKEN');

  if (missing.length > 0) {
    throw new ApiConfigError(
      `Configuração da API em falta: ${missing.join(', ')}. ` +
        'Preenche estas variáveis no .env (ver .env.example).',
    );
  }

  if (!/^\d{17,20}$/.test(allowedUserId)) {
    throw new ApiConfigError(
      `PANEL_ALLOWED_USER_ID não parece um ID do Discord válido: "${allowedUserId}".`,
    );
  }
  if (!/^\d{17,20}$/.test(clientId)) {
    throw new ApiConfigError(`CLIENT_ID não parece um ID do Discord válido: "${clientId}".`);
  }
  if (!/^\d{17,20}$/.test(guildId)) {
    throw new ApiConfigError(`GUILD_ID não parece um ID do Discord válido: "${guildId}".`);
  }
  if (sessionSecret.length < 32) {
    throw new ApiConfigError('PANEL_SESSION_SECRET deve ter pelo menos 32 caracteres.');
  }

  const port = Number.parseInt((env.PANEL_API_PORT ?? '8788').trim(), 10) || 8788;
  const allowedOrigin = (env.PANEL_ALLOWED_ORIGIN ?? 'https://rexy40407.github.io').trim();
  const dbPath = (env.DB_PATH ?? '').trim() || './vozen-helper.db';

  return { clientId, allowedUserId, guildId, botToken, sessionSecret, port, allowedOrigin, dbPath };
}
