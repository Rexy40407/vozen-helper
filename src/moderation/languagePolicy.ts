export interface LanguagePolicyConfig {
  terms: readonly string[];
  windowMs: number;
  repeatedTermCount: number;
  excessiveTermCount: number;
}

export type LanguageReason = 'targeted' | 'excessive' | 'repeated';

export interface LanguageVerdict {
  moderate: boolean;
  reason: LanguageReason | null;
  termCount: number;
}

interface LanguageState {
  hits: number[];
}

/** Normalização conservadora: palavras inteiras para evitar falsos positivos por substring. */
function words(text: string): string[] {
  return (
    text
      .normalize('NFKD')
      .replace(/\p{M}/gu, '')
      .toLowerCase()
      .match(/[a-z0-9']+/g) ?? []
  );
}

export function countConfiguredTerms(text: string, terms: readonly string[]): number {
  const configured = new Set(terms.map((term) => term.toLowerCase()));
  return words(text).filter((word) => configured.has(word)).length;
}

const PERSONAL_INSULTS = new Set([
  'asshole',
  'bastard',
  'bitch',
  'cunt',
  'dickhead',
  'motherfucker',
]);

function isTargeted(text: string, mentionCount: number, terms: readonly string[]): boolean {
  const tokens = words(text);
  const configured = new Set(terms.map((term) => term.toLowerCase()));
  const hasPersonalInsult = tokens.some(
    (token) => configured.has(token) && PERSONAL_INSULTS.has(token),
  );
  if (mentionCount > 0 && hasPersonalInsult) return true;

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (!configured.has(token)) continue;
    const directPronoun = [tokens[i - 1], tokens[i + 1]].some(
      (word) => word === 'you' || word === 'your' || word === 'u' || word === 'ur',
    );
    if (token.startsWith('fuck') && directPronoun) return true;
    const near = tokens.slice(Math.max(0, i - 2), i + 3);
    if (
      PERSONAL_INSULTS.has(token) &&
      near.some((word) => word === 'you' || word === 'your' || word === 'u' || word === 'ur')
    ) {
      return true;
    }
  }
  return false;
}

/** Tolera uso casual isolado e modera apenas abuso dirigido, excessivo ou repetido. */
export class LanguageTracker {
  private readonly users = new Map<string, LanguageState>();

  constructor(private readonly cfg: LanguagePolicyConfig) {}

  record(userId: string, content: string, mentionCount: number, now: number): LanguageVerdict {
    const termCount = countConfiguredTerms(content, this.cfg.terms);
    if (termCount === 0) return { moderate: false, reason: null, termCount: 0 };

    const state = this.users.get(userId) ?? { hits: [] };
    const since = now - this.cfg.windowMs;
    state.hits = state.hits.filter((at) => at >= since);
    for (let i = 0; i < termCount; i++) state.hits.push(now);

    let reason: LanguageReason | null = null;
    if (isTargeted(content, mentionCount, this.cfg.terms)) reason = 'targeted';
    else if (termCount >= this.cfg.excessiveTermCount) reason = 'excessive';
    else if (state.hits.length >= this.cfg.repeatedTermCount) reason = 'repeated';

    if (reason) state.hits = [];
    this.users.set(userId, state);
    return { moderate: reason !== null, reason, termCount };
  }

  forget(userId: string): void {
    this.users.delete(userId);
  }
}
