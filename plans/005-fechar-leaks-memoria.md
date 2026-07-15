# Plan 005: Fechar leaks de memória (SpamTracker + cooldowns de XP)

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): excertos vs código vivo antes de editar.

## Status
- **Prioridade**: P2 · **Esforço**: S · **Risco**: LOW · **Depende de**: nenhum
- **Categoria**: bug · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

Dois Maps em memória crescem uma entrada por cada utilizador que já alguma vez
interagiu e **nunca** são limpos, num processo que corre 24/7:

1. `SpamTracker.users` — tem um método `forget(userId)` que **não é chamado em lado
   nenhum** (grep a `.forget(` devolve zero). Cresce para sempre.
2. `cooldowns` em `community/levels.ts` — recebe uma entrada por utilizador em cada
   mensagem e nunca é podado.

Não é crítico a curto prazo (single-guild, poucos milhares de membros), mas é um leak
real num serviço permanente. `NukeTracker.hits` é limitado (só executores de ações
destrutivas) e limpa em `reset()` — não é problema.

## Estado atual

- `src/moderation/spamTracker.ts` — método existente, sem callers:
  ```ts
  /** Esquece um utilizador (ex.: saiu do servidor). */
  forget(userId: string): void { this.users.delete(userId); }
  ```
- `src/index.ts` — o handler `GuildMemberRemove` NÃO chama `forget`:
  ```ts
  client.on(Events.GuildMemberRemove, (m) => {
    handleMemberLeaveSticky(ctx, m);
    bumpStat(ctx, 'leaves');
    void updateMemberCounter(ctx);
  });
  ```
  O `spamTracker` está em escopo em `index.ts` (`const spamTracker = new SpamTracker(...)`).
- `src/community/levels.ts` — Map de cooldown a nível de módulo, sem poda:
  ```ts
  const cooldowns = new Map<string, number>();
  // ... em handleXpMessage:
  const key = `${message.guildId}:${message.author.id}`;
  const last = cooldowns.get(key) ?? 0;
  if (now - last < cfg.cooldownMs) return;
  cooldowns.set(key, now);
  ```

## Comandos

| Objetivo | Comando | Esperado |
|-----------|---------|----------|
| Typecheck/Build | `npm run typecheck` / `npm run build` | exit 0 |
| Testes | `npx vitest run` | todos passam |

## Scope

**In scope:** `src/index.ts` · `src/moderation/spamTracker.ts` (talvez) ·
`src/community/levels.ts` · `tests/`.
**Out of scope:** `NukeTracker`, `RaidDetector` (não têm o problema).

## Passos

### Passo 1: esquecer o utilizador no leave

Em `src/index.ts`, no handler `GuildMemberRemove`, acrescenta `spamTracker.forget(m.id);`.
(Confirma que `m.id` é o id do membro/utilizador — é.)

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: podar os cooldowns de XP

Em `src/community/levels.ts`, adiciona uma poda barata e determinística: ao inserir no
`cooldowns`, se o Map exceder um teto (ex.: `> 10_000` entradas), remove as entradas
cujo `last` é mais velho que `cfg.cooldownMs` (já não servem para nada — o cooldown já
expirou). Extrai a poda para uma função **pura testável**:
```ts
/** Remove entradas de cooldown já expiradas. Pura (recebe o Map e o now). */
export function pruneCooldowns(map: Map<string, number>, now: number, cooldownMs: number): void {
  if (map.size <= 10_000) return;
  for (const [k, t] of map) if (now - t >= cooldownMs) map.delete(k);
}
```
E chama `pruneCooldowns(cooldowns, now, cfg.cooldownMs)` dentro de `handleXpMessage`
antes/depois do `set`. Exporta `pruneCooldowns`.

Opcional (defesa extra): dar ao `SpamTracker` uma poda análoga por `lastUpdate` para
estados ociosos — só se for barato; senão o Passo 1 já resolve o caso principal.

**Verify**: `npm run typecheck` → exit 0.

### Passo 3: teste da poda

Em `tests/` (podes juntar a `tests/spam.test.ts` ou `tests/community.test.ts`):
- `pruneCooldowns`: com um Map abaixo do teto → não remove nada; acima do teto com
  entradas expiradas → remove as expiradas e mantém as recentes.

**Verify**: `npx vitest run` → todos passam, +1 teste.

## Done criteria

- [ ] `npm run typecheck` / `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] `grep -n "spamTracker.forget" src/index.ts` → presente
- [ ] `grep -n "pruneCooldowns" src/community/levels.ts` → presente e chamado
- [ ] Teste de `pruneCooldowns` passa
- [ ] `plans/README.md` linha 005 atualizada

## STOP conditions

- O handler `GuildMemberRemove` ou o bloco de cooldown não correspondem aos excertos → STOP.
- Se `SpamTracker` já não tiver `forget` → STOP e reporta.

## Notas de manutenção

- Se um dia se adicionar XP de voz, o mesmo padrão de cooldown/poda deve cobri-lo.
- O teto de 10 000 é heurístico; ajustável sem risco.
