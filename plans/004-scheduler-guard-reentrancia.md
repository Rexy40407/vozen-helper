# Plan 004: Guard de reentrância no scheduler (sem lembretes/bump duplicados)

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): excertos vs código vivo antes de editar.

## Status
- **Prioridade**: P2 · **Esforço**: S · **Risco**: LOW · **Depende de**: nenhum
- **Categoria**: bug · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

O `ExpiryScheduler` corre `runDue` no arranque (`start()`) E num `setInterval` de 30s,
sem guard de reentrância. `runDue` só apaga a ação (`deleteScheduled`, no `finally`)
**depois** do `await runner(...)`. Se um `runDue` demora mais que o tick (muitas ações
acumuladas após downtime, ou rede lenta), o tick seguinte — ou o scan inicial a correr
em paralelo com o 1.º tick — volta a buscar as mesmas linhas ainda não apagadas e chama
o runner outra vez. Ações não-idempotentes (`reminder`, `bump_reminder`) são então
enviadas em duplicado. (`giveaway_end` está protegido: `markGiveawayEnded` é síncrono e
corre antes de qualquer await.)

## Estado atual

`src/moderation/scheduler.ts`, classe `ExpiryScheduler`:
```ts
async runDue(now: number): Promise<void> {
  const due = getDueActions(this.db, now);
  for (const action of due) {
    try { await this.runner(this.client, action); }
    catch (err) { log.error(`Falha na ação agendada #${action.id} (${action.type}):`, err); }
    finally { deleteScheduled(this.db, action.id); }
  }
}
start(nowProvider: () => number = Date.now): void {
  void this.runDue(nowProvider());
  this.timer = setInterval(() => void this.runDue(nowProvider()), this.tickMs);
  this.timer.unref?.();
}
```
`getDueActions` (`src/store/cases.ts`) devolve tudo com `execute_at <= now`;
`deleteScheduled(db, id)` apaga por id.

## Comandos

| Objetivo | Comando | Esperado |
|-----------|---------|----------|
| Typecheck/Build | `npm run typecheck` / `npm run build` | exit 0 |
| Testes | `npx vitest run` | todos passam |

## Scope

**In scope:** `src/moderation/scheduler.ts` · `tests/` (teste do guard).
**Out of scope:** o dispatcher `runCommunityScheduled` e os runners individuais — não
alterar. `getDueActions`/`deleteScheduled` — não alterar.

## Passos

### Passo 1: flag de reentrância

Adiciona um campo `private running = false;` e envolve o corpo de `runDue`:
```ts
async runDue(now: number): Promise<void> {
  if (this.running) return;        // evita sobreposição de passagens
  this.running = true;
  try {
    const due = getDueActions(this.db, now);
    for (const action of due) {
      try { await this.runner(this.client, action); }
      catch (err) { log.error(`Falha na ação agendada #${action.id} (${action.type}):`, err); }
      finally { deleteScheduled(this.db, action.id); }
    }
  } finally {
    this.running = false;
  }
}
```

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: teste do guard

`ExpiryScheduler` recebe `db`, `client`, `runner`, `tickMs` no construtor — testável
com um `client` fake (`{}` as never) e um `runner` que regista as ações que recebe. Em
`tests/` (novo `tests/scheduler.test.ts`):
1. `initDb(':memory:')`; agenda 1 ação `reminder` vencida (`scheduleAction` com
   `executeAt` no passado).
2. Cria o scheduler com um `runner` LENTO (devolve uma Promise que resolves quando tu
   quiseres) que conta chamadas.
3. Dispara `runDue(now)` **duas vezes concorrentemente** (sem await na 1.ª) → o runner é
   chamado **exatamente 1×** (a 2.ª passagem sai pelo guard).

Se orquestrar a Promise lenta for complicado, um teste mais simples que sirva: chamar
`runDue` e, no `runner`, chamar `runDue` outra vez (reentrância) e afirmar que a
chamada aninhada não reprocessa. Modela por `tests/cases.test.ts`.

**Verify**: `npx vitest run` → todos passam, +1 teste.

## Done criteria

- [ ] `npm run typecheck` / `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] `grep -n "this.running" src/moderation/scheduler.ts` → presente (guard existe)
- [ ] Teste prova que passagens sobrepostas não reprocessam a mesma ação
- [ ] `plans/README.md` linha 004 atualizada

## STOP conditions

- O corpo de `runDue`/`start` não corresponde ao excerto → STOP.
- Se descobrires que `deleteScheduled` já corre ANTES do `await runner` no código vivo
  (já mitigado) → STOP e reporta.

## Notas de manutenção

- O guard resolve sobreposição no MESMO processo. Duas instâncias do bot com o mesmo
  token (erro de operação) continuariam a duplicar — isso é prevenido pelo lock de
  instância única do `scripts/start-prod.mjs`, fora deste plano.
