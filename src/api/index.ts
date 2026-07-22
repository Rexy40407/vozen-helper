import 'dotenv/config';
import { loadApiEnv } from './config.js';
import { openApiDb } from './db.js';
import { buildServer } from './server.js';

// Ponto de entrada da API do painel. Processo SEPARADO do bot (o bot corre em
// scripts/start-prod.mjs). Ouve só em 127.0.0.1 — a exposição HTTPS é feita pelo
// túnel (cloudflared), como no spike da Fase 0.

async function main(): Promise<void> {
  const cfg = loadApiEnv(process.env);
  const db = openApiDb(cfg.dbPath);
  const app = buildServer({
    clientId: cfg.clientId,
    allowedUserId: cfg.allowedUserId,
    guildId: cfg.guildId,
    botToken: cfg.botToken,
    sessionSecret: cfg.sessionSecret,
    allowedOrigin: cfg.allowedOrigin,
    db,
  });

  const addr = await app.listen({ port: cfg.port, host: '127.0.0.1' });
  console.log(`[api] a ouvir em ${addr} (origin autorizado: ${cfg.allowedOrigin})`);
}

main().catch((err: unknown) => {
  console.error('[api] falha ao arrancar:', err);
  process.exit(1);
});
