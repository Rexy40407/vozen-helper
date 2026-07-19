import { describe, expect, it } from 'vitest';
import { LanguageTracker, countConfiguredTerms } from '../src/moderation/languagePolicy.js';

const cfg = {
  terms: ['fuck', 'fucking', 'shit', 'bitch', 'asshole'],
  windowMs: 60_000,
  repeatedTermCount: 5,
  excessiveTermCount: 3,
};

describe('política de linguagem', () => {
  it('conta palavras inteiras sem apanhar substrings inocentes', () => {
    expect(countConfiguredTerms('That was fucking shit.', cfg.terms)).toBe(2);
    expect(countConfiguredTerms('The classic assignment passed.', cfg.terms)).toBe(0);
  });

  it('não modera um palavrão casual e isolado', () => {
    const tracker = new LanguageTracker(cfg);
    expect(tracker.record('u', 'well, shit happens', 0, 0).moderate).toBe(false);
  });

  it('modera linguagem dirigida a uma menção', () => {
    const tracker = new LanguageTracker(cfg);
    const result = tracker.record('u', '<@123> you asshole', 1, 0);
    expect(result).toMatchObject({ moderate: true, reason: 'targeted' });
  });

  it('não confunde uma menção positiva com ataque', () => {
    const tracker = new LanguageTracker(cfg);
    const result = tracker.record('u', '<@123> you are fucking awesome', 1, 0);
    expect(result.moderate).toBe(false);
  });

  it('apanha um ataque direto mesmo sem menção', () => {
    const tracker = new LanguageTracker(cfg);
    expect(tracker.record('u', 'fuck you', 0, 0)).toMatchObject({
      moderate: true,
      reason: 'targeted',
    });
  });

  it('modera linguagem excessiva numa mensagem', () => {
    const tracker = new LanguageTracker(cfg);
    const result = tracker.record('u', 'fuck this shit, fucking awful', 0, 0);
    expect(result).toMatchObject({ moderate: true, reason: 'excessive' });
  });

  it('modera repetição dentro da janela e esquece o estado depois', () => {
    const tracker = new LanguageTracker(cfg);
    expect(tracker.record('u', 'shit', 0, 0).moderate).toBe(false);
    expect(tracker.record('u', 'fuck', 0, 10_000).moderate).toBe(false);
    expect(tracker.record('u', 'fucking', 0, 20_000).moderate).toBe(false);
    expect(tracker.record('u', 'shit', 0, 30_000).moderate).toBe(false);
    expect(tracker.record('u', 'asshole', 0, 40_000)).toMatchObject({
      moderate: true,
      reason: 'repeated',
    });
    expect(tracker.record('u', 'shit', 0, 41_000).moderate).toBe(false);
  });
});
