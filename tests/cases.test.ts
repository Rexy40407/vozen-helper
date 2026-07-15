import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import {
  insertCase,
  getCasesForUser,
  getCase,
  editCaseReason,
  addNote,
  getNotes,
  addInfraction,
  countInfractions,
  scheduleAction,
  getDueActions,
  deleteScheduled,
  cancelScheduled,
} from '../src/store/cases.js';

let db: Database.Database;
const G = '111111111111111111';
const U = '222222222222222222';

beforeEach(() => {
  db = initDb(':memory:');
});

describe('casos', () => {
  it('insere e numera casos sequencialmente', () => {
    const a = insertCase(db, { guildId: G, type: 'warn', targetId: U, moderatorId: 'm', createdAt: 1 });
    const b = insertCase(db, { guildId: G, type: 'ban', targetId: U, moderatorId: 'm', createdAt: 2 });
    expect(b).toBe(a + 1);
    expect(getCase(db, G, a)?.type).toBe('warn');
  });

  it('lista casos do mais recente ao mais antigo', () => {
    insertCase(db, { guildId: G, type: 'warn', targetId: U, moderatorId: 'm', createdAt: 1 });
    insertCase(db, { guildId: G, type: 'kick', targetId: U, moderatorId: 'm', createdAt: 2 });
    const list = getCasesForUser(db, G, U);
    expect(list[0].type).toBe('kick');
    expect(list).toHaveLength(2);
  });

  it('edita a razão e reporta se o caso não existe', () => {
    const id = insertCase(db, { guildId: G, type: 'warn', targetId: U, moderatorId: 'm', reason: 'x', createdAt: 1 });
    expect(editCaseReason(db, G, id, 'novo')).toBe(true);
    expect(getCase(db, G, id)?.reason).toBe('novo');
    expect(editCaseReason(db, G, 9999, 'nada')).toBe(false);
  });
});

describe('notas', () => {
  it('adiciona e lê notas', () => {
    addNote(db, G, U, 'author', 'suspeito', 1);
    const notes = getNotes(db, G, U);
    expect(notes[0].content).toBe('suspeito');
  });
});

describe('strikes', () => {
  it('conta strikes dentro da janela', () => {
    addInfraction(db, G, U, 100);
    addInfraction(db, G, U, 200);
    addInfraction(db, G, U, 50); // fora da janela [since=100]
    expect(countInfractions(db, G, U, 100)).toBe(2);
    expect(countInfractions(db, G, U, 150)).toBe(1);
  });
});

describe('ações agendadas', () => {
  it('devolve só as vencidas e apaga', () => {
    scheduleAction(db, { guildId: G, type: 'unban', targetId: U, executeAt: 100, payload: '', caseId: null });
    scheduleAction(db, { guildId: G, type: 'unban', targetId: 'x', executeAt: 500, payload: '', caseId: null });
    const due = getDueActions(db, 200);
    expect(due).toHaveLength(1);
    expect(due[0].targetId).toBe(U);
    deleteScheduled(db, due[0].id);
    expect(getDueActions(db, 1000)).toHaveLength(1);
  });

  it('cancela agendamentos pendentes de um alvo', () => {
    scheduleAction(db, { guildId: G, type: 'unban', targetId: U, executeAt: 100, payload: '', caseId: null });
    cancelScheduled(db, G, 'unban', U);
    expect(getDueActions(db, 1000)).toHaveLength(0);
  });
});
