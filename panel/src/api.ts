export type RankCardConfig = {
  font: string;
  primary_color: string;
  text_color: string;
  background_color: string;
  overlay_opacity: number;
  background_preset: string | null;
  background_url: string | null;
  background_data: string | null;
  avatar_ring_color: string;
  avatar_ring_width: number;
};

export type Guild = { id: string; name: string; canManage: boolean };
export type Me = { id: string; guildId: string; expiresAt: string; dbOk?: boolean };
export type Feature = {
  key: string;
  label: string;
  description: string;
  category: 'protection' | 'community' | 'management' | 'utility' | 'social' | 'growth' | 'web3';
  capability: string;
  available: boolean;
  enabled: boolean;
};
export type FeatureConfig = Record<string, unknown>;
export type FeatureDetail = { guildId: string; key: string; enabled: boolean; config: FeatureConfig; updatedAt?: string };
export type CaseRecord = {
  id: number;
  kind?: string;
  type?: string;
  target_id?: string;
  targetId?: string;
  moderator_id?: string;
  moderatorId?: string;
  reason: string;
  created_at?: number;
  createdAt?: string;
};
export type AuditRecord = { action: string; actor_id?: string; actorId?: string; outcome: string; created_at?: number };

const base = (import.meta.env.VITE_HELPER_API_BASE as string | undefined)?.replace(/\/$/, '') ?? '';
let sessionBearer: string | null = null;

function persistSessionBearer(token: string | null): void {
  sessionBearer = token;
  try {
    if (token) sessionStorage.setItem('vh_session_bearer', token);
    else sessionStorage.removeItem('vh_session_bearer');
  } catch { /* sessionStorage pode estar bloqueado */ }
}

try { sessionBearer = sessionStorage.getItem('vh_session_bearer'); } catch { /* opcional */ }
const oauthSession = window.location.hash.match(/^#session=([A-Za-z0-9._~-]{32,4096})$/)?.[1];
if (oauthSession) {
  persistSessionBearer(oauthSession);
  window.history.replaceState(null, '', `${window.location.pathname}${window.location.search}#/`);
}

export function apiUrl(path: string): string {
  return `${base}${path}`;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(apiUrl(path), {
    ...init,
    credentials: 'include',
    headers: {
      Accept: 'application/json',
      ...(sessionBearer ? { Authorization: `Bearer ${sessionBearer}` } : {}),
      ...(init?.headers ?? {}),
    },
  });
  if (!response.ok) {
    if (response.status === 401) persistSessionBearer(null);
    const payload = (await response.json().catch(() => ({}))) as { message?: string; code?: string };
    throw new Error(payload.message ?? payload.code ?? `API ${response.status}`);
  }
  return (await response.json()) as T;
}

export const api = {
  me: () => request<Me>('/api/me'),
  guilds: () => request<{ guilds: Guild[] }>('/api/guilds'),
  switchGuild: (guildId: string) => request<{ ok: boolean; guildId: string }>('/api/session/switch', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ guild_id: guildId }),
  }),
  stats: () => request<{ totalCases: number; guildId: string }>('/api/stats'),
  cases: () => request<{ cases: CaseRecord[] }>('/api/cases?limit=8'),
  audit: () => request<{ events: AuditRecord[] }>('/api/audit?limit=12'),
  quotas: () => request<{ plan: string; limits: Record<string, number>; usage: Record<string, number> }>('/api/quotas'),
  modules: () => request<{ modules: string[] }>('/api/modules'),
  features: () => request<{ guildId: string; features: Feature[] }>('/api/config/features'),
  updateFeature: (key: string, enabled: boolean) => request<{ ok: boolean; enabled: boolean }>('/api/config/features', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key, enabled }),
  }),
  feature: (key: string) => request<FeatureDetail>(`/api/config/features/${encodeURIComponent(key)}`),
  saveFeature: (key: string, enabled: boolean, config: FeatureConfig) =>
    request<FeatureDetail>(`/api/config/features/${encodeURIComponent(key)}`, {
      method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ enabled, config }),
    }),
  testFeature: (key: string, config: FeatureConfig) =>
    request<{ ok: boolean; key: string; preview: FeatureConfig }>(`/api/config/features/${encodeURIComponent(key)}/test`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ config }),
    }),
  rankCard: () => request<{ guildId: string; config: RankCardConfig }>('/api/studio/rank-card'),
  saveRankCard: (config: RankCardConfig) =>
    request<{ guildId: string; config: RankCardConfig }>('/api/studio/rank-card', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config),
    }),
  startOAuth: async (guildId = '') => {
    persistSessionBearer(null);
    const verifier = crypto.randomUUID().replaceAll('-', '') + crypto.randomUUID().replaceAll('-', '');
    try { sessionStorage.setItem('vh_oauth_verifier', verifier); } catch { /* storage opcional */ }
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier));
    const challenge = btoa(String.fromCharCode(...new Uint8Array(digest)))
      .replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
    const result = await request<{ authorization_url: string }>('/api/oauth/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ guild_id: guildId, code_challenge: challenge, code_verifier: verifier }),
    });
    window.location.assign(result.authorization_url);
  },
};
