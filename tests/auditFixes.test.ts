import { describe, it, expect } from 'vitest';
import { createKeyedLock } from '../src/community/keyedLock.js';

// Correções da auditoria intensiva (2026-07-15). Lógica pura extraída dos handlers.

describe('createKeyedLock — exclusão mútua por chave (fecha a race do starboard)', () => {
  it('só um titular por chave até libertar', () => {
    const lock = createKeyedLock();
    expect(lock.tryAcquire('g:1')).toBe(true); // 1º entra
    expect(lock.tryAcquire('g:1')).toBe(false); // 2º concorrente é barrado
    expect(lock.tryAcquire('g:2')).toBe(true); // chave diferente é independente
    lock.release('g:1');
    expect(lock.tryAcquire('g:1')).toBe(true); // depois de libertar, entra de novo
  });
});
