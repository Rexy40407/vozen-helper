import type { Client } from 'discord.js';
import type { ScheduledAction } from '../store/cases.js';
import { log } from '../log.js';

// Executa uma ação agendada vencida. As expirações são best-effort: se o estado já
// mudou (ex.: já foi desbanido à mão), ignora-se em silêncio.

export function makeExpiryRunner(guildId: string) {
  return async function run(client: Client, action: ScheduledAction): Promise<void> {
    if (action.guildId !== guildId) return;
    const guild = await client.guilds.fetch(guildId).catch(() => null);
    if (!guild) return;

    switch (action.type) {
      case 'unban':
        await guild.bans.remove(action.targetId, 'Fim do tempban (auto-unban)').catch((err) => {
          log.debug(`auto-unban de ${action.targetId} sem efeito:`, (err as Error).message);
        });
        break;
      case 'untimeout': {
        const m = await guild.members.fetch(action.targetId).catch(() => null);
        await m?.timeout(null, 'Fim do timeout').catch(() => undefined);
        break;
      }
      case 'unmute': {
        // payload = ID do role de mute a remover.
        const m = await guild.members.fetch(action.targetId).catch(() => null);
        if (m && action.payload) await m.roles.remove(action.payload, 'Fim do mute').catch(() => undefined);
        break;
      }
    }
  };
}
