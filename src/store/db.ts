import Database from 'better-sqlite3';

// Base de dados do Vozen Helper (better-sqlite3, WAL).
//
// Esquema versionado por MIGRAÇÕES: um array ordenado de passos SQL, cada um
// aplicado uma única vez. A versão atual fica em `PRAGMA user_version` (inteiro que
// o SQLite guarda no próprio ficheiro). No arranque corremos só as migrações com
// índice >= user_version. Idempotente: reabrir a DB não re-aplica nada.

/**
 * Lista ORDENADA de migrações. Cada entrada é o SQL que leva o esquema da versão N
 * para N+1. NUNCA reordenar nem editar migrações já lançadas — só ACRESCENTAR ao
 * fim (senão bases de dados existentes ficam dessincronizadas). As fases seguintes
 * (casos, strikes, sticky roles, ...) acrescentam migrações aqui.
 */
export const MIGRATIONS: readonly string[] = [
  // v0 -> v1: fundação. Tabela mínima de metadados (marca a instalação); as tabelas
  // de moderação entram nas próximas fases como novas migrações.
  `
  CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
  );
  `,
  // v1 -> v2: Fase 2 (ações de moderação + casos).
  //  - cases: histórico numerado (o id AUTOINCREMENT é o nº do caso).
  //  - notes: notas de staff sobre um membro (invisíveis ao membro).
  //  - scheduled_actions: expirações persistentes (tempban/untimeout/unmute-role) que
  //    têm de sobreviver a um restart.
  //  - infractions: strikes que alimentam a escalação (origem manual ou automod).
  `
  CREATE TABLE IF NOT EXISTS cases (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id     TEXT NOT NULL,
    type         TEXT NOT NULL,
    target_id    TEXT NOT NULL,
    moderator_id TEXT NOT NULL,
    reason       TEXT NOT NULL DEFAULT '',
    duration_ms  INTEGER,
    created_at   INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_cases_target ON cases (guild_id, target_id);

  CREATE TABLE IF NOT EXISTS notes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    target_id  TEXT NOT NULL,
    author_id  TEXT NOT NULL,
    content    TEXT NOT NULL,
    created_at INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_notes_target ON notes (guild_id, target_id);

  CREATE TABLE IF NOT EXISTS scheduled_actions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    type       TEXT NOT NULL,
    target_id  TEXT NOT NULL,
    execute_at INTEGER NOT NULL,
    payload    TEXT NOT NULL DEFAULT '',
    case_id    INTEGER
  );
  CREATE INDEX IF NOT EXISTS idx_sched_due ON scheduled_actions (execute_at);

  CREATE TABLE IF NOT EXISTS infractions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    target_id  TEXT NOT NULL,
    weight     INTEGER NOT NULL DEFAULT 1,
    source     TEXT NOT NULL DEFAULT 'manual',
    created_at INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_infractions_target ON infractions (guild_id, target_id);
  `,
  // v2 -> v3: Fase 6. Sticky roles — guarda os cargos de um membro quando sai, para
  // os repor no rejoin (anti-evasão: rejoin não limpa um mute).
  `
  CREATE TABLE IF NOT EXISTS sticky_roles (
    guild_id   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    role_ids   TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
  );
  `,
  // v3 -> v4: Fase 7 (anti-nuke). Quarentena — cargos removidos a uma conta suspeita,
  // guardados para restauro manual (/unquarantine). Separado dos sticky_roles: aqui é
  // uma remoção DELIBERADA e reversível pela staff, não um rejoin.
  `
  CREATE TABLE IF NOT EXISTS quarantine (
    guild_id   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    role_ids   TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
  );
  `,
  // v4 -> v5: features de comunidade (vagas A-D). Tudo tabelas novas.
  `
  -- Sugestões: embed numerado com votos por botão.
  CREATE TABLE IF NOT EXISTS suggestions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    author_id  TEXT NOT NULL,
    content    TEXT NOT NULL,
    message_id TEXT,
    status     TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL
  );
  CREATE TABLE IF NOT EXISTS suggestion_votes (
    suggestion_id INTEGER NOT NULL,
    user_id       TEXT NOT NULL,
    vote          INTEGER NOT NULL,
    PRIMARY KEY (suggestion_id, user_id)
  );

  -- AFK por (guild,user): estado + razão.
  CREATE TABLE IF NOT EXISTS afk (
    guild_id TEXT NOT NULL,
    user_id  TEXT NOT NULL,
    reason   TEXT NOT NULL DEFAULT '',
    since    INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
  );

  -- Tags / custom commands (staff): nome -> resposta.
  CREATE TABLE IF NOT EXISTS tags (
    guild_id   TEXT NOT NULL,
    name       TEXT NOT NULL,
    content    TEXT NOT NULL,
    author_id  TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, name)
  );

  -- Aniversários (só dia/mês, nunca o ano — minimização de dados).
  CREATE TABLE IF NOT EXISTS birthdays (
    guild_id TEXT NOT NULL,
    user_id  TEXT NOT NULL,
    day      INTEGER NOT NULL,
    month    INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
  );

  -- Mapa de self-roles: por mensagem+componente -> role + modo.
  CREATE TABLE IF NOT EXISTS self_roles (
    message_id TEXT NOT NULL,
    custom_id  TEXT NOT NULL,
    role_id    TEXT NOT NULL,
    mode       TEXT NOT NULL DEFAULT 'normal',
    PRIMARY KEY (message_id, custom_id)
  );

  -- Giveaways.
  CREATE TABLE IF NOT EXISTS giveaways (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id         TEXT NOT NULL,
    channel_id       TEXT NOT NULL,
    message_id       TEXT,
    prize            TEXT NOT NULL,
    winners          INTEGER NOT NULL DEFAULT 1,
    end_at           INTEGER NOT NULL,
    ended            INTEGER NOT NULL DEFAULT 0,
    required_role_id TEXT,
    host_id          TEXT NOT NULL,
    created_at       INTEGER NOT NULL
  );
  CREATE TABLE IF NOT EXISTS giveaway_entries (
    giveaway_id INTEGER NOT NULL,
    user_id     TEXT NOT NULL,
    PRIMARY KEY (giveaway_id, user_id)
  );

  -- Leveling: XP acumulado por (guild,user). O nível deriva do XP.
  CREATE TABLE IF NOT EXISTS levels (
    guild_id TEXT NOT NULL,
    user_id  TEXT NOT NULL,
    xp       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, user_id)
  );
  CREATE INDEX IF NOT EXISTS idx_levels_xp ON levels (guild_id, xp DESC);

  -- Tickets (threads privadas).
  CREATE TABLE IF NOT EXISTS tickets (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    opener_id  TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'open',
    claimed_by TEXT,
    created_at INTEGER NOT NULL
  );

  -- Starboard: mapa mensagem original -> entrada no starboard + contagem.
  CREATE TABLE IF NOT EXISTS starboard (
    guild_id            TEXT NOT NULL,
    original_message_id TEXT NOT NULL,
    starboard_message_id TEXT NOT NULL,
    star_count          INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, original_message_id)
  );

  -- Estatísticas diárias por guild (para /serverstats).
  CREATE TABLE IF NOT EXISTS stats (
    guild_id TEXT NOT NULL,
    date     TEXT NOT NULL,
    messages INTEGER NOT NULL DEFAULT 0,
    joins    INTEGER NOT NULL DEFAULT 0,
    leaves   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, date)
  );
  `,
  // v5 -> v6: settings de runtime (overrides ao modConfig, editáveis pelo painel).
  // `value` é texto (JSON/'true'/'false'); a chave é livre mas o painel só escreve
  // as da allowlist (flag.*). Sobrepõe-se aos defaults de config-as-code.
  `
  CREATE TABLE IF NOT EXISTS settings (
    guild_id   TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, key)
  );
  `,
  // v6 -> v7: registo de atividade (dashboard do painel, estilo audit-log mas mais rico).
  //  - type: 'join' | 'leave' | 'ban' | 'unban' | 'kick' | ... (extensível)
  //  - user_id/user_tag: o membro-alvo (guardamos o tag em snapshot: quem sai deixa de ser
  //    resolúvel só pelo id).
  //  - actor_id: quem executou (moderador), quando aplicável.
  //  - detail: JSON com o extra por tipo (join: inviteCode/inviterId/inviterTag/accountAgeMs;
  //    leave: membershipMs/roles; etc.).
  `
  CREATE TABLE IF NOT EXISTS activity_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   TEXT NOT NULL,
    type       TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    user_tag   TEXT,
    actor_id   TEXT,
    detail     TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_activity_time ON activity_log (guild_id, created_at DESC);
  CREATE INDEX IF NOT EXISTS idx_activity_user ON activity_log (guild_id, user_id);
  `,
];

