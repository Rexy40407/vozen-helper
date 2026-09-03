import { describe, it, expect } from 'vitest';
import {
  extractDomains,
  isBlacklistedDomain,
  isLookalikeDomain,
  levenshtein,
  scanForScam,
} from '../src/moderation/phishing.js';
import {
  needsDehoist,
  sanitizeName,
  isUnreadable,
  isImpersonating,
} from '../src/moderation/nickname.js';

const PROTECTED = ['discord.com', 'discord.gg', 'discord.gift'];

describe('phishing', () => {
  it('extrai domínios', () => {
    expect(extractDomains('vê https://www.Example.com/x e http://y.org')).toEqual([
      'example.com',
      'y.org',
    ]);
  });
  it('blacklist casa domínio e subdomínio', () => {
    expect(isBlacklistedDomain('bad.com', ['bad.com'])).toBe(true);
    expect(isBlacklistedDomain('a.bad.com', ['bad.com'])).toBe(true);
    expect(isBlacklistedDomain('good.com', ['bad.com'])).toBe(false);
  });
  it('levenshtein', () => {
    expect(levenshtein('discord', 'dlscord')).toBe(1);
    expect(levenshtein('abc', 'abc')).toBe(0);
  });
  it('deteta lookalike mas não o legítimo', () => {
    expect(isLookalikeDomain('dlscord.gift', PROTECTED)).toBe(true);
    expect(isLookalikeDomain('discorcl.com', PROTECTED)).toBe(true);
    expect(isLookalikeDomain('discord.com', PROTECTED)).toBe(false);
    expect(isLookalikeDomain('github.com', PROTECTED)).toBe(false);
  });
  it('scanForScam devolve o motivo (URLs precisam de protocolo — menos falsos positivos)', () => {
    expect(scanForScam('grátis em https://dlscord.gift/free', [], PROTECTED).kind).toBe(
      'lookalike',
    );
    expect(scanForScam('vê https://dlscord.gift/x', [], PROTECTED).hit).toBe(true);
    expect(scanForScam('olá', [], PROTECTED).hit).toBe(false);
    // Domínio "nu" sem protocolo NÃO dispara (limitação assumida).
    expect(scanForScam('dlscord.gift/free', [], PROTECTED).hit).toBe(false);
  });
});

describe('nickname', () => {
  it('deteta hoisting', () => {
    expect(needsDehoist('!!!topo')).toBe(true);
    expect(needsDehoist('normal')).toBe(false);
  });
  it('sanitiza removendo hoisting', () => {
    expect(sanitizeName('!!!diogo')).toBe('diogo');
  });
  it('usa fallback para nomes ilegíveis', () => {
    expect(sanitizeName('░▒▓', 'Membro')).toBe('Membro');
    expect(isUnreadable('░▒▓')).toBe(true);
  });
  it('deteta impersonation por nome normalizado', () => {
    expect(isImpersonating('Dïogo', ['diogo'])).toBe(true);
    expect(isImpersonating('outra', ['diogo'])).toBe(false);
  });
});
