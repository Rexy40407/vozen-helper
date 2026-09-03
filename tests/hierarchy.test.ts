import { describe, it, expect } from 'vitest';
import { canBotActOn, canModeratorActOn } from '../src/moderation/hierarchy.js';

describe('canBotActOn', () => {
  it('pode agir sobre alguém abaixo', () => {
    expect(canBotActOn({ botTopPosition: 10, targetTopPosition: 5, targetIsOwner: false }).ok).toBe(
      true,
    );
  });
  it('não age sobre o dono', () => {
    expect(canBotActOn({ botTopPosition: 10, targetTopPosition: 5, targetIsOwner: true }).ok).toBe(
      false,
    );
  });
  it('não age sobre role igual ou superior', () => {
    expect(canBotActOn({ botTopPosition: 5, targetTopPosition: 5, targetIsOwner: false }).ok).toBe(
      false,
    );
    expect(canBotActOn({ botTopPosition: 5, targetTopPosition: 8, targetIsOwner: false }).ok).toBe(
      false,
    );
  });
  it('não age sobre si próprio', () => {
    expect(
      canBotActOn({
        botTopPosition: 10,
        targetTopPosition: 5,
        targetIsOwner: false,
        targetIsSelf: true,
      }).ok,
    ).toBe(false);
  });
});

describe('canModeratorActOn', () => {
  it('o dono pode sempre', () => {
    expect(canModeratorActOn(1, 100, true).ok).toBe(true);
  });
  it('mod não age sobre igual/superior', () => {
    expect(canModeratorActOn(5, 5, false).ok).toBe(false);
    expect(canModeratorActOn(5, 6, false).ok).toBe(false);
  });
  it('mod age sobre inferior', () => {
    expect(canModeratorActOn(6, 5, false).ok).toBe(true);
  });
});
