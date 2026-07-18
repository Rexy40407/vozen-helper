import { describe, it, expect } from 'vitest';
import {
  capsRatio,
  countEmojis,
  countNewlines,
  countLinks,
  hasInvite,
  contentKey,
} from '../src/moderation/spamMetrics.js';
import { SpamTracker } from '../src/moderation/spamTracker.js';
import type { SpamConfig } from '../src/config.js';

describe('métricas de spam', () => {
  it('capsRatio', () => {
    expect(capsRatio('HELLO')).toBe(1);
    expect(capsRatio('hello')).toBe(0);
    expect(capsRatio('12345')).toBe(0);
    expect(capsRatio('HeLLo')).toBeCloseTo(0.6, 1);
  });
  it('countEmojis', () => {
    expect(countEmojis('oi 😀😀 <:custom:123>')).toBe(3);
    expect(countEmojis('sem emoji')).toBe(0);
  });
  it('countNewlines/countLinks/hasInvite', () => {
    expect(countNewlines('a\nb\nc')).toBe(2);
    expect(countLinks('http://a.com e https://b.com')).toBe(2);
    expect(hasInvite('junta-te discord.gg/abc')).toBe(true);
    expect(hasInvite('sem convite')).toBe(false);
  });
  it('contentKey normaliza espaços/caixa', () => {
    expect(contentKey('  Olá   MUNDO ')).toBe('olá mundo');
  });
});

const cfg: SpamConfig = {
  enabled: true,
  windowMs: 10_000,
  decayPerSec: 1,
  punishAt: 10,
  timeoutMs: 300_000,
  floodCount: 5,
  floodHeat: 4,
  duplicateHeat: 4,
  multiChannelThreshold: 3,
  multiChannelHeat: 6,
  mentionLimit: 5,
  mentionHeat: 2,
  capsRatio: 0.7,
  capsMinLength: 10,
  capsHeat: 3,
  emojiLimit: 8,
  emojiHeat: 3,
  newlineLimit: 10,
  newlineHeat: 3,
  linkLimit: 3,
  linkHeat: 3,
  inviteHeat: 5,
};

describe('SpamTracker', () => {
  it('conversa normal não pune', () => {
    const t = new SpamTracker(cfg);
    let punished = false;
    for (let i = 0; i < 4; i++) {
      const v = t.record({ userId: 'u', channelId: 'c', content: `msg ${i}`, mentionCount: 0 }, i * 3000);
      punished ||= v.punish;
    }
    expect(punished).toBe(false);
  });

  it('duplicados rápidos acumulam heat e punem', () => {
    const t = new SpamTracker(cfg);
    let punished = false;
    for (let i = 0; i < 6; i++) {
      const v = t.record({ userId: 'u', channelId: 'c', content: 'compra nitro gratis', mentionCount: 0 }, i * 500);
      punished ||= v.punish;
    }
    expect(punished).toBe(true);
  });

  it('mesmo link em vários canais dispara multi-canal', () => {
    const t = new SpamTracker(cfg);
    const v1 = t.record({ userId: 'u', channelId: 'c1', content: 'http://x.com http://y.com', mentionCount: 0 }, 0);
    const v2 = t.record({ userId: 'u', channelId: 'c2', content: 'http://x.com http://y.com', mentionCount: 0 }, 200);
    const v3 = t.record({ userId: 'u', channelId: 'c3', content: 'http://x.com http://y.com', mentionCount: 0 }, 400);
    expect([v1, v2, v3].some((v) => v.signals.includes('multi-channel'))).toBe(true);
  });

  it('o calor decai com o tempo (não pune se espaçado)', () => {
    const t = new SpamTracker(cfg);
    let punished = false;
    for (let i = 0; i < 8; i++) {
      // 20s entre mensagens: decaimento (1/s) apaga o heat entre elas.
      const v = t.record({ userId: 'u', channelId: 'c', content: 'igual', mentionCount: 0 }, i * 20_000);
      punished ||= v.punish;
    }
    expect(punished).toBe(false);
  });

  it('mass mentions somam heat proporcional', () => {
    const t = new SpamTracker(cfg);
    const v = t.record({ userId: 'u', channelId: 'c', content: 'oi', mentionCount: 10 }, 0);
    expect(v.signals).toContain('mentions');
  });
});
