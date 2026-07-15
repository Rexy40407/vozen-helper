import { describe, it, expect, beforeEach, vi } from 'vitest';
import type Database from 'better-sqlite3';
import { initDb } from '../src/store/db.js';
import { saveQuarantine, isQuarantined } from '../src/store/quarantine.js';
import { unquarantineMember } from '../src/moderation/quarantineService.js';
import { modConfig } from '../src/config.js';

// Plano 002 — unquarantine não pode apagar o registo se a reposição de cargos falhar.

const G = '111111111111111111';
const U = '222222222222222222';

function makeCtx(db: Database.Database) {
  // client stub: alertOwner tenta fetch de canal/owner — devolvem null → no-op silencioso.
  const client = {
    user: { id: 'bot' },
    channels: { fetch: async () => null },
    guilds: { fetch: async () => null },
    users: { fetch: async () => null },
  } as never;
  return { db, env: { guildId: G }, modConfig, client } as never;
}

function makeMember(add: () => Promise<unknown>) {
  return {
    id: U,
    guild: {
      id: G,
      roles: { cache: new Map([['r1', {}]]) },
      fetchOwner: async () => null,
    },
    roles: { add },
  } as never;
}

describe('unquarantineMember', () => {
  let db: Database.Database;
  beforeEach(() => {
    db = initDb(':memory:');
    saveQuarantine(db, G, U, ['r1'], 'nuke', 1);
  });

  it('mantém o registo quando a reposição falha', async () => {
    const ctx = makeCtx(db);
    const member = makeMember(vi.fn().mockRejectedValue(new Error('missing permissions')));
    const ok = await unquarantineMember(ctx, (member as { guild: unknown }).guild as never, member);
    expect(ok).toBe(false);
    expect(isQuarantined(db, G, U)).toBe(true); // NÃO se perdeu
  });

  it('limpa o registo quando a reposição tem sucesso', async () => {
    const ctx = makeCtx(db);
    const member = makeMember(vi.fn().mockResolvedValue(undefined));
    const ok = await unquarantineMember(ctx, (member as { guild: unknown }).guild as never, member);
    expect(ok).toBe(true);
    expect(isQuarantined(db, G, U)).toBe(false);
  });
});
