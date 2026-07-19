import { describe, it, expect } from 'vitest';
import { normalizeForMatch, squashRepeats } from '../src/moderation/normalize.js';
import { findBannedWord, findDangerousAttachment } from '../src/moderation/contentFilter.js';
import { buildDesiredRules, planSync, RULE_PREFIX } from '../src/moderation/automodSync.js';
import { modConfig, type AutomodConfig } from '../src/config.js';

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
    { name: `${RULE_PREFIX} advertising`, build: () => ({}) as never },
    { name: `${RULE_PREFIX} presets`, build: () => ({}) as never },
  ];
  it('cria as que faltam e atualiza as nossas já existentes', () => {
    const plan = planSync([`${RULE_PREFIX} advertising`, 'Regra feita à mão'], desired);
    expect(plan.toUpdate).toContain(`${RULE_PREFIX} advertising`);
    expect(plan.toCreate).toContain(`${RULE_PREFIX} presets`);
  });
  it('ignora regras não-nossas no update', () => {
    const plan = planSync(['Regra feita à mão'], desired);
    expect(plan.toCreate).toHaveLength(2);
    expect(plan.toUpdate).toHaveLength(0);
  });
});

describe('buildDesiredRules', () => {
  const cfg: AutomodConfig = {
    advertisingKeywords: ['*discord.gg/*', '*youtube.com/*'],
    advertisingRegexPatterns: ['domain-regex'],
    enableSlurPreset: true,
    enableProfanityPreset: false,
    enableSexualPreset: true,
    mentionLimit: 8,
    mentionRaidProtectionEnabled: true,
  };

  it('separa publicidade, conteúdo severo e spam de menções', () => {
    const rules = buildDesiredRules(cfg);
    expect(rules.map((r) => r.name)).toEqual([
      `${RULE_PREFIX} advertising`,
      `${RULE_PREFIX} presets`,
      `${RULE_PREFIX} mention-spam`,
    ]);

    const advertising = rules[0].build('guild');
    expect(advertising.triggerMetadata?.keywordFilter).toEqual(cfg.advertisingKeywords);
    expect(advertising.triggerMetadata?.regexPatterns).toEqual(cfg.advertisingRegexPatterns);

    const presets = rules[1].build('guild');
    expect(presets.triggerMetadata?.presets).toEqual([2, 3]);

    const mentions = rules[2].build('guild');
    expect(mentions.triggerMetadata?.mentionRaidProtectionEnabled).toBe(true);
  });

  it('regexes de publicidade não confundem domínios parecidos', () => {
    const matches = (text: string) =>
      modConfig.automod.advertisingRegexPatterns.some((pattern) =>
        new RegExp(pattern, 'i').test(text),
      );
    expect(matches('join us at discord.gg/example')).toBe(true);
    expect(matches('https://x.com/vozen/status/1')).toBe(true);
    expect(matches('https://examplex.com/path')).toBe(false);
    expect(matches('https://sidekick.com/path')).toBe(false);
  });
});
