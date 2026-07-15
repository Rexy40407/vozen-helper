// Deteção de phishing/scam por URL. Puro: extrai domínios, compara com a blacklist e
// deteta "lookalikes" (dlscord.gift, discorcl.com) por distância de edição aos
// domínios protegidos. A resolução de redirects (encurtadores) é async e opcional.

const RE_URL = /https?:\/\/[^\s<>()]+/gi;

/** Extrai os hostnames (minúsculas, sem www.) dos URLs de um texto. */
export function extractDomains(text: string): string[] {
  const urls = text.match(RE_URL) ?? [];
  const domains: string[] = [];
  for (const u of urls) {
    try {
      const host = new URL(u).hostname.toLowerCase().replace(/^www\./, '');
      domains.push(host);
    } catch {
      // URL inválido — ignorar
    }
  }
  return domains;
}

/** True se algum domínio (ou o seu domínio-pai) está na blacklist. */
export function isBlacklistedDomain(domain: string, blacklist: readonly string[]): boolean {
  const d = domain.toLowerCase();
  return blacklist.some((b) => {
    const bl = b.toLowerCase();
    return d === bl || d.endsWith('.' + bl);
  });
}

/** Distância de Levenshtein (iterativa, O(n·m)). */
export function levenshtein(a: string, b: string): number {
  const m = a.length;
  const n = b.length;
  const dp = Array.from({ length: m + 1 }, (_, i) => i);
  for (let j = 1; j <= n; j++) {
    let prev = dp[0];
    dp[0] = j;
    for (let i = 1; i <= m; i++) {
      const tmp = dp[i];
      dp[i] = a[i - 1] === b[j - 1] ? prev : 1 + Math.min(prev, dp[i], dp[i - 1]);
      prev = tmp;
    }
  }
  return dp[m];
}

/**
 * "Lookalike" de um domínio protegido: MUITO parecido (distância 1-2) mas NÃO igual.
 * Apanha typosquatting de `discord.com`, `discord.gg`, `discord.gift`. Não marca o
 * domínio legítimo (distância 0).
 */
export function isLookalikeDomain(domain: string, protectedDomains: readonly string[]): boolean {
  const d = domain.toLowerCase();
  for (const p of protectedDomains) {
    const pl = p.toLowerCase();
    if (d === pl || d.endsWith('.' + pl)) return false; // legítimo
    const dist = levenshtein(d, pl);
    if (dist > 0 && dist <= 2) return true;
  }
  return false;
}

/** Verdicto do filtro de scam sobre um texto. */
export function scanForScam(
  text: string,
  blacklist: readonly string[],
  protectedDomains: readonly string[],
): { hit: boolean; domain: string; kind: 'blacklist' | 'lookalike' | '' } {
  for (const domain of extractDomains(text)) {
    if (isBlacklistedDomain(domain, blacklist)) return { hit: true, domain, kind: 'blacklist' };
    if (isLookalikeDomain(domain, protectedDomains)) return { hit: true, domain, kind: 'lookalike' };
  }
  return { hit: false, domain: '', kind: '' };
}
