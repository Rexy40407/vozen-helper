# Plan 007: Forçar um `undici` sem o advisory *high* (via `overrides`)

> **Instruções do executor**: passo a passo, verifica, STOP nas condições, atualiza o
> README. **Drift check** (sem git): confirma o `package.json` vivo antes de editar.

## Status
- **Prioridade**: P2 · **Esforço**: S · **Risco**: LOW/MED · **Depende de**: nenhum
- **Categoria**: dependencies · **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

`npm audit` reporta 1 advisory *high* + 3 *moderate*, todos no `undici` (`<=6.26.0`)
trazido transitivamente por `discord.js` (`@discordjs/rest` → `@discordjs/ws`). O
*high* é HTTP header injection via `Set-Cookie` (GHSA-p88m-4jfj-68fv). O `undici` é o
cliente HTTP em runtime do discord.js (REST + gateway), logo é código alcançável. O
alcance prático é limitado — um grep a `src/` confirma que o bot só faz pedidos a
endpoints do Discord (não a URLs controladas por atacantes; o resolvedor de redirects de
phishing referido em comentários NÃO está implementado) — mas o advisor recomenda
corrigir uma dependência de runtime central na mesma.

## Estado atual

- `package.json` — `discord.js: ^14.18.0` nas `dependencies`; NÃO existe bloco
  `overrides`. Confirma com:
  ```
  npm audit            # → mostra o high + moderates no undici
  cat package.json     # → sem "overrides"
  ```
- **NÃO correr `npm audit fix --force`**: ele "resolve" fazendo downgrade de
  `discord.js` para v13 — mudança disruptiva e retrocesso. O projeto irmão `Vozen-bot`
  usa exatamente o padrão de `overrides` para CVEs transitivos sem breaking change
  (ver `bots-discord/Vozen-bot/package.json`, bloco `overrides` + comentário
  `//overrides` como referência de estilo).

## Comandos

| Objetivo | Comando | Esperado |
|-----------|---------|----------|
| Ver advisories | `npm audit` | lista atual (antes) |
| Reinstalar | `npm install` | exit 0, atualiza lockfile |
| Re-auditar | `npm audit` | **0** high (depois) |
| Typecheck/Build | `npm run typecheck` / `npm run build` | exit 0 |
| Testes | `npx vitest run` | todos passam |

## Scope

**In scope:** `package.json` (bloco `overrides`), `package-lock.json` (gerado pelo
`npm install`).
**Out of scope:** subir a major do `discord.js`; qualquer alteração de código-fonte.

## Passos

### Passo 1: descobrir a versão corrigida do undici

Corre `npm audit` e lê a versão-alvo recomendada para o `undici` (a primeira 6.x sem os
advisories; à data, `>=6.26.1` — confirma no output). NÃO inventes a versão; usa a que o
audit indica como fixed.

**Verify**: `npm audit` mostra a versão fixed do undici.

### Passo 2: adicionar `overrides` ao `package.json`

Acrescenta ao `package.json` (top-level, ao lado de `dependencies`) — ajusta a versão à
que o audit indicou:
```json
"overrides": {
  "undici": "^6.26.1"
}
```
(Comentário opcional numa chave `"//overrides"` a explicar, como no Vozen-bot.)

### Passo 3: reinstalar e re-auditar

```
npm install
npm audit
```
**Verify**: `npm audit` → **0 vulnerabilidades high** (os moderates podem baixar também;
o objetivo mínimo é eliminar o high). Se ainda houver high, STOP.

### Passo 4: confirmar que nada partiu

```
npm run typecheck && npm run build && npx vitest run
```
**Verify**: todos exit 0 / todos os testes passam. O `undici` é interno ao discord.js,
por isso não deve haver mudanças de tipos no nosso código.

## Done criteria

- [ ] `package.json` tem `overrides.undici`
- [ ] `npm audit` → 0 high (idealmente 0 no total)
- [ ] `npm run typecheck` / `npm run build` exit 0 · `npx vitest run` todos passam
- [ ] `package-lock.json` atualizado (o `undici` resolvido é a versão forçada)
- [ ] `plans/README.md` linha 007 atualizada

## STOP conditions

- `npm install` falha ou o override não resolve o high (peer conflicts do discord.js) → STOP e reporta.
- Algum teste ou o build passa a falhar depois do override → STOP (reverter o override e reportar).
- Se a versão fixed do undici for uma major diferente (>=7) que o discord.js não aceita → STOP.

## Notas de manutenção

- Cada bump de `discord.js` pode tornar o override redundante — reverificar `npm audit`
  nesse momento e remover o override se já não for preciso.
- **Nota de deploy**: no VPS, depois de aprovado, é preciso `npm install` (ou `npm ci`
  com o novo lockfile) além do rebuild habitual — o override altera dependências.
