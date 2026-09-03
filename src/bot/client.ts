import { Client, GatewayIntentBits, Partials, Options } from 'discord.js';

// Constrói o cliente discord.js com os intents necessários à moderação.
//
// Privileged (ligar no Developer Portal → Bot → Privileged Gateway Intents):
//   - GuildMembers   → join gate, anti-raid, sticky roles, fim do screening
//   - MessageContent → anti-spam e filtros próprios (sem ele, content vem vazio)
// Não-privileged: Guilds (base/anti-nuke), GuildModeration (audit log em tempo real
//   + bans), GuildMessages, AutoModeration*, GuildWebhooks, GuildInvites.
//
// GuildPresences é DELIBERADAMENTE omitido (privileged e desnecessário à moderação).

export function createClient(): Client {
  return new Client({
    intents: [
      GatewayIntentBits.Guilds,
      GatewayIntentBits.GuildMembers,
      GatewayIntentBits.GuildModeration,
      GatewayIntentBits.GuildMessages,
      GatewayIntentBits.MessageContent,
      GatewayIntentBits.AutoModerationConfiguration,
      GatewayIntentBits.AutoModerationExecution,
      GatewayIntentBits.GuildWebhooks,
      GatewayIntentBits.GuildInvites,
      // Reações para o starboard (comunidade).
      GatewayIntentBits.GuildMessageReactions,
    ],
    // Partials para eventos de mensagens/reações não-cacheadas (logging + starboard).
    partials: [
      Partials.Message,
      Partials.GuildMember,
      Partials.User,
      Partials.Reaction,
      Partials.Channel,
    ],
    makeCache: Options.cacheWithLimits(Options.DefaultMakeCacheSettings),
  });
}
