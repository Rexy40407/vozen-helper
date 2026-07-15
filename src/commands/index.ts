import type {
  ChatInputCommandInteraction,
  SlashCommandOptionsOnlyBuilder,
  SlashCommandBuilder,
  SlashCommandSubcommandsOnlyBuilder,
} from 'discord.js';
import type { AppContext } from '../context.js';
import * as ping from './ping.js';
import { modCommands } from './moderation.js';
import { securityCommands } from './security.js';
import { utilCommands } from './utils.js';
import { suggestionCommands } from '../community/suggestions.js';
import { afkCommands } from '../community/afk.js';
import { tagCommands } from '../community/tags.js';
import { reminderCommands } from '../community/reminders.js';
import { selfRoleCommands } from '../community/selfroles.js';
import { giveawayCommands } from '../community/giveaways.js';
import { levelCommands } from '../community/levels.js';
import { ticketCommands } from '../community/tickets.js';
import { statsCommands } from '../community/stats.js';
import { privacyCommands } from '../community/privacy.js';

// Registo central de comandos. Cada comando exporta `data` (builder) e `execute`.
// O `execute` recebe o contexto partilhado (db/config/client) — o /ping ignora-o.

export interface Command {
  data:
    | SlashCommandBuilder
    | SlashCommandOptionsOnlyBuilder
    | SlashCommandSubcommandsOnlyBuilder;
  execute: (interaction: ChatInputCommandInteraction, ctx: AppContext) => Promise<void>;
  /** Se true, o comando pode ser usado em QUALQUER canal (ignora commandChannelIds). */
  public?: boolean;
}

export const commands: readonly Command[] = [
  ping,
  ...modCommands,
  ...securityCommands,
  ...utilCommands,
  // Comunidade
  ...suggestionCommands,
  ...afkCommands,
  ...tagCommands,
  ...reminderCommands,
  ...selfRoleCommands,
  ...giveawayCommands,
  ...levelCommands,
  ...ticketCommands,
  ...statsCommands,
  ...privacyCommands,
];

/** Mapa nome -> comando, para o dispatch das interações. */
export const commandByName: ReadonlyMap<string, Command> = new Map(
  commands.map((c) => [c.data.name, c]),
);
