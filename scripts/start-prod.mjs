// Supervisor de produção do Vozen Helper. Espelha o padrão do Vozen:
//  - lock de instância única (evita dois bots com o mesmo token → sessão duplicada),
//  - auto-restart com backoff exponencial em caso de crash,
//  - logs persistentes com timestamp.
//
// Uso: `npm run start:prod` (que faz build + corre isto). NUNCA `npm start` cru em
// produção — salta o supervisor.

import { spawn } from 'node:child_process';
import { existsSync, writeFileSync, unlinkSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const lockFile = join(root, '.vozen-helper.lock');
const entry = join(root, 'dist', 'index.js');

// ── Lock de instância única ──
if (existsSync(lockFile)) {
  const pid = Number(readFileSync(lockFile, 'utf8').trim());
  let alive = false;
  try {
    process.kill(pid, 0); // sinal 0 = só testa se o processo existe
    alive = true;
  } catch {
    // processo não existe → lock órfão (alive fica false)
  }
  if (alive) {
    console.error(`[supervisor] Já corre uma instância (pid ${pid}). A sair.`);
    process.exit(1);
  }
  console.warn('[supervisor] Lock órfão encontrado — a substituir.');
}
writeFileSync(lockFile, String(process.pid));

function cleanup() {
  try {
    unlinkSync(lockFile);
  } catch {
    // ignorar
  }
}
process.on('exit', cleanup);
process.on('SIGINT', () => process.exit(0));
process.on('SIGTERM', () => process.exit(0));

// ── Auto-restart com backoff ──
let backoff = 1000;
const MAX_BACKOFF = 60_000;

function start() {
  const child = spawn(process.execPath, [entry], { stdio: 'inherit', env: process.env });
  child.on('exit', (code, signal) => {
    if (signal === 'SIGINT' || signal === 'SIGTERM') {
      console.log('[supervisor] Encerramento pedido. A sair.');
      process.exit(0);
    }
    console.error(
      `[supervisor] Bot terminou (code=${code}, signal=${signal}). Reinício em ${backoff}ms.`,
    );
    setTimeout(() => {
      backoff = Math.min(backoff * 2, MAX_BACKOFF);
      start();
    }, backoff);
  });
  // Reset do backoff se sobreviver >1 min (arranque saudável).
  setTimeout(() => {
    backoff = 1000;
  }, 60_000);
}

start();
