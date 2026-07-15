// Construtores de texto puros para as features de comunidade (welcome, counter).

/** Substitui as variáveis {user}, {server}, {membercount} no template. */
export function renderWelcome(
  template: string,
  vars: { userMention: string; serverName: string; memberCount: number },
): string {
  return template
    .replaceAll('{user}', vars.userMention)
    .replaceAll('{server}', vars.serverName)
    .replaceAll('{membercount}', String(vars.memberCount));
}

/** Nome do canal-contador a partir do template ({count}). */
export function renderCounter(template: string, count: number): string {
  return template.replaceAll('{count}', String(count));
}

/** Valida dia/mês de aniversário (sem ano; aceita 29/2). */
export function isValidBirthday(day: number, month: number): boolean {
  if (month < 1 || month > 12) return false;
  const daysInMonth = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return day >= 1 && day <= daysInMonth[month - 1];
}
