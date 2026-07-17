import { describe, it, expect, beforeEach } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import { setSetting } from '../src/store/settings.js';
import {
  getTextSetting,
  getEffectiveTexts,
  clearTextCache,
  textSettingKey,
  TEXT_SETTING_KEYS,
} from '../src/store/textSettings.js';
import { shouldSendWelcomeDm, buildWelcomeDm } from '../src/community/welcomeDm.js';
import { handleWelcomeDm } from '../src/community/welcome.js';
import { flagSettingKey, clearFlagCache } from '../src/store/flags.js';
import { RaidDetector } from '../src/moderation/raidDetector.js';
import { modConfig } from '../src/config.js';
import type { AppContext } from '../src/context.js';

// DM de boas-vindas + mini-tour: resolver do texto (override do painel) e a
// decisão pura de enviar (flag / bot / raid).

const G = '111111111111111111';
let db: Database.Database;
beforeEach(() => {
  db = initDb(':memory:');
  clearTextCache();
  clearFlagCache();
});

describe('resolver de textos (override do painel)', () => {
  it('sem override devolve o default do modConfig', () => {
    const def = modConfig.community.welcomeDm.message;
    expect(getTextSetting(db, G, 'welcomedm.message', def, 1000)).toBe(def);
  });

  it('override na BD vence o default', () => {
    setSetting(db, G, textSettingKey('welcomedm.message'), 'Olá {user}!', 1000);
    clearTextCache();
    expect(getTextSetting(db, G, 'welcomedm.message', 'default', 1000)).toBe('Olá {user}!');
  });

  it('cache: mudança só é vista após o TTL', () => {
    getTextSetting(db, G, 'welcomedm.message', 'A', 1000); // popula cache
    setSetting(db, G, textSettingKey('welcomedm.message'), 'B', 1000);
    expect(getTextSetting(db, G, 'welcomedm.message', 'A', 1000)).toBe('A'); // cacheado
    expect(getTextSetting(db, G, 'welcomedm.message', 'A', 1000 + 5000)).toBe('B'); // relê
  });

  it('getEffectiveTexts devolve valor efetivo e default', () => {
    setSetting(db, G, textSettingKey('welcomedm.message'), 'custom', 1);
    const texts = getEffectiveTexts(db, G, modConfig);
    const t = texts.find((x) => x.key === 'welcomedm.message');
    expect(t?.value).toBe('custom');
    expect(t?.default).toBe(modConfig.community.welcomeDm.message);
  });

  it('allowlist só tem as chaves conhecidas', () => {
    expect(TEXT_SETTING_KEYS.has('welcomedm.message')).toBe(true);
    expect(TEXT_SETTING_KEYS.has('evil.key')).toBe(false);
  });
});

describe('shouldSendWelcomeDm', () => {
  const base = { enabled: true, isBot: false, isRaiding: false };

  it('envia no caso normal', () => {
    expect(shouldSendWelcomeDm(base)).toBe(true);
  });

  it('não envia com a flag off', () => {
    expect(shouldSendWelcomeDm({ ...base, enabled: false })).toBe(false);
  });

  it('não envia a bots', () => {
    expect(shouldSendWelcomeDm({ ...base, isBot: true })).toBe(false);
  });

  it('não envia durante um raid (evita rajada de DMs)', () => {
    expect(shouldSendWelcomeDm({ ...base, isRaiding: true })).toBe(false);
  });
});

describe('buildWelcomeDm', () => {
  it('substitui as variáveis do template', () => {
    const out = buildWelcomeDm('Olá {user}, bem-vindo a {server}! Nº {membercount}', {
      userMention: '<@42>',
      serverName: 'Vozen',
      memberCount: 7,
    });
    expect(out).toBe('Olá <@42>, bem-vindo a Vozen! Nº 7');
  });
});

// ── Handler (integração leve com um member/ctx fake) ─────────────────────────
interface SendCall {
  content: string;
}
function fakeMember(opts: { bot?: boolean; sendImpl?: () => Promise<unknown> }) {
  const sent: SendCall[] = [];
  const member = {
    id: '42',
    user: { bot: opts.bot ?? false, tag: 'user#0001' },
    guild: { id: G, name: 'Vozen', memberCount: 100 },
    send: opts.sendImpl ?? (async (payload: SendCall) => void sent.push(payload)),
  };
  return { member, sent };
}
function fakeCtx(database: Database.Database): AppContext {
  return {
    db: database,
    env: { guildId: G } as AppContext['env'],
    modConfig,
    client: {} as AppContext['client'],
  };
}
/** RaidDetector que nunca dispara (config sem raid). */
function calmRaid(): RaidDetector {
  return new RaidDetector({ ...modConfig.raid, joinThreshold: 999999 });
}

describe('handleWelcomeDm (handler)', () => {
  it('envia a DM no caso normal (flag default = on)', async () => {
    const { member, sent } = fakeMember({});
    await handleWelcomeDm(fakeCtx(db), member as never, calmRaid(), 1000);
    expect(sent).toHaveLength(1);
    expect(sent[0].content).toContain('Vozen');
  });

  it('não envia com o flag desligado no painel', async () => {
    setSetting(db, G, flagSettingKey('welcomedm'), 'false', 1000);
    clearTextCache();
    const { member, sent } = fakeMember({});
    await handleWelcomeDm(fakeCtx(db), member as never, calmRaid(), 6000);
    expect(sent).toHaveLength(0);
  });

  it('não envia a bots', async () => {
    const { member, sent } = fakeMember({ bot: true });
    await handleWelcomeDm(fakeCtx(db), member as never, calmRaid(), 1000);
    expect(sent).toHaveLength(0);
  });

  it('usa o texto editado no painel', async () => {
    setSetting(db, G, textSettingKey('welcomedm.message'), 'CUSTOM {server}', 1000);
    clearTextCache();
    const { member, sent } = fakeMember({});
    await handleWelcomeDm(fakeCtx(db), member as never, calmRaid(), 6000);
    expect(sent[0].content).toBe('CUSTOM Vozen');
  });

  it('sobrevive a DMs fechadas (erro 50007) sem lançar', async () => {
    const { member } = fakeMember({
      sendImpl: async () => {
        throw Object.assign(new Error('Cannot send messages to this user'), { code: 50007 });
      },
    });
    await expect(handleWelcomeDm(fakeCtx(db), member as never, calmRaid(), 1000)).resolves.toBeUndefined();
  });
});
