import { describe, it, expect } from 'vitest';
import { AuditLogEvent } from 'discord.js';
import { snowflakeToTimestamp, accountAgeMs, DISCORD_EPOCH } from '../src/moderation/snowflake.js';
import { pickLogChannel, shouldIgnore } from '../src/logging/router.js';
import { truncate, diffRoles, describeAuditAction, LOGGED_AUDIT_ACTIONS } from '../src/logging/format.js';
import type { LogConfig } from '../src/config.js';

describe('snowflake', () => {
  it('descodifica o timestamp (id 0 = epoch)', () => {
    expect(snowflakeToTimestamp('0')).toBe(DISCORD_EPOCH);
  });
  it('conta idade não-negativa', () => {
    const id = '0';
    expect(accountAgeMs(id, DISCORD_EPOCH + 1000)).toBe(1000);
    expect(accountAgeMs(id, DISCORD_EPOCH - 1000)).toBe(0);
  });
});

const logCfg: LogConfig = {
  channels: { messages: 'chan-msg', members: null, voice: null, server: null, mod: 'chan-mod' },
  ignoreChannelIds: ['ignored'],
  ignoreUserIds: ['bot-user'],
};

describe('router de logs', () => {
  it('escolhe o canal certo (ou null)', () => {
    expect(pickLogChannel(logCfg, 'messages')).toBe('chan-msg');
    expect(pickLogChannel(logCfg, 'members')).toBeNull();
  });
  it('ignora canal/utilizador da lista', () => {
    expect(shouldIgnore(logCfg, { channelId: 'ignored' })).toBe(true);
    expect(shouldIgnore(logCfg, { userId: 'bot-user' })).toBe(true);
    expect(shouldIgnore(logCfg, { channelId: 'ok', userId: 'ok' })).toBe(false);
  });
});

describe('formatadores', () => {
  it('truncate respeita o limite', () => {
    expect(truncate('abcdef', 4)).toHaveLength(4);
    expect(truncate('ab', 4)).toBe('ab');
  });
  it('diffRoles calcula adicionados/removidos', () => {
    const d = diffRoles(['a', 'b'], ['b', 'c']);
    expect(d.added).toEqual(['c']);
    expect(d.removed).toEqual(['a']);
  });
  it('describeAuditAction cobre ações de mod', () => {
    expect(describeAuditAction(AuditLogEvent.MemberBanAdd)).toBe('banned');
    expect(LOGGED_AUDIT_ACTIONS).toContain(AuditLogEvent.ChannelDelete);
  });
});
