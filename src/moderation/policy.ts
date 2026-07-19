/** Regras públicas que têm uma consequência objetiva no Vozen Support. */
export type ServerRule =
  | 'nsfw'
  | 'tos'
  | 'doxxing'
  | 'hate_speech'
  | 'spam'
  | 'disrespect'
  | 'staff_disrespect'
  | 'harassment'
  | 'advertising'
  | 'language'
  | 'channel_misuse';

export type PolicyAction =
  { action: 'ban' } | { action: 'timeout'; durationMs: number } | { action: 'strike' };

export const DAY_MS = 24 * 60 * 60 * 1000;

/** Aumenta o timeout de spam em 6x por reincidência, com um teto configurável. */
export function progressiveSpamTimeout(
  baseMs: number,
  priorOffenses: number,
  maxMs: number,
): number {
  const safePrior = Math.max(0, Math.floor(priorOffenses));
  return Math.min(baseMs * 6 ** safePrior, maxMs);
}

/** Traduz uma violação na punição publicada, sem depender do Discord ou da DB. */
export function decidePolicyAction(
  rule: ServerRule,
  priorOffenses: number,
  spamBaseMs: number,
  spamMaxMs: number,
): PolicyAction {
  if (rule === 'nsfw' || rule === 'tos' || rule === 'doxxing' || rule === 'hate_speech') {
    return { action: 'ban' };
  }
  if (rule === 'advertising') {
    return priorOffenses > 0 ? { action: 'ban' } : { action: 'timeout', durationMs: DAY_MS };
  }
  if (rule === 'spam') {
    return {
      action: 'timeout',
      durationMs: progressiveSpamTimeout(spamBaseMs, priorOffenses, spamMaxMs),
    };
  }
  return { action: 'strike' };
}

export const SERVER_RULE_LABELS: Readonly<Record<ServerRule, string>> = {
  nsfw: 'NSFW / NSFL content',
  tos: 'Discord Terms of Service violation',
  doxxing: 'Doxxing / leaked personal information',
  hate_speech: 'Hate speech',
  spam: 'Spam',
  disrespect: 'Disrespect',
  staff_disrespect: 'Disrespecting staff',
  harassment: 'Harassment',
  advertising: 'Advertising',
  language: 'Repeated, excessive, or targeted language',
  channel_misuse: 'Using a channel for the wrong purpose',
};
