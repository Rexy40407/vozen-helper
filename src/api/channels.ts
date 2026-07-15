// Lista os canais do servidor via REST do Discord (com o token do bot). A API não
// tem cliente gateway; para o painel poder mostrar um dropdown de canais, vai buscá-los
// diretamente à API do Discord. Só canais onde dá para enviar (texto=0, anúncios=5).

export interface GuildChannel {
  id: string;
  name: string;
  type: number;
}

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

/** Canais de texto/anúncios do servidor. Lança em erro de rede/HTTP. */
export async function fetchGuildChannels(
  token: string,
  guildId: string,
  fetchImpl: FetchLike = fetch,
): Promise<GuildChannel[]> {
  const res = await fetchImpl(`https://discord.com/api/v10/guilds/${guildId}/channels`, {
    headers: { Authorization: `Bot ${token}` },
  });
  if (!res.ok) throw new Error(`channels ${res.status}`);
  const data = (await res.json()) as { id: string; name: string; type: number }[];
  return data
    .filter((c) => c.type === 0 || c.type === 5)
    .map((c) => ({ id: c.id, name: c.name, type: c.type }));
}

/**
 * Valida o `value` de uma chave de canal contra o conjunto de IDs válidos.
 * '' = limpar (usar default). 'current' só é válido para `chan.levelup`.
 * `xp.exclude` é uma lista CSV de IDs.
 */
export function validateChannelValue(key: string, value: string, validIds: ReadonlySet<string>): boolean {
  if (value === '') return true; // limpar → volta ao default
  if (key === 'chan.levelup' && value === 'current') return true;
  if (key === 'xp.exclude') {
    return value
      .split(',')
      .filter((s) => s.length > 0)
      .every((id) => validIds.has(id));
  }
  return validIds.has(value);
}
