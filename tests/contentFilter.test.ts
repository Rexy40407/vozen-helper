import { describe, it, expect } from 'vitest';
import { normalizeForMatch, squashRepeats } from '../src/moderation/normalize.js';
import { findBannedWord, findDangerousAttachment } from '../src/moderation/contentFilter.js';
import { planSync, RULE_PREFIX } from '../src/moderation/automodSync.js';

describe('normalizeForMatch', () => {
  it('remove acentos e maiúsculas', () => {
    expect(normalizeForMatch('CÃO')).toBe('cao');
  });
  it('mapeia homóglifos cirílicos e números-letra', () => {
    // "scam" escrito com cirílico 'с' e '0'
    expect(normalizeForMatch('sс0m')).toContain('sc0m'.replace('0', 'o'));
  });
  it('colapsa separadores entre letras', () => {
    expect(normalizeForMatch('s c a m')).toBe('scam');
    expect(normalizeForMatch('s.c.a.m')).toBe('scam');
  });
  it('remove zero-width', () => {
    expect(normalizeForMatch('ni​g')).toBe('nig');
  });
});

describe('squashRepeats', () => {
  it('reduz runs longos', () => {
    expect(squashRepeats('scaaaam')).toBe('scaam');
  });
});

describe('findBannedWord', () => {
  const list = ['scam', 'badword'];
  it('apanha por substring normalizada', () => {
    expect(findBannedWord('this is a $cam link', list)).toBe('scam');
    expect(findBannedWord('s c a m alert', list)).toBe('scam');
  });
  it('não dispara em texto limpo', () => {
    expect(findBannedWord('olá mundo', list)).toBeNull();
  });
  it('lista vazia nunca dispara', () => {
    expect(findBannedWord('scam', [])).toBeNull();
  });
});

describe('findDangerousAttachment', () => {
  const exts = ['exe', 'bat', 'scr'];
  it('apanha extensão perigosa', () => {
    expect(findDangerousAttachment(['foto.png', 'virus.exe'], exts)).toBe('virus.exe');
  });
  it('ignora ficheiros seguros', () => {
    expect(findDangerousAttachment(['a.png', 'b.txt'], exts)).toBeNull();
  });
  it('é case-insensitive', () => {
    expect(findDangerousAttachment(['X.EXE'], exts)).toBe('X.EXE');
  });
});

describe('planSync', () => {
  const desired = [
    { name: `${RULE_PREFIX} keywords`, build: () => ({}) as never },
    { name: `${RULE_PREFIX} presets`, build: () => ({}) as never },
  ];
  it('cria as que faltam e atualiza as nossas já existentes', () => {
    const plan = planSync([`${RULE_PREFIX} keywords`, 'Regra feita à mão'], desired);
    expect(plan.toUpdate).toContain(`${RULE_PREFIX} keywords`);
    expect(plan.toCreate).toContain(`${RULE_PREFIX} presets`);
  });
  it('ignora regras não-nossas no update', () => {
    const plan = planSync(['Regra feita à mão'], desired);
    expect(plan.toCreate).toHaveLength(2);
    expect(plan.toUpdate).toHaveLength(0);
  });
});
