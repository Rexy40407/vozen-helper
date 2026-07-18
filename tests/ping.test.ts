import { describe, it, expect } from 'vitest';
import { formatPing } from '../src/commands/ping.js';

describe('formatPing', () => {
  it('mostra a latência quando medida', () => {
    expect(formatPing(42)).toContain('42 ms');
  });

  it('trata a latência ainda-não-medida (-1)', () => {
    expect(formatPing(-1)).toContain('measuring');
  });
});
