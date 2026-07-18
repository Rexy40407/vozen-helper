import type { JoinGateConfig } from '../config.js';

// Join gate: avalia um membro que acabou de entrar contra filtros de risco (idade da
// conta, avatar, padrões de username) e decide a ação. Puro — recebe factos simples.

export interface JoinInfo {
  accountAgeMs: number;
  hasAvatar: boolean;
  username: string;
}

export type GateAction = 'none' | 'kick' | 'ban';

export interface GateVerdict {
  action: GateAction;
  reason: string;
}

/** Aplica os filtros pela ordem de severidade; devolve a primeira ação que dispara. */
export function evaluateJoin(info: JoinInfo, cfg: JoinGateConfig): GateVerdict {
  if (cfg.minAccountAgeMs > 0 && info.accountAgeMs < cfg.minAccountAgeMs) {
    return { action: cfg.newAccountAction, reason: 'Account too new' };
  }
  if (cfg.requireAvatar && !info.hasAvatar) {
    return { action: cfg.noAvatarAction, reason: 'Account without avatar' };
  }
  const lower = info.username.toLowerCase();
  for (const pat of cfg.blockedUsernameSubstrings) {
    if (lower.includes(pat.toLowerCase())) {
      return { action: cfg.badUsernameAction, reason: `Username suspeito (${pat})` };
    }
  }
  return { action: 'none', reason: '' };
}
