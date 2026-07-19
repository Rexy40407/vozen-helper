import 'dotenv/config';
import { Events, MessageFlags, type Guild, type Interaction } from 'discord.js';
import { loadEnv, modConfig } from './config.js';
import { setLogLevel, log } from './log.js';
import { initDb } from './store/db.js';
import { createClient } from './bot/client.js';
import { shouldLeaveGuild } from './bot/guildGuard.js';
import { commandByName } from './commands/index.js';
import type { AppContext } from './context.js';
import { ExpiryScheduler } from './moderation/scheduler.js';
import { makeExpiryRunner } from './moderation/expiryRunner.js';
import { syncAutomod } from './moderation/automodSync.js';
import { handleMessageContent, handleAutomodExecution } from './events/contentModeration.js';
import { SpamTracker } from './moderation/spamTracker.js';
import { handleMessageSpam } from './events/antispam.js';
import { registerLoggingHandlers } from './events/logging.js';
import { RaidDetector } from './moderation/raidDetector.js';
import {
  handleMemberJoin,
  handleMemberLeaveSticky,
  handleMemberVerify,
} from './events/memberGate.js';
import { handleVerifyButton, VERIFY_BUTTON_ID } from './commands/security.js';
import { NukeTracker } from './moderation/nukeTracker.js';
import { handleAuditForNuke } from './events/antinuke.js';
import { handleMessageScam, handleNickname } from './events/antiscam.js';
import { runCommunityScheduled } from './community/scheduler.js';
import { routeCommunityButton, handleCommunityMessage } from './community/router.js';
import { updateMemberCounter, startCounterJob, handleWelcomeDm } from './community/welcome.js';
import { bumpStat } from './community/stats.js';
import { handleStarReaction } from './community/starboard.js';
import { purgeExpired, deleteLevelsForUser, DAY_MS } from './store/gdpr.js';
import { primeInviteCache, registerInviteCacheHandlers } from './community/inviteTracker.js';
import { handleActivityJoin, handleActivityLeave } from './events/activity.js';
import { LanguageTracker } from './moderation/languagePolicy.js';
import { handleMessageLanguage } from './events/languageModeration.js';

// Ponto de entrada do Vozen Helper. Carrega e valida a config, abre a DB, cria o
// cliente, liga os handlers e faz login. Falha rápido se a config estiver errada.

const cfg = loadEnv(process.env);
setLogLevel(cfg.logLevel);

const db = initDb(cfg.dbPath);
log.info(`Base de dados aberta em ${cfg.dbPath}.`);

const client = createClient();

const ctx: AppContext = { db, env: cfg, modConfig, client };

// Scheduler partilhado: expirações de moderação + ações de comunidade (lembretes,
// giveaways, aniversário, bump), encaminhadas por tipo.
const modRunner = makeExpiryRunner(cfg.guildId);
const scheduler = new ExpiryScheduler(db, client, async (c, action) => {
  if (action.type === 'unban' || action.type === 'untimeout' || action.type === 'unmute') {
    await modRunner(c, action);
  } else {
    await runCommunityScheduled(ctx, c, action);
  }
});

// Purga RGPD: corre uma vez no arranque e depois a cada 24h. Apaga registos além da
// retenção (ver docs/GDPR-INVENTARIO.md). unref() para não segurar o event loop.
let purgeTimer: NodeJS.Timeout | null = null;
function runPurge(): void {
  try {
    const counts = purgeExpired(db, cfg.guildId, Date.now());
    if (Object.keys(counts).length > 0) log.info('Purga RGPD:', counts);
  } catch (err) {
    log.error('Falha na purga RGPD:', err);
  }
}

// Tracker de anti-spam (estado em memória).
const spamTracker = new SpamTracker(modConfig.spam);

// Palavrões casuais isolados não são punidos; abuso dirigido/repetido é moderado.
const languageTracker = new LanguageTracker(modConfig.language);

// Detetor de raid (janela de joins, em memória).
const raidDetector = new RaidDetector(modConfig.raid);

// Tracker de anti-nuke (contadores por executor, em memória).
const nukeTracker = new NukeTracker(modConfig.antiNuke.windowMs);

// Guard single-guild: sair de qualquer guild que não a permitida. Verifica no
// arranque (guilds onde já estava) e a cada novo convite.
async function enforceGuildGuard(guild: Guild): Promise<void> {
  if (shouldLeaveGuild(guild.id, cfg.guildId)) {
    log.warn(`Guild não autorizada (${guild.name} / ${guild.id}) — a sair.`);
    await guild.leave().catch((err) => log.error('Falha ao sair da guild:', err));
  }
}

client.once(Events.ClientReady, async (c) => {
  log.info(`Ligado como ${c.user.tag}. A verificar guilds...`);
  for (const guild of c.guilds.cache.values()) {
    await enforceGuildGuard(guild);
  }
  // Varredura inicial das expirações vencidas durante o downtime + tick periódico.
  scheduler.start();
  // Purga RGPD: já no arranque + diária.
  runPurge();
  purgeTimer = setInterval(runPurge, DAY_MS);
  purgeTimer.unref?.();
  // Sincronizar as regras do AutoMod nativo com a config (best-effort).
  const guild = c.guilds.cache.get(cfg.guildId);
  if (guild) await syncAutomod(guild, modConfig.automod);
  // Registo de atividade: preparar o cache de convites (para atribuir joins) + manter fresco.
  if (guild) await primeInviteCache(guild);
  registerInviteCacheHandlers(c, cfg.guildId);
  // Jobs de comunidade: contador de membros.
  startCounterJob(c, ctx);
  void updateMemberCounter(ctx, true);
  log.info('Vozen Helper pronto.');
});

