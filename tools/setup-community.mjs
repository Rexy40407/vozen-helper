// Setup one-shot da infraestrutura de comunidade: cria os canais e cargos das
// features (sugestões, starboard, tickets, transcripts, contador, níveis) e imprime
// o mapa nome->ID. Idempotente: reutiliza o que já existir com o mesmo nome.
// Uso: node tools/setup-community.mjs   (lê DISCORD_TOKEN/GUILD_ID do .env)

import 'dotenv/config';
import { Client, GatewayIntentBits, Events, ChannelType, PermissionFlagsBits } from 'discord.js';

const client = new Client({ intents: [GatewayIntentBits.Guilds] });

client.once(Events.ClientReady, async (c) => {
  const guild = await c.guilds.fetch(process.env.GUILD_ID);
  await guild.channels.fetch();
  await guild.roles.fetch();
  const everyone = guild.roles.everyone;
  const out = {};

  const findChannel = (name) => guild.channels.cache.find((ch) => ch.name === name);
  const findRole = (name) => guild.roles.cache.find((r) => r.name === name);

  const textCategory = guild.channels.cache.find(
    (ch) => ch.type === ChannelType.GuildCategory && ch.name === 'Canais de Texto',
  );
  const voiceCategory = guild.channels.cache.find(
    (ch) => ch.type === ChannelType.GuildCategory && ch.name === 'Canais de Voz',
  );
  const modCategory = guild.channels.cache.find(
    (ch) => ch.type === ChannelType.GuildCategory && ch.name === 'Mod Room',
  );

  // Helper: criar canal de texto read-only (@everyone não escreve) idempotente.
  async function textChannel(name, parent, readOnly) {
    let ch = findChannel(name);
    if (!ch) {
      ch = await guild.channels.create({
        name,
        type: ChannelType.GuildText,
        parent: parent?.id,
        permissionOverwrites: readOnly
          ? [{ id: everyone.id, deny: [PermissionFlagsBits.SendMessages] }]
          : undefined,
      });
      console.log(`criado texto ${name}`);
    } else {
      console.log(`reutilizado texto ${name}`);
    }
    return ch.id;
  }

  // #transcripts: só staff vê (esconde de @everyone). Precisa do cargo Staff.
  async function staffOnlyChannel(name, parent, staffRoleId) {
    let ch = findChannel(name);
    if (!ch) {
      const ov = [{ id: everyone.id, deny: [PermissionFlagsBits.ViewChannel] }];
      if (staffRoleId) ov.push({ id: staffRoleId, allow: [PermissionFlagsBits.ViewChannel] });
      ch = await guild.channels.create({
        name,
        type: ChannelType.GuildText,
        parent: parent?.id,
        permissionOverwrites: ov,
      });
      console.log(`criado (staff) ${name}`);
    } else {
      console.log(`reutilizado ${name}`);
    }
    return ch.id;
  }

  // Canal de voz-contador: trancado (ninguém entra).
  async function counterChannel(name, parent) {
    // Procurar por prefixo (o nome muda com a contagem).
    let ch = guild.channels.cache.find(
      (c) => c.type === ChannelType.GuildVoice && c.name.startsWith('📊 Membros'),
    );
    if (!ch) {
      ch = await guild.channels.create({
        name,
        type: ChannelType.GuildVoice,
        parent: parent?.id,
        position: 0,
        permissionOverwrites: [{ id: everyone.id, deny: [PermissionFlagsBits.Connect] }],
      });
      console.log(`criado voz ${name}`);
    } else {
      console.log(`reutilizado voz ${ch.name}`);
    }
    return ch.id;
  }

  async function makeRole(name) {
    let r = findRole(name);
    if (!r) {
      r = await guild.roles.create({ name, mentionable: false, reason: 'Cargo de nível' });
      console.log(`criado cargo ${name}`);
    } else {
      console.log(`reutilizado cargo ${name}`);
    }
    return r.id;
  }

  const staffRole = findRole('👮Staff');

  // Cada passo tolerante a falhas: uma falha regista-se e não aborta o resto.
  const step = async (key, fn) => {
    try {
      out[key] = await fn();
    } catch (err) {
      out[key] = null;
      console.log(`FALHOU ${key}: ${err.message}`);
    }
  };

  // NOTA: a categoria "Mod Room" está trancada ao bot → o transcripts fica SEM
  // categoria (top-level), mas na mesma escondido de @everyone (staff-only).
  void modCategory;
  await step('suggestions', () => textChannel('₊˚ʚ💡୧﹕sugestões', textCategory, true));
  await step('starboard', () => textChannel('₊˚ʚ⭐୧﹕destaques', textCategory, true));
  await step('ticketPanel', () => textChannel('₊˚ʚ🎫୧﹕suporte', textCategory, false));
  await step('transcripts', () => staffOnlyChannel('₊˚ʚ📄୧﹕transcripts', null, staffRole?.id));
  await step('counter', () => counterChannel('📊 Membros: 0', voiceCategory));
  await step('level5', () => makeRole('🥉 Nível 5'));
  await step('level10', () => makeRole('🥈 Nível 10'));
  await step('level20', () => makeRole('🥇 Nível 20'));
  out.staffRole = staffRole?.id ?? null;
  out.generalChat = findChannel('₊˚ʚ📜୧﹕general-chat')?.id ?? null;
  out.botTesting = findChannel('₊˚ʚ🤖୧﹕bot-testing')?.id ?? null;
  out.modHelperBot = findChannel('₊˚ʚ🤖୧﹕mod-helper-bot')?.id ?? null;

  console.log('===IDS===');
  console.log(JSON.stringify(out, null, 2));
  await c.destroy();
  process.exit(0);
});

client.login(process.env.DISCORD_TOKEN);
