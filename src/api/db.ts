import Database from 'better-sqlite3';

// Abre a SQLite do bot a partir do processo da API (processo separado do bot). A API
// LÊ casos/stats/flags e ESCREVE apenas na tabela `settings` (toggles do painel). O
// bot continua dono do esquema (migrações) e de todas as outras escritas. WAL +
// `busy_timeout` permitem os dois processos partilharem o ficheiro em segurança.
//
// NÃO corre migrações: a tabela `settings` tem de existir (criada pela migração do
// bot). Se faltar, é sinal de que o bot ainda não foi atualizado — deploy do bot 1º.

/** Abre a base de dados do bot (leitura + escrita de settings). Tem de já existir. */
export function openApiDb(path: string): Database.Database {
  const db = new Database(path, { fileMustExist: true });
  db.pragma('busy_timeout = 5000');
  return db;
}
