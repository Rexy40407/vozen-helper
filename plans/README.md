# Implementation Plans — Vozen Helper

Gerado pela skill `improve` (advisor: Fable 5) em 2026-07-14. Executor previsto: **Opus**.
Auditoria de nível `standard` sobre `src/` (correctness, segurança, testes, tech-debt, DX).

> **Nota de contexto para o executor**: este projeto **não é um repositório git**
> (não há SHA para drift-check). Em vez do `git diff`, cada plano traz excertos do
> "estado atual" — compara-os com o código vivo antes de agir; se não baterem, é
> STOP. Os comandos de verificação do repo são: `npm run build` (tsc), `npm run
> typecheck`, `npx vitest run` (correr a partir de
> `C:\Users\diogo\Videos\second brain\bots-discord\Vozen-helper`). Comentários de
> código em **português** (convenção do `CLAUDE.md`). TDD: teste primeiro quando
> aplicável. **Deploy** (fazer UMA vez no fim, fora dos planos): o bot corre num VPS;
> após tudo verde, transferir `src` e reiniciar — ver `docs/SETUP.md`.

## Ordem de execução & estado

| Plano | Título | Prioridade | Esforço | Depende de | Estado |
|------|-------|----------|--------|------------|--------|
| 001 | Restringir menções nos lembretes (`/remind`) | P1 | S | — | DONE |
| 002 | Não perder cargos quando `unquarantine` falha | P1 | S | — | DONE |
| 003 | `/gend` só cancela o auto-fim do giveaway certo | P1 | S | — | DONE |
| 004 | Guard de reentrância no scheduler (lembretes duplicados) | P2 | S | — | DONE |
| 005 | Fechar leaks de memória (SpamTracker + cooldowns de XP) | P2 | S | — | DONE |
| 006 | `/rolepanel` respeita a hierarquia do moderador | P2 | S | — | DONE |
| 007 | Forçar `undici` corrigido via `overrides` | P2 | S | — | DONE |
| 008 | Testar as funções sem cobertura de `community/store.ts` | P2 | S | — | DONE |
| 009 | Adicionar ESLint + Prettier + CI | P3 | S | — | DONE |

**Executado em 2026-07-14**: os 9 planos aplicados; build + typecheck + lint + 127 testes
verdes; `npm audit` → 0 vulnerabilidades. Deployed no VPS.

Valores de estado: TODO | IN PROGRESS | DONE | BLOCKED (motivo) | REJECTED (motivo).

## Notas de dependências

- Nenhum plano depende de outro; podem correr em qualquer ordem. Sugestão: fazer os
  P1 (001–003) primeiro (bugs com impacto direto), depois P2, depois 009.
- **008 antes de qualquer refactor futuro de `community/store.ts`** (o split DEBT-01,
  não planeado aqui, deve esperar por estes testes como rede de segurança).
- Fazer **um único deploy no fim**, depois de todos os planos escolhidos estarem
  verdes localmente — evita reiniciar o bot em produção a cada plano.

## Findings considerados e NÃO planeados (para não re-auditar)

- **PERF-01** (fetches de membro redundantes por mensagem): impacto só quando o membro
  não está em cache; a esta escala single-guild não compensa — deixado como micro-opt.
- **C-03 / C-06** (starboard duplicado sob race; strike em dobro quando conteúdo é
  proibido E scam na mesma mensagem): reais mas dependentes de timing e de baixa
  frequência; MED-confidence. Vale um plano futuro se se observarem na prática; não
  incluídos agora para manter a lista de alta alavancagem.
- **DEBT-01** (partir `community/store.ts` por domínio): refactor mecânico de valor
  médio; fazer só DEPOIS do plano 008. Não planeado agora.
- **DOCS-01** (README não menciona features de comunidade): baixo custo/baixo impacto;
  incluir na próxima ronda de docs.
- `leveling.ts` vs `levels.ts`: **não** é duplicação (matemática pura vs handler).
  `.env.example` está completo. Migrações já têm testes. `any`/`@ts-ignore`: zero.
