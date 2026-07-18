import { describe, it, expect } from 'vitest';
import { buildPunishmentDm, formatCaseLine } from '../src/moderation/messages.js';

describe('buildPunishmentDm', () => {
  it('inclui servidor, motivo e (para timeout) duração', () => {
    const dm = buildPunishmentDm('timeout', 'MeuServer', 'spam', 3_600_000);
    expect(dm).toContain('MeuServer');
    expect(dm).toContain('spam');
    expect(dm).toContain('1h');
  });

  it('omite duração em warn', () => {
    const dm = buildPunishmentDm('warn', 'S', 'x', 3_600_000);
    expect(dm).not.toContain('Duration');
  });

  it('usa fallback quando não há motivo', () => {
    expect(buildPunishmentDm('kick', 'S', '   ')).toContain('no reason given');
  });
});

describe('formatCaseLine', () => {
  it('mostra nº, tipo e motivo', () => {
    const line = formatCaseLine({
      id: 7,
      guildId: 'g',
      type: 'ban',
      targetId: 't',
      moderatorId: 'm',
      reason: 'raid',
      durationMs: null,
      createdAt: 1_700_000_000_000,
    });
    expect(line).toContain('#7');
    expect(line).toContain('ban');
    expect(line).toContain('raid');
  });
});
