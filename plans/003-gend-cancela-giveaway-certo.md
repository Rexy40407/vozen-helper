# Plan 003: `/gend` só cancela o auto-fim do giveaway indicado

> **Instruções do executor**: passo a passo, verifica cada comando, STOP nas condições,
> atualiza `plans/README.md`. **Drift check** (sem git): confirma os excertos vs código
> vivo antes de editar.

## Status
- **Prioridade**: P1
- **Esforço**: S
- **Risco**: LOW
- **Depende de**: nenhum
- **Categoria**: bug
- **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

Ao criar um giveaway, a ação agendada `giveaway_end` é gravada com `targetId =
interaction.user.id` (o host) e o id real do giveaway só no `payload`. O `/gend`
cancela com `cancelScheduled(db, guildId, 'giveaway_end', gw.hostId)`, que apaga por
`target_id` — ou seja **todas** as ações `giveaway_end` desse host. Se o mesmo staffer
tem 2+ giveaways ativos, terminar um via `/gend` remove o auto-fim dos outros → esses
ficam pendurados e nunca terminam sozinhos.

## Estado atual

- `src/community/giveaways.ts`, criação (~linha 94):
  ```ts
  scheduleAction(ctx.db, { guildId: interaction.guildId, type: 'giveaway_end', targetId: interaction.user.id, executeAt: endAt, payload: String(id), caseId: null });
  ```
- `src/community/giveaways.ts`, `/gend` (~linha 112):
  ```ts
  cancelScheduled(ctx.db, interaction.guildId, 'giveaway_end', gw.hostId);
  await endGiveaway(ctx, id);
  ```
- `src/store/cases.ts`, `cancelScheduled` (~linha 223) apaga por `(guild_id, type,
  target_id)` — não olha para o `payload`:
  ```ts
  export function cancelScheduled(db, guildId, type: ScheduledType, targetId: string): void {
    db.prepare(`DELETE FROM scheduled_actions WHERE guild_id = ? AND type = ? AND target_id = ?`).run(guildId, type, targetId);
  }
  ```
- `endGiveaway(ctx, id)` no mesmo ficheiro já é idempotente (`markGiveawayEnded`
  síncrono no início; `getGiveaway` devolve `ended`).

## Comandos

| Objetivo | Comando | Esperado |
|-----------|---------|----------|
| Typecheck | `npm run typecheck` | exit 0 |
| Testes | `npx vitest run` | todos passam |

## Scope

**In scope:**
- `src/store/cases.ts` (nova função de cancelamento por payload)
- `src/community/giveaways.ts` (usar a nova função)
- `tests/` — teste do novo cancelamento

**Out of scope:**
- A lógica de `endGiveaway`/`scheduleAction`/`getDueActions` — não alterar.
- Os outros tipos de ação agendada (`unban`, `reminder`, etc.) — a assinatura antiga de
  `cancelScheduled` continua a servi-los.

## Passos

### Passo 1: cancelamento por payload

Em `src/store/cases.ts`, adiciona uma função que apaga a ação agendada pelo `payload`
exato (que para giveaways é o id como string), mantendo `guild_id` + `type`:
```ts
/** Cancela ações agendadas de um tipo cujo payload é EXATAMENTE `payload`. */
export function cancelScheduledByPayload(
  db: Database.Database,
  guildId: string,
  type: ScheduledType,
  payload: string,
): void {
  db.prepare(`DELETE FROM scheduled_actions WHERE guild_id = ? AND type = ? AND payload = ?`).run(guildId, type, payload);
}
```
Mantém `cancelScheduled` como está (outros callers dependem dela).

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: `/gend` usa o cancelamento certo

Em `src/community/giveaways.ts`, no `/gend`, troca a linha de cancelamento por:
```ts
cancelScheduledByPayload(ctx.db, interaction.guildId, 'giveaway_end', String(id));
```
(importa `cancelScheduledByPayload` do store; remove o import de `cancelScheduled` se
deixar de ser usado neste ficheiro — o typecheck avisa se ficar por usar.)

**Verify**: `npm run typecheck` → exit 0.

### Passo 3: teste

Em `tests/` (novo `tests/giveawaysSchedule.test.ts` ou dentro de `community.test.ts`),
com DB `:memory:`:
1. `scheduleAction` de dois `giveaway_end` do MESMO host, payloads `'1'` e `'2'`.
2. `cancelScheduledByPayload(db, g, 'giveaway_end', '1')`.
3. `getDueActions(db, farFuture)` → resta exatamente a de payload `'2'`.

Modela por `tests/cases.test.ts` (secção "ações agendadas").

**Verify**: `npx vitest run` → todos passam, +1 teste.

## Done criteria

- [ ] `npm run typecheck` exit 0 · `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] `grep -n "cancelScheduled(" src/community/giveaways.ts` → sem correspondência (passou a `cancelScheduledByPayload`)
- [ ] Teste novo prova que cancelar o giveaway `'1'` preserva o `'2'`
- [ ] `plans/README.md` linha 003 atualizada

## STOP conditions

- Os excertos de `giveaways.ts`/`cases.ts` não batem com o código vivo → STOP.
- Se `scheduleAction` do giveaway já não usar `payload: String(id)` → STOP (a chave de
  cancelamento mudou).

## Notas de manutenção

- Alternativa de design (não obrigatória): gravar `giveaway_end` com `targetId = id do
  giveaway` em vez do host, tornando o cancelamento por `targetId` já correto. Não
  escolhida para não misturar semânticas de `targetId` entre tipos de ação; se um dia se
  fizer, rever este cancelamento.
