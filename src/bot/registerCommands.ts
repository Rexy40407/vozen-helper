import 'dotenv/config';
import { REST, Routes } from 'discord.js';
import { loadEnv } from '../config.js';
import { commands } from '../commands/index.js';
import { log } from '../log.js';

// Regista os slash commands NO GUILD configurado (não globalmente): num bot
// single-guild o registo por-guild é instantâneo (o global demora até 1h a propagar).
// Corre à parte com `npm run register` sempre que a lista de comandos muda.

async function main(): Promise<void> {
  const cfg = loadEnv(process.env);
  const body = commands.map((c) => c.data.toJSON());
  const rest = new REST({ version: '10' }).setToken(cfg.token);

  log.info(`A registar ${body.length} comando(s) no guild ${cfg.guildId}...`);
  await rest.put(Routes.applicationGuildCommands(cfg.clientId, cfg.guildId), { body });
  log.info('Comandos registados.');
}

main().catch((err) => {
  log.error('Falha ao registar comandos:', err);
  process.exitCode = 1;
});
