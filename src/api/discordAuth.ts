// Verificação de identidade Discord — feita SEMPRE no servidor.
//
// O browser obtém um access_token (OAuth implicit, scope=identify) e envia-o à API.
// NUNCA se confia no id que o browser diz ter; a API pergunta ao Discord "quem é o
// dono deste token?" (/oauth2/@me e /users/@me) e compara com a conta autorizada.

/** Utilizador Discord (só os campos que usamos). */
export interface DiscordUser {
  id: string;
  username: string;
  global_name?: string | null;
}

/** Assinatura mínima do `fetch` — permite injetar um fake nos testes. */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

const DISCORD_ME = 'https://discord.com/api/v10/users/@me';
const DISCORD_OAUTH_ME = 'https://discord.com/api/v10/oauth2/@me';

interface DiscordOAuthInfo {
  application?: { id?: unknown };
  scopes?: unknown;
  user?: { id?: unknown };
}

function isOAuthInfoForClient(data: unknown, clientId: string): data is DiscordOAuthInfo {
  if (!data || typeof data !== 'object') return false;
  const oauth = data as DiscordOAuthInfo;
  return (
    typeof oauth.application?.id === 'string' &&
    oauth.application.id === clientId &&
    typeof oauth.user?.id === 'string' &&
    oauth.user.id.length > 0 &&
    Array.isArray(oauth.scopes) &&
    oauth.scopes.includes('identify')
  );
}

/**
 * Devolve o utilizador dono do `token`, ou `null` se o token for inválido/vazio.
 * Erros de rede propagam (o chamador decide o código: ex. 502).
 */
export async function fetchDiscordUser(
  token: string,
  clientId: string,
  fetchImpl: FetchLike = fetch,
): Promise<DiscordUser | null> {
  const clean = (token ?? '').trim();
  const expectedClientId = (clientId ?? '').trim();
  if (!clean || !expectedClientId) return null;

  const oauthRes = await fetchImpl(DISCORD_OAUTH_ME, {
    headers: { Authorization: `Bearer ${clean}` },
  });
  if (!oauthRes.ok) return null;

  let oauth: unknown;
  try {
    oauth = await oauthRes.json();
  } catch {
    return null;
  }
  if (!isOAuthInfoForClient(oauth, expectedClientId)) return null;

  const res = await fetchImpl(DISCORD_ME, {
    headers: { Authorization: `Bearer ${clean}` },
  });
  if (!res.ok) return null;

  let data: unknown;
  try {
    data = await res.json();
  } catch {
    return null;
  }
  if (!data || typeof data !== 'object') return null;
  const user = data as DiscordUser;
  if (typeof user.id !== 'string' || user.id !== oauth.user?.id) return null;
  return user;
}
