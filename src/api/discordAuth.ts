// Verificação de identidade Discord — feita SEMPRE no servidor.
//
// O browser obtém um access_token (OAuth implicit, scope=identify) e envia-o à API.
// NUNCA se confia no id que o browser diz ter; a API pergunta ao Discord "quem é o
// dono deste token?" (/users/@me) e compara com a conta autorizada.

/** Utilizador Discord (só os campos que usamos). */
export interface DiscordUser {
  id: string;
  username: string;
  global_name?: string | null;
}

/** Assinatura mínima do `fetch` — permite injetar um fake nos testes. */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

const DISCORD_ME = 'https://discord.com/api/v10/users/@me';

/**
 * Devolve o utilizador dono do `token`, ou `null` se o token for inválido/vazio.
 * Erros de rede propagam (o chamador decide o código: ex. 502).
 */
export async function fetchDiscordUser(
  token: string,
  fetchImpl: FetchLike = fetch,
): Promise<DiscordUser | null> {
  const clean = (token ?? '').trim();
  if (!clean) return null;

  const res = await fetchImpl(DISCORD_ME, {
    headers: { Authorization: `Bearer ${clean}` },
  });
  if (!res.ok) return null;

  const data = (await res.json()) as DiscordUser;
  if (!data || typeof data.id !== 'string') return null;
  return data;
}
