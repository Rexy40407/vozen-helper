import { describe, expect, it } from 'vitest';
import { decidePolicyAction, progressiveSpamTimeout } from '../src/moderation/policy.js';

describe('política das regras públicas', () => {
  it.each(['nsfw', 'tos', 'doxxing', 'hate_speech'] as const)(
    '%s resulta em ban imediato',
    (rule) => {
      expect(decidePolicyAction(rule, 0, 300_000, 86_400_000)).toEqual({ action: 'ban' });
    },
  );

  it('publicidade dá timeout de 1 dia na primeira infração e ban na segunda', () => {
    expect(decidePolicyAction('advertising', 0, 300_000, 86_400_000)).toEqual({
      action: 'timeout',
      durationMs: 86_400_000,
    });
    expect(decidePolicyAction('advertising', 1, 300_000, 86_400_000)).toEqual({ action: 'ban' });
  });

  it('spam repetido recebe timeouts progressivamente maiores com limite', () => {
    expect(progressiveSpamTimeout(300_000, 0, 86_400_000)).toBe(300_000);
    expect(progressiveSpamTimeout(300_000, 1, 86_400_000)).toBe(1_800_000);
    expect(progressiveSpamTimeout(300_000, 2, 86_400_000)).toBe(10_800_000);
    expect(progressiveSpamTimeout(300_000, 99, 86_400_000)).toBe(86_400_000);
  });

  it.each(['disrespect', 'staff_disrespect', 'harassment', 'language', 'channel_misuse'] as const)(
    '%s segue warn/strike',
    (rule) => {
      expect(decidePolicyAction(rule, 0, 300_000, 86_400_000)).toEqual({ action: 'strike' });
    },
  );
});
