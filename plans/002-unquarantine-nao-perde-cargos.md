# Plan 002: `unquarantine` deixa de apagar o registo quando a reposição de cargos falha

> **Instruções do executor**: segue passo a passo; verifica cada comando; STOP nas
> condições listadas; atualiza `plans/README.md` no fim.
>
> **Drift check**: sem git — compara os excertos de "Estado atual" com o código vivo
> antes de editar; mismatch = STOP.

## Status

- **Prioridade**: P1
- **Esforço**: S
- **Risco**: LOW
- **Depende de**: nenhum
- **Categoria**: bug
- **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

A quarentena do anti-nuke remove **todos** os cargos de uma conta suspeita e guarda-os
para restauro — o design está declarado como "reversível de propósito"
(`quarantineService.ts`, topo do ficheiro). Mas `unquarantineMember` faz o
`member.roles.add(...)` com `.catch(() => undefined)` (engole o erro) e a seguir chama
`clearQuarantine` **incondicionalmente**. Se a reposição falhar (hierarquia, rate
limit, permissões), a exceção é engolida e o registo com os cargos guardados é apagado
à mesma → a lista desaparece **permanentemente** e é impossível restaurar. É
precisamente na emergência (um mod comprometido, muitas ações a competir) que o
`add` tem mais probabilidade de falhar e onde a perda dói mais.

## Estado atual

`src/moderation/quarantineService.ts`, função `unquarantineMember` (~linhas 47-67):

```ts
export async function unquarantineMember(
  ctx: AppContext,
  guild: Guild,
  member: GuildMember,
): Promise<boolean> {
  const saved = getQuarantine(ctx.db, guild.id, member.id);
  if (!saved) return false;
  const valid = saved.roleIds.filter((r) => guild.roles.cache.has(r) && r !== guild.id);
  if (valid.length) await member.roles.add(valid, 'Fim da quarentena').catch(() => undefined);
  clearQuarantine(ctx.db, guild.id, member.id); // <-- corre mesmo que o add falhe
  recordCase(ctx, {/* ... type: 'unquarantine' ... */});
  return true;
}
```

Store relevante (`src/store/quarantine.ts`): `getQuarantine`, `clearQuarantine`
existem e são síncronas.

Padrão de logging do repo: `import { log } from '../log.js';` e `log.error('...', err)`
(ver outros ficheiros de `moderation/`). `alertOwner(ctx, guild, msg)` existe no mesmo
`quarantineService.ts` e é a forma de avisar a staff.

## Comandos que vais precisar

| Objetivo  | Comando             | Esperado     |
| --------- | ------------------- | ------------ |
| Typecheck | `npm run typecheck` | exit 0       |
| Build     | `npm run build`     | exit 0       |
| Testes    | `npx vitest run`    | todos passam |

## Scope

**In scope:**

- `src/moderation/quarantineService.ts`
- `tests/` — novo teste (ver Test plan)

**Out of scope:**

- `src/store/quarantine.ts` — o store está correto; não alterar.
- O caminho de `quarantineMember` (aplicar quarentena) — só o restauro é o alvo.

## Passos

### Passo 1: só limpar o registo se a reposição teve sucesso

Reescreve `unquarantineMember` para: tentar o `add` num try/catch REAL; se falhar,
**não** apagar o registo, alertar o dono e devolver `false`; só em sucesso chamar
`clearQuarantine` + `recordCase` + devolver `true`. Se `valid.length === 0` (não há
cargos válidos a repor) considera sucesso (limpa o registo e regista o caso).

Forma alvo (mantém comentários em PT):

```ts
const valid = saved.roleIds.filter((r) => guild.roles.cache.has(r) && r !== guild.id);
if (valid.length) {
  try {
    await member.roles.add(valid, 'Fim da quarentena');
  } catch (err) {
    log.error('Falha a repor cargos na unquarantine:', err);
    await alertOwner(
      ctx,
      guild,
      `⚠️ Não consegui repor os cargos de <@${member.id}> — quarentena mantida.`,
    );
    return false;
  }
}
clearQuarantine(ctx.db, guild.id, member.id);
recordCase(ctx, {/* inalterado */});
return true;
```

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: teste

`unquarantineMember` usa objetos do discord.js (`GuildMember`), por isso o teste usa um
**stub mínimo**. Cria `tests/quarantine.test.ts` (DB `:memory:` via `initDb`) e:

1. `saveQuarantine(db, g, u, ['r1','r2'], 'nuke', now)`.
2. Chama `unquarantineMember` com um `ctx` mínimo (`{ db, env:{guildId}, modConfig, client }`
   com o `client` stubado o suficiente) e um `member` fake cujo `roles.add` **rejeita**
   → assert: devolve `false` E `isQuarantined(db, g, u) === true` (registo mantido).
3. Repete com `roles.add` que **resolve** → assert: devolve `true` E
   `isQuarantined === false`.

Se o stub do `ctx.client`/`alertOwner` for demasiado pesado, extrai a decisão testável:
uma função que, dado "add falhou?" (boolean), decide se limpa — e testa essa. Modela o
estilo por `tests/antinuke.test.ts` (que já usa o store de quarentena).

**Verify**: `npx vitest run` → todos passam, +≥2 asserts novos.

## Done criteria

- [ ] `npm run typecheck` exit 0
- [ ] `npm run build` exit 0
- [ ] `npx vitest run` — todos passam; teste novo cobre falha-mantém-registo e sucesso-limpa
- [ ] `grep -n "clearQuarantine" src/moderation/quarantineService.ts` mostra a chamada só DEPOIS do add bem-sucedido (não antes/incondicional)
- [ ] `plans/README.md` linha 002 atualizada

## STOP conditions

- O corpo de `unquarantineMember` não corresponde ao excerto → STOP.
- `alertOwner` ou `clearQuarantine` já não existem/mudaram de assinatura → STOP e reporta.

## Notas de manutenção

- Mesmo padrão (não-limpar-em-falha) aplica-se conceptualmente ao restauro de sticky
  roles em `src/events/memberGate.ts` (~linha 38: `clearStickyRoles` corre após um
  `add` best-effort), mas aí é one-shot no rejoin e de severidade menor — deixado como
  follow-up, fora deste plano.
- Um reviewer deve confirmar que o caminho de sucesso continua a registar o caso
  `unquarantine` (para o `/modlogs`).
