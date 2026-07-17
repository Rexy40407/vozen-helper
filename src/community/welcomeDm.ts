import { renderWelcome } from './text.js';

// DM privada de boas-vindas + mini-tour ao membro novo. A lógica pura (decidir se
// envia, e construir o texto) vive aqui, testável sem Discord. O envio em si e o
// wiring no GuildMemberAdd ficam no handler (welcome.ts / index.ts).

/** Decide se se envia a DM. Puro: flag ligada, não-bot, e fora de modo raid. */
export function shouldSendWelcomeDm(input: {
  enabled: boolean;
  isBot: boolean;
  /** Durante um raid não se envia (evita rajada de DMs → spam-flag da Discord). */
  isRaiding: boolean;
}): boolean {
  return input.enabled && !input.isBot && !input.isRaiding;
}

/** Constrói o texto da DM a partir do template efetivo (vars {user}/{server}/{membercount}). */
export function buildWelcomeDm(
  template: string,
  vars: { userMention: string; serverName: string; memberCount: number },
): string {
  return renderWelcome(template, vars);
}
