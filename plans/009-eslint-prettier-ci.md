# Plan 009: Adicionar ESLint + Prettier + CI

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): confirma `package.json` e ausência de configs vivos.

## Status

- **Prioridade**: P3 · **Esforço**: S · **Risco**: LOW · **Depende de**: nenhum
- **Categoria**: dx · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

O `CONTRIBUTING.md` impõe disciplina forte (TDD, comentários em PT, convenções tipadas) mas
não há tooling a fazê-la cumprir: não há `lint`/`format` scripts, nem ESLint/Prettier,
nem CI. O `strict: true` do tsc é a única rede — não apanha unused vars/imports nem
**floating promises**, e o `index.ts` está cheio do padrão `void handler(...)` que uma
regra `@typescript-eslint/no-floating-promises` validaria (garante que um `void`
deliberado não esconde uma promise realmente esquecida). Para um projeto onde agentes de
IA escrevem código, um linter é alta alavancagem. Um workflow de CI transforma a regra
manual "build+typecheck+test verdes antes de commitar" em garantia.

## Estado atual

- `package.json` scripts: `dev`, `build` (`tsc`), `start`, `start:prod`, `test`
  (`vitest run`), `typecheck` (`tsc -p tsconfig.test.json --noEmit`), `register`. **Sem**
  `lint`/`format`.
- Raiz **sem** `.eslintrc*` / `eslint.config.*` / `.prettierrc*` / `.editorconfig`.
- **Sem** `.github/workflows/`.
- Runtime: Node ≥ 22 (`engines` no `package.json`). ESM (`"type": "module"`).
- O projeto irmão `Vozen-bot` tem ESLint flat + prettier a funcionar
  (`bots-discord/Vozen-bot/package.json` e `eslint.config.*`) — usa como referência de
  versões/estilo para manter consistência entre os dois bots.

## Comandos

| Objetivo        | Comando                               | Esperado                       |
| --------------- | ------------------------------------- | ------------------------------ |
| Instalar        | `npm install`                         | exit 0                         |
| Lint (novo)     | `npm run lint`                        | exit 0 (após resolver achados) |
| Typecheck/Build | `npm run typecheck` / `npm run build` | exit 0                         |
| Testes          | `npx vitest run`                      | todos passam                   |

## Scope

**In scope:** `package.json` (devDeps + scripts), `eslint.config.js` (novo, flat),
`.prettierrc` (novo), `.editorconfig` (novo), `.github/workflows/ci.yml` (novo),
correções mínimas que o lint aponte.
**Out of scope:** refactors grandes motivados pelo lint — se o lint apontar muitos
problemas estruturais, corrige só os triviais (unused imports/vars, formatação) e
**reporta** o resto; não reescrever lógica. NÃO tocar em ficheiros de `src/` além do que
o autofix trivial exigir.

## Passos

### Passo 1: dependências e configs (espelhar o Vozen-bot)

Lê `bots-discord/Vozen-bot/package.json` e `bots-discord/Vozen-bot/eslint.config.*` e
replica as mesmas versões de `eslint`, `typescript-eslint`, `prettier`,
`eslint-config-prettier`. Cria `eslint.config.js` (flat) com `typescript-eslint`
recommended + **ativar** `@typescript-eslint/no-floating-promises` e
`no-misused-promises`, e `eslint-config-prettier` no fim. Cria `.prettierrc` e
`.editorconfig` coerentes com o estilo atual (2 espaços, aspas simples, ponto e vírgula).

Adiciona scripts:

```json
"lint": "eslint .",
"format": "prettier --write .",
"format:check": "prettier --check ."
```

**Verify**: `npm install` → exit 0.

### Passo 2: correr o lint e resolver o trivial

```
npm run lint
```

Corrige só o trivial: imports/vars não usados, e confirma que os `void handler(...)` do
`index.ts` passam a regra `no-floating-promises` (o `void` explícito é o padrão aceite;
se a regra sinalizar algum sítio SEM `void`, isso é um bug real de promise esquecida —
regista e reporta). Se houver muitos achados estruturais, resolve só os triviais e lista
o resto no fim (não STOP, mas não reescrevas lógica).

**Verify**: `npm run lint` → exit 0 (ou lista curta e justificada do que ficou por
resolver, se for estrutural).

### Passo 3: CI

Cria `.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: '22' }
      - run: npm ci
      - run: npm run lint
      - run: npm run typecheck
      - run: npm run build
      - run: npx vitest run
```

**Verify**: o YAML é válido (`node -e "require('js-yaml')" ` não aplicável — basta rever
a indentação); `npm run typecheck && npm run build && npx vitest run` continuam verdes
localmente.

## Done criteria

- [ ] `npm run lint` existe e sai 0 (ou com lista curta e justificada de exceções)
- [ ] `npm run format:check` existe
- [ ] `.github/workflows/ci.yml` presente e coerente
- [ ] `npm run typecheck` / `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] Nenhuma alteração de LÓGICA em `src/` (só formatação/unused); confirma na revisão
- [ ] `plans/README.md` linha 009 atualizada

## STOP conditions

- O lint aponta > ~30 problemas ou problemas que exigem reescrever lógica → resolve os
  triviais, para, e reporta a lista (não improvises refactors).
- `no-floating-promises` sinaliza uma promise SEM `void`/`await` num caminho crítico
  (moderação) → é um bug potencial: regista e reporta, não o mascares com `void` cego.

## Notas de manutenção

- CI é de graça em repo privado a esta escala. Se o repo migrar para git/GitHub (hoje
  **não é sequer um repo git**), o workflow começa a correr automaticamente.
- Manter as versões de ESLint/prettier alinhadas com o `Vozen-bot` para consistência
  entre os dois projetos.
