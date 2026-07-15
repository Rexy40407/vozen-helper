import type Database from 'better-sqlite3';
import type { Client } from 'discord.js';
import type { EnvConfig, ModConfig } from './config.js';

// Contexto partilhado passado a comandos e handlers de eventos. Evita globais e
// facilita testes (injeta-se uma DB `:memory:` e mocks).

export interface AppContext {
  db: Database.Database;
  env: EnvConfig;
  modConfig: ModConfig;
  client: Client;
}
