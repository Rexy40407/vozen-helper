import { describe, it, expect } from 'vitest';
import { shouldLeaveGuild } from '../src/bot/guildGuard.js';

describe('shouldLeaveGuild', () => {
  const allowed = '111111111111111111';

  it('fica no guild permitido', () => {
    expect(shouldLeaveGuild(allowed, allowed)).toBe(false);
  });

  it('sai de qualquer outro guild', () => {
    expect(shouldLeaveGuild('222222222222222222', allowed)).toBe(true);
  });
});
