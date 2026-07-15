import type { ModConfig } from '../config.js';

// Quem escapa ao automod/anti-spam/filtros: staff (roles de mod), whitelist (owner,
// bots de confiança). Puro — recebe os IDs, não objetos do Discord.

export function isExemptMember(
  userId: string,
  memberRoleIds: readonly string[],
  cfg: Pick<ModConfig, 'modRoleIds' | 'whitelistIds'>,
): boolean {
  if (cfg.whitelistIds.includes(userId)) return true;
  const modRoles = new Set(cfg.modRoleIds);
  return memberRoleIds.some((r) => modRoles.has(r));
}

export function isExemptChannel(channelId: string, exemptChannelIds: readonly string[]): boolean {
  return exemptChannelIds.includes(channelId);
}