client.on(Events.GuildCreate, enforceGuildGuard);

// Filtro de conteúdo próprio (mensagens novas e editadas) + anti-spam + anti-scam +
// hooks de comunidade (AFK, XP, stats, bump).
client.on(Events.MessageCreate, (m) => {
  void handleMessageContent(ctx, m);
  void handleMessageSpam(ctx, spamTracker, m);
  void handleMessageScam(ctx, m);
  void handleMessageLanguage(ctx, languageTracker, m);
  void handleCommunityMessage(ctx, m);
});
client.on(Events.MessageUpdate, (_old, m) => {
  void (async () => {
    const full = m.partial ? await m.fetch().catch(() => null) : m;
    if (full) await handleMessageContent(ctx, full);
  })();
});

// Consumo dos triggers do AutoMod nativo (alimenta a escalação).
client.on(Events.AutoModerationActionExecution, (exec) => void handleAutomodExecution(ctx, exec));

// Logging completo (mensagens, membros, voz, servidor, mod).
registerLoggingHandlers(client, ctx);

// Anti-raid / join gate / sticky roles / verificação + stats + counter.
client.on(Events.GuildMemberAdd, (m) => {
  void handleMemberJoin(ctx, m, raidDetector);
  void handleNickname(ctx, m);
  bumpStat(ctx, 'joins');
  void updateMemberCounter(ctx);
  void handleActivityJoin(ctx, m); // registo de atividade (join + convite)
  void handleWelcomeDm(ctx, m, raidDetector); // DM de boas-vindas + mini-tour
});
client.on(Events.GuildMemberRemove, (m) => {
  handleMemberLeaveSticky(ctx, m);
  bumpStat(ctx, 'leaves');
  void updateMemberCounter(ctx);
  spamTracker.forget(m.id); // liberta o estado de anti-spam de quem saiu (evita leak)
  languageTracker.forget(m.id);
  deleteLevelsForUser(db, cfg.guildId, m.id); // minimização RGPD: XP não persiste após a saída
  handleActivityLeave(ctx, m); // registo de atividade (leave)
});
client.on(Events.GuildMemberUpdate, (oldM, newM) => {
  void handleMemberVerify(ctx, oldM, newM);
  void handleNickname(ctx, newM);
});

// Anti-nuke: mesmo evento de audit log que o logging, mas com contadores por executor.
client.on(
  Events.GuildAuditLogEntryCreate,
  (entry, guild) => void handleAuditForNuke(ctx, entry, guild, nukeTracker),
);

// Starboard (reações ⭐).
client.on(
  Events.MessageReactionAdd,
  (reaction, user) => void handleStarReaction(ctx, reaction, user),
);
client.on(
  Events.MessageReactionRemove,
  (reaction, user) => void handleStarReaction(ctx, reaction, user),
);

client.on(Events.InteractionCreate, async (interaction: Interaction) => {
  // Botões (verificação + comunidade: sugestões, giveaways, self-roles, tickets).
  if (interaction.isButton()) {
    if (interaction.guildId && interaction.guildId !== cfg.guildId) return;
    if (interaction.customId === VERIFY_BUTTON_ID) {
      await handleVerifyButton(interaction, ctx);
      return;
    }
    await routeCommunityButton(ctx, interaction).catch((err) => log.error('Erro num botão:', err));
    return;
  }
  if (!interaction.isChatInputCommand()) return;
  // Nunca processar interações fora do guild permitido (defesa em profundidade).
  if (interaction.guildId && interaction.guildId !== cfg.guildId) return;

  const command = commandByName.get(interaction.commandName);
  if (!command) return;

  // Restringir os comandos aos canais permitidos (se configurados), EXCETO os
  // públicos (ex.: /rank, /suggest, /afk). A moderação automática corre no servidor
  // todo — isto é só para os slash commands de mod.
  const allowedChannels = modConfig.commandChannelIds;
  if (
    !command.public &&
    allowedChannels.length > 0 &&
    interaction.channelId &&
    !allowedChannels.includes(interaction.channelId)
  ) {
    await interaction.reply({
      content: `Use the commands in ${allowedChannels.map((c) => `<#${c}>`).join(' or ')}.`,
      flags: MessageFlags.Ephemeral,
    });
    return;
  }

  try {
    await command.execute(interaction, ctx);
  } catch (err) {
    log.error(`Erro no comando /${interaction.commandName}:`, err);
    if (interaction.isRepliable() && !interaction.replied && !interaction.deferred) {
      await interaction
        .reply({ content: 'An error occurred while running the command.', ephemeral: true })
        .catch(() => undefined);
    }
  }
});

// Encerramento limpo: fechar a DB (o WAL faz checkpoint) para não deixar o ficheiro
// bloqueado. Idempotente sob SIGINT/SIGTERM repetidos.
let closing = false;
function shutdown(signal: string): void {
  if (closing) return;
  closing = true;
  log.info(`Recebido ${signal} — a encerrar.`);
  scheduler.stop();
  if (purgeTimer) clearInterval(purgeTimer);
  client.destroy();
  try {
    db.close();
  } catch {
    // ignorar
  }
  process.exit(0);
}
process.on('SIGINT', () => shutdown('SIGINT'));
process.on('SIGTERM', () => shutdown('SIGTERM'));

client.login(cfg.token).catch((err) => {
  log.error('Falha no login:', err);
  process.exit(1);
});