/**
 * Abre a DB e aplica as migrações em falta. Devolve o handle aberto.
 *
 * @param path caminho do ficheiro SQLite (`:memory:` nos testes)
 */
export function initDb(path: string): Database.Database {
  let db: Database.Database | undefined;
  try {
    db = new Database(path);
    db.pragma('journal_mode = WAL');
    db.pragma('foreign_keys = ON');

    runMigrations(db);

    return db;
  } catch (err) {
    // Fechar o handle se o abrimos antes de a migração falhar (não deixar o ficheiro
    // bloqueado no Windows nem o handle pendurado).
    try {
      db?.close();
    } catch {
      // ignorar
    }
    throw new Error(`Falha ao abrir a base de dados em ${path}: ${(err as Error).message}`, {
      cause: err,
    });
  }
}

/**
 * Aplica as migrações a partir da versão atual (`user_version`). Cada migração corre
 * dentro de uma transação e só depois se avança a versão — se uma falhar, a DB fica
 * na última versão consistente.
 */
export function runMigrations(db: Database.Database): void {
  const current = db.pragma('user_version', { simple: true }) as number;

  for (let version = current; version < MIGRATIONS.length; version++) {
    const sql = MIGRATIONS[version];
    const apply = db.transaction(() => {
      db.exec(sql);
      // user_version não aceita bind de parâmetros — o valor vem do índice do array
      // (inteiro controlado por nós, nunca input externo), por isso é seguro.
      db.pragma(`user_version = ${version + 1}`);
    });
    apply();
  }
}
