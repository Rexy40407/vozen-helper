# Plan 006: `/rolepanel` respeita a hierarquia do moderador que o cria

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): excertos vs código vivo.

## Status
- **Prioridade**: P2 · **Esforço**: S · **Risco**: LOW · **Depende de**: nenhum
- **Categoria**: security · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

`/rolepanel` (permissão `ManageRoles`) filtra os cargos oferecíveis **apenas** pelo teto
do BOT (`r.position < botTop`), nunca pelo cargo mais alto do moderador que corre o
comando. Um staffer de posição baixa (ex.: "helper" com `ManageRoles`) pode publicar um
painel que oferece um cargo **acima do dele** — até um degrau abaixo do bot — e todo o
servidor passa a poder auto-atribuir-se esse cargo pelo botão. Isto contorna a
salvaguarda nativa do Discord (um utilizador não concede cargos acima do seu mais alto):
aqui quem atribui é o bot, anulando essa proteção.

## Estado atual

`src/community/selfroles.ts`, comando `/rolepanel` (~linhas 50-58):
```ts
const roles: Role[] = [];
for (const key of ['role1', 'role2', 'role3', 'role4', 'role5']) {
  const r = interaction.options.getRole(key);
  if (r) roles.push(r as Role);
}
const botTop = interaction.guild.members.me?.roles.highest.position ?? 0;
const usable = roles.filter((r) => r.position < botTop && !r.managed);
if (!usable.length) {
  return void interaction.reply({ content: 'Nenhum dos cargos é atribuível (têm de estar abaixo do meu).', flags: MessageFlags.Ephemeral });
}
```
Padrão de referência já existente para "quem pode agir sobre quê":
`src/moderation/hierarchy.ts` — `canModeratorActOn(modTop, targetTop, modIsOwner)`
devolve `{ ok }` e trata o caso do dono. `interaction.member` é o `GuildMember` do
moderador (o comando usa `interaction.inCachedGuild()`).

## Comandos

| Objetivo | Comando | Esperado |
|-----------|---------|----------|
| Typecheck/Build | `npm run typecheck` / `npm run build` | exit 0 |
| Testes | `npx vitest run` | todos passam |

## Scope

**In scope:** `src/community/selfroles.ts` · `tests/` (teste do filtro).
**Out of scope:** `handleSelfRoleButton` (a atribuição em si) — o painel só deve conter
cargos legítimos; não alterar o handler. `src/moderation/hierarchy.ts` — reutilizar, não
alterar.

## Passos

### Passo 1: filtrar também pela posição do moderador

Calcula o teto do moderador e usa o MENOR entre o teto do bot e o do moderador (o dono
do servidor é exceção — pode oferecer qualquer cargo abaixo do bot). Forma alvo:
```ts
const botTop = interaction.guild.members.me?.roles.highest.position ?? 0;
const isOwner = interaction.guild.ownerId === interaction.member.id;
const modTop = interaction.member.roles.highest.position;
const ceiling = isOwner ? botTop : Math.min(botTop, modTop);
const usable = roles.filter((r) => r.position < ceiling && !r.managed);
if (!usable.length) {
  return void interaction.reply({ content: 'Nenhum cargo é oferecível — têm de estar abaixo do teu cargo e do meu.', flags: MessageFlags.Ephemeral });
}
```

Extrai a decisão numa função pura para testar (recomendado):
```ts
/** Cargos oferecíveis num painel: abaixo do teto (min do bot e do mod, salvo dono). */
export function pickOfferableRoles<T extends { position: number; managed: boolean }>(
  roles: readonly T[], botTop: number, modTop: number, isOwner: boolean,
): T[] {
  const ceiling = isOwner ? botTop : Math.min(botTop, modTop);
  return roles.filter((r) => r.position < ceiling && !r.managed);
}
```
e usa-a no comando.

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: teste

Em `tests/` (novo `tests/selfroles.test.ts` ou dentro de outro), testa `pickOfferableRoles`
com objetos simples `{position, managed}`:
- mod de posição 5, bot 8: um cargo de posição 6 é EXCLUÍDO (acima do mod), um de 3 é incluído.
- `isOwner=true`: o mesmo cargo de posição 6 passa a ser incluído (dono usa o teto do bot).
- cargos `managed` sempre excluídos.

**Verify**: `npx vitest run` → todos passam, +1 teste.

## Done criteria

- [ ] `npm run typecheck` / `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] `grep -n "modTop\|pickOfferableRoles" src/community/selfroles.ts` → presente
- [ ] Teste cobre mod-abaixo excluído, dono-permitido, managed-excluído
- [ ] `plans/README.md` linha 006 atualizada

## STOP conditions

- O bloco de filtragem não corresponde ao excerto → STOP.
- Se `interaction.member.roles.highest` não estiver disponível (o comando deixou de usar
  `inCachedGuild()`) → STOP.

## Notas de manutenção

- Se surgir um comando que edite painéis existentes, aplicar o mesmo teto.
- Reviewer: confirmar que o dono continua a poder criar painéis com cargos altos.
