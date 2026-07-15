import { describe, it, expect } from 'vitest';
import { decideEscalation } from '../src/moderation/escalation.js';
import type { EscalationStep } from '../src/config.js';

const ladder: EscalationStep[] = [
  { atStrikes: 3, action: 'timeout', durationMs: 3_600_000 },
  { atStrikes: 5, action: 'timeout', durationMs: 86_400_000 },
  { atStrikes: 7, action: 'ban' },
];

describe('decideEscalation', () => {
  it('não escala abaixo do primeiro degrau', () => {
    expect(decideEscalation(1, ladder)).toBeNull();
    expect(decideEscalation(2, ladder)).toBeNull();
  });

  it('escolhe o degrau exato', () => {
    expect(decideEscalation(3, ladder)?.action).toBe('timeout');
    expect(decideEscalation(3, ladder)?.durationMs).toBe(3_600_000);
  });

  it('escolhe o degrau MAIS ALTO atingido', () => {
    expect(decideEscalation(6, ladder)?.durationMs).toBe(86_400_000);
    expect(decideEscalation(10, ladder)?.action).toBe('ban');
  });

  it('funciona com uma escada desordenada', () => {
    const shuffled = [ladder[2], ladder[0], ladder[1]];
    expect(decideEscalation(7, shuffled)?.action).toBe('ban');
  });
});
