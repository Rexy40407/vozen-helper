import { describe, it, expect } from 'vitest';
import { initDb, runMigrations, MIGRATIONS } from '../src/store/db.js';

describe('initDb / migrações', () => {
  it('cria o esquema e fica na versão mais recente', () => {
    const db = initDb(':memory:');
    const version = db.pragma('user_version', { simple: true }) as number;
    expect(version).toBe(MIGRATIONS.length);

    const tables = db
      .prepare("SELECT name FROM sqlite_master WHERE type='table'")
      .all()
      .map((r) => (r as { name: string }).name);
    expect(tables).toContain('meta');
    db.close();
  });

  it('é idempotente: correr migrações de novo não altera a versão', () => {
    const db = initDb(':memory:');
    const before = db.pragma('user_version', { simple: true }) as number;
    runMigrations(db);
    runMigrations(db);
    const after = db.pragma('user_version', { simple: true }) as number;
    expect(after).toBe(before);
    db.close();
  });

  it('permite escrever e ler na tabela meta', () => {
    const db = initDb(':memory:');
    db.prepare('INSERT INTO meta (key, value) VALUES (?, ?)').run('installed_at', '2026');
    const row = db.prepare('SELECT value FROM meta WHERE key = ?').get('installed_at') as {
      value: string;
    };
    expect(row.value).toBe('2026');
    db.close();
  });
});
