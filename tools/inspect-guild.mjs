// Ferramenta de leitura: lista os canais e cargos do servidor (id + nome + tipo),
// para o assistente poder configurar o bot por NOME em vez de pedir IDs à mão.
// Uso: node tools/inspect-guild.mjs   (lê DISCORD_TOKEN/GUILD_ID do .env)
// Só LÊ — não altera nada no servidor.

import 'dotenv/config';
import { Client, GatewayIntentBits, Events, ChannelType } from 'discord.js';

const TYPE = {
  [ChannelType.GuildText]: 'text',
  [ChannelType.GuildVoice]: 'voice',
  [ChannelType.GuildCategory]: 'category',
  [ChannelType.GuildAnnouncement]: 'announcement',
  [ChannelType.GuildForum]: 'forum',
  [ChannelType.GuildStageVoice]: 'stage',
};

const client = new Client({ intents: [GatewayIntentBits.Guilds] });

client.once(Events.ClientReady, async (c) => {
  const g = await c.guilds.fetch(process.env.GUILD_ID);
  const channels = await g.channels.fetch();
  const roles = await g.roles.fetch();

  const chOut = [...channels.values()]
    .filter(Boolean)
    .map((ch) => ({ id: ch.id, name: ch.name, type: TYPE[ch.type] ?? String(ch.type) }));
  const roleOut = [...roles.values()]
    .map((r) => ({ id: r.id, name: r.name, position: r.position, managed: r.managed }))
    .sort((a, b) => b.position - a.position);

  console.log('===GUILD===', g.name);
  console.log('===CHANNELS===');
  console.log(JSON.stringify(chOut, null, 0));
  console.log('===ROLES===');
  console.log(JSON.stringify(roleOut, null, 0));
  await c.destroy();
  process.exit(0);
});

client.login(process.env.DISCORD_TOKEN);
