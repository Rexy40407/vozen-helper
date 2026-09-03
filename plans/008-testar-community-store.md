# Plan 008: Testar as funções sem cobertura de `community/store.ts`

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): confirma os exports vivos de `store.ts`.

## Status

- **Prioridade**: P2 · **Esforço**: S · **Risco**: LOW · **Depende de**: nenhum
- **Categoria**: tests · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

`src/community/store.ts` (~481 linhas) é o coração de estado de 6 features novas.
`tests/community.test.ts` só cobre suggestions-votes, XP (rank/leaderboard),
giveaway-entries, tags e birthdays. Ficam **sem qualquer teste**: AFK, self-roles,
ciclo de vida de giveaways (`createGiveaway`/`getGiveaway`/`markGiveawayEnded`/
`listActiveGiveaways`), tickets, starboard e stats. São funções puras sobre SQLite — um
erro de `ON CONFLICT`, nome de coluna ou mapeamento de row passa silencioso até
produção. É o teste de maior ROI: o padrão já existe, é só replicar.

## Estado atual

- Padrão de teste existente — `tests/community.test.ts`:
  ```ts
  import { initDb } from '../src/store/db.js';
  import {
    createSuggestion,
    voteSuggestion,
    countSuggestionVotes,
    addXp /* ... */,
  } from '../src/community/store.js';
  let db: Database.Database;
  beforeEach(() => {
    db = initDb(':memory:');
  });
  it('...', () => {
    /* asserts */
  });
  ```
- Funções por cobrir (assinaturas em `src/community/store.ts` — abre o ficheiro e
  confirma antes de escrever os testes):
  - **AFK**: `setAfk(db,g,u,reason,since)`, `getAfk(db,g,u)`, `clearAfk(db,g,u)`
  - **Self-roles**: `setSelfRole(db,msgId,customId,roleId,mode)`, `getSelfRole(db,msgId,customId)`, `getSelfRolesForMessage(db,msgId)`
  - **Giveaways**: `createGiveaway(db, {...})`, `getGiveaway(db,id)`, `markGiveawayEnded(db,id)`, `listActiveGiveaways(db,g)`
  - **Tickets**: `createTicket(db,g,channelId,openerId,now)`, `getOpenTicketForUser(db,g,openerId)`, `getTicketByChannel(db,channelId)`, `setTicketStatus(db,id,status)`, `claimTicket(db,id,userId)`
  - **Starboard**: `getStarEntry(db,g,origId)`, `upsertStarEntry(db,g,origId,sbMsgId,count)`, `deleteStarEntry(db,g,origId)`
  - **Stats**: `incrStat(db,g,date,field)`, `getStatsTotals(db,g)`

## Comandos

| Objetivo  | Comando             | Esperado     |
| --------- | ------------------- | ------------ |
| Testes    | `npx vitest run`    | todos passam |
| Typecheck | `npm run typecheck` | exit 0       |

## Scope

**In scope:** `tests/` (só ficheiros de teste, novos ou estender `community.test.ts`).
**Out of scope:** qualquer alteração a `src/**` — se um teste revelar um bug real,
**NÃO** o corrijas aqui: regista-o (STOP) e reporta, para ser planeado à parte.

## Passos

### Passo 1: AFK, self-roles, stats (casos diretos)

Estende `tests/community.test.ts` (ou cria `tests/communityStore.test.ts`) com, para
cada domínio, um teste do caminho feliz + 1 borda:

- **AFK**: set→get devolve reason/since; clear remove (get→null); clear de inexistente → false.
- **Self-roles**: set→get devolve `{roleId, mode}`; `getSelfRolesForMessage` devolve todos os do msgId; upsert (setSelfRole 2× no mesmo customId) atualiza.
- **Stats**: `incrStat` 3× 'messages' num dia + 1 noutro dia → `getStatsTotals` soma 4; joins/leaves independentes.

**Verify**: `npx vitest run` → passam.

### Passo 2: ciclo de vida de giveaways e tickets

- **Giveaways**: `createGiveaway` (todos os campos) → `getGiveaway` devolve-os
  (`ended=false`); `listActiveGiveaways` inclui-o; `markGiveawayEnded` → `getGiveaway`
  tem `ended=true` E `listActiveGiveaways` **já não o inclui**.
- **Tickets**: `createTicket` → `getOpenTicketForUser` encontra-o (`status='open'`);
  `getTicketByChannel` idem; `claimTicket` grava `claimedBy`; `setTicketStatus('closed')`
  → `getOpenTicketForUser` deixa de o devolver.

**Verify**: `npx vitest run` → passam.

### Passo 3: starboard

- `getStarEntry` de inexistente → null; `upsertStarEntry` cria; upsert de novo atualiza
  o `starCount` e o `starboardMessageId`; `deleteStarEntry` remove (get→null).

**Verify**: `npx vitest run` → todos passam, com os novos testes.

## Done criteria

- [ ] `npx vitest run` — todos passam; ≥10 testes novos cobrindo AFK, self-roles,
      giveaway lifecycle, tickets, starboard, stats
- [ ] `npm run typecheck` exit 0
- [ ] Nenhum ficheiro em `src/` alterado (`ls` / revisão do diff)
- [ ] `plans/README.md` linha 008 atualizada

## STOP conditions

- Uma assinatura de função no `store.ts` vivo difere do que está aqui listado → confirma
  no ficheiro e adapta o teste (não é STOP; é esperado abrires o ficheiro).
- **Um teste revela um bug real** (ex.: `listActiveGiveaways` continua a devolver um
  giveaway terminado) → STOP: NÃO corrijas o `src`; regista o bug e reporta para plano
  próprio.

## Notas de manutenção

- Estes testes são a rede de segurança para o refactor futuro DEBT-01 (partir o
  `store.ts` por domínio). Fazer esse split só DEPOIS destes testes verdes.
