import type { Client, Guild } from 'discord.js';
import { log } from '../log.js';

// Atribuição de convites: o Discord NÃO diz qual convite um membro usou. Truque padrão —
// manter em cache o nº de usos de cada convite; quando alguém entra, voltar a buscar os
// convites e ver qual subiu de usos (ou qual de uso-único desapareceu). Requer o intent
// GuildInvites + permissão Manage Guild. A parte de decisão (`attributeInvite`) é pura.

export interface CachedInvite {
  uses: number;
  inviterId: string | null;
}

/** Cache por guild: code -> { uses, inviterId }. Em memória (repõe-se no arranque). */
const cache = new Map<string, Map<string, CachedInvite>>();

/**
 * Descobre por que convite se entrou, comparando o cache ANTES com os convites DEPOIS.
 * Pura e testável. Ordem de deteção: (1) um convite cujos usos aumentaram; (2) um convite
 * de uso-único que foi consumido e desapareceu (recupera o inviter do cache). Se nada
 * bater certo (vanity URL, ou dois joins entre fetches) → null (indeterminado).
 */
export function attributeInvite(
  before: Map<string, CachedInvite>,
  after: Array<{ code: string; uses: number; inviterId: string | null }>,
): { code: string; inviterId: string | null } | null {
  for (const inv of after) {
    const prevUses = before.get(inv.code)?.uses ?? 0;
    if (inv.uses > prevUses) return { code: inv.code, inviterId: inv.inviterId };
  }
  const afterCodes = new Set(after.map((i) => i.code));
  for (const [code, prev] of before) {
    if (!afterCodes.has(code)) return { code, inviterId: prev.inviterId };
  }
  return null;
}

/** Lê os convites atuais do guild como lista simples (para o cache / attribute). */
async function fetchInvites(
  guild: Guild,
): Promise<Array<{ code: string; uses: number; inviterId: string | null }>> {
  const invites = await guild.invites.fetch().catch(() => null);
  if (!invites) return [];
  return [...invites.values()].map((i) => ({
    code: i.code,
    uses: i.uses ?? 0,
    inviterId: i.inviter?.id ?? null,
  }));
}

/** Converte a lista num Map para o cache. */
function toCache(list: Array<{ code: string; uses: number; inviterId: string | null }>): Map<string, CachedInvite> {
  return new Map(list.map((i) => [i.code, { uses: i.uses, inviterId: i.inviterId }]));
}

/** Popula o cache de convites de um guild (chamar no ClientReady). */
export async function primeInviteCache(guild: Guild): Promise<void> {
  const list = await fetchInvites(guild);
  cache.set(guild.id, toCache(list));
  log.debug(`[invites] cache preparada: ${list.length} convites em ${guild.id}`);
}

/**
 * Chamado no GuildMemberAdd: re-busca os convites, descobre o usado, atualiza o cache e
 * devolve a atribuição (ou null se indeterminado). Best-effort — nunca lança.
 */
export async function resolveJoinInvite(
  guild: Guild,
): Promise<{ code: string; inviterId: string | null } | null> {
  try {
    const before = cache.get(guild.id) ?? new Map<string, CachedInvite>();
    const after = await fetchInvites(guild);
    const used = attributeInvite(before, after);
    cache.set(guild.id, toCache(after)); // atualizar o cache para o próximo join
    return used;
  } catch (err) {
    log.debug('[invites] falha a resolver convite do join:', (err as Error).message);
    return null;
  }
}

/** Mantém o cache fresco quando um convite é criado/apagado (evita falhas de atribuição). */
export function registerInviteCacheHandlers(client: Client, guildId: string): void {
  client.on('inviteCreate', (invite) => {
    if (invite.guild?.id !== guildId) return;
    const g = cache.get(guildId) ?? new Map<string, CachedInvite>();
    g.set(invite.code, { uses: invite.uses ?? 0, inviterId: invite.inviter?.id ?? null });
    cache.set(guildId, g);
  });
  client.on('inviteDelete', (invite) => {
    if (invite.guild?.id !== guildId) return;
    cache.get(guildId)?.delete(invite.code);
  });
}
