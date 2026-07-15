// Guard single-guild: o Vozen Helper é PRIVADO e só pode operar no servidor
// configurado (GUILD_ID). Se for adicionado a qualquer outro servidor, sai logo.
// Isto impede que uma cópia do token/convite ponha o bot a moderar sítios errados.

/**
 * Decide se o bot deve SAIR de um dado guild. Função pura (sem I/O) — a decisão é
 * testável isoladamente; quem chama trata do `guild.leave()`.
 *
 * @param guildId    guild onde o bot se encontra (ou onde acabou de entrar)
 * @param allowedGuildId  o único guild permitido (config.guildId)
 * @returns true se o guild NÃO é o permitido (deve sair)
 */
export function shouldLeaveGuild(guildId: string, allowedGuildId: string): boolean {
  return guildId !== allowedGuildId;
}
