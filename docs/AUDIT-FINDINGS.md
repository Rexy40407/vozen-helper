# Vozen-helper — Auditoria intensiva (findings)

## ✅ CORRIGIDO (2026-07-15, TDD, suite 189 verde)

- **CS-P2-1** starboard race → lock por-mensagem (`keyedLock.ts` novo + `starboard.ts`); teste.
- **CS-P2-2** `/suggestion` sem defer → `deferReply` + `editReply` (`suggestions.ts`).
- **CS-P3-3** ticket open double-click → lock por (guild,user) (`tickets.ts`).
- **CS-P3-5** ticket close não idempotente → guard already-closed (`tickets.ts`).
- **CS-P3-7** voto apagava "Resposta da staff" → preserva do embed (`suggestions.ts`).
- **CS-P3-8** `greroll` sem check `ended` → guard (`giveaways.ts`).
- **SEC-P3-b** logout enchia Set em memória → verify-before-revoke (`api/server.ts`).

## ⏳ POR FAZER

- **SEC-P2** cookie sem expiry/revogação (meter token assinado no cookie, ou dropar o cookie).
- **SEC-P3-a** rate-limit bucket global (trustProxy/keyGenerator) · **SEC-P3-c** segredo reutilizado
  · **SEC-P3-e** OAuth cross-app · **SEC-P3-f** CSRF implícito.
- **CS-P3-6** contagem starboard satura ~100 (cosmético) · **CS-P3-9** gaps de purga.
- **Remoção (precisa OK do dono):** birthdays dead code (CS-P3-4), `renderWelcome` (CS-P3-11).
- **Re-correr:** 2 agentes que falharam por limite de sessão (moderação/eventos; cobertura/docs).

---

> Auditoria de 2026-07-15. 4 agentes read-only; 2 concluíram (segurança do painel API,
> comunidade+store), 2 falharam por limite de sessão (moderação/eventos, cobertura/ficheiros)
> — **a re-correr**. Baseline: build/typecheck/lint 0, 188 testes, `npm audit` 0 CVEs.
> Sem tradução (mantém pt-PT, por decisão do dono). Repo NÃO é git — fixes vão direto aos
> ficheiros, suite de testes como rede. Remoções de ficheiros só com OK do dono.

## Segurança — Painel API (concluído)

Veredicto: **0 P1.** Núcleo de auth sólido (sem bypass, sem token forjável, sem SQLi, sem
escrita arbitrária de settings). 1 P2 + 6 P3.

- **SEC-P2** (medium): cookie de sessão assinado **sem expiry server-side nem revogação**
  (`server.ts:95-102,136-140,118-123`). Valor = só userId assinado; guard aceita por
  assinatura+match, sem timestamp. Logout só limpa client-side + revoga o Bearer, não o
  cookie. Um cookie copiado vale até rotação do segredo. Fix: meter o token assinado
  (com exp) no cookie e verificá-lo como o Bearer — ou dropar o cookie (Brave bloqueia-o).
- **SEC-P3-a** (high): rate-limit num único bucket global (`server.ts:74`, sem `trustProxy`)
  → `req.ip`=127.0.0.1 p/ todo o tráfego atrás do túnel → 120/min esgota e tranca o dono (DoS).
- **SEC-P3-b** (medium): `/api/logout` sem `guard` (`server.ts:118-123`) alimenta um Set
  `revoked` em memória sem cap com string atacante-controlada → DoS lento de memória. Fix:
  só revogar se o token for válido (verify-before-revoke) — bounda e fecha.
- **SEC-P3-c** (medium): segredo reutilizado p/ cookie signer + HMAC token (`server.ts:67,105`).
  Derivar 2 subchaves. Não explorável, hardening.
- **SEC-P3-d** (high, aceite): revogação só em memória — não sobrevive a restart (documentado).
- **SEC-P3-e** (high): OAuth verifica dono mas não a app/scope (`discordAuth.ts:30-37`) — token
  de qualquer app que o dono já autorizou passa (phishing-gated). Baixo.
- **SEC-P3-f** (medium): CSRF nas escritas é implícito (SameSite=None + CORS + JSON preflight),
  não explícito — frágil a mudanças futuras (ex.: registar formbody). Considerar Origin check.

## Comunidade + Store (concluído)

2 P2 + vários P3. Verificados CORRETOS (não mexer): leveling (síncrono, upsert atómico),
scheduler (reentrância + idempotente), vote dedup (PK), giveaway entries, migrações
append-only, cache TTL 4s, GDPR erase (sem cache user-keyed), export (omite notes/moderator_id).

- **CS-P2-1** (high): **race read-modify-write no starboard** (`starboard.ts:46,67-75`, dispatch
  fire-and-forget `index.ts:154-155`). Secção crítica atravessa `await board.send`. 2 ⭐ quase
  simultâneas → 2 posts no starboard, DB só referencia 1 → post órfão para sempre + drift.
  Fix: guard de in-flight por original_message_id (ou lock).
- **CS-P2-2** (medium): `/suggestion` staff **sem defer** (`suggestions.ts:94-143`) — ack só
  após ~5 awaits sequenciais (fetch canal/msg/user + edit + DM). Pode estourar os 3s →
  "Unknown interaction". Fix: `deferReply` no início.
- **CS-P3-3** (high mech): ticket open double-click race (`tickets.ts:59-88`) → 2 threads + 2 rows.
- **CS-P3-4** (high): **birthdays é dead code** — `setBirthday/clearBirthday/getBirthdaysOn`
  (`store.ts:151-179`) sem call sites, sem comando registado, branch `birthday_role_remove`
  do scheduler inalcançável, `dist/community/birthdays.js` stale sem source. Candidato a limpeza.
- **CS-P3-5** (high): ticket close não idempotente (`tickets.ts:102-121`) — 2º clique "Fechar"
  reposta transcript de 100 msgs + re-lock. Fix: guard already-closed.
- **CS-P3-6** (high, cosmético): contagem do starboard satura em ~100 (`starboard.ts:38-40`,
  `users.fetch()` não paginado).
- **CS-P3-7** (medium): voto de sugestão apaga o campo "Resposta da staff" (`suggestions.ts:146-159`
  `buildEmbed` sem `reason`). Fix: passar reason.
- **CS-P3-8** (high logic, low impact): `greroll` não checa `gw.ended` (`giveaways.ts:124-140`)
  → reroll de giveaway ativo. Fix: guard ended.
- **CS-P3-9** (low): `purgeExpired` não toca starboard/self_roles/tags/settings órfãos
  (`gdpr.ts:35-107`); `deleteUserData` deixa votos de outros órfãos até à sweep diária.
- **CS-P3-11** (high): `renderWelcome` dead code (`text.ts:4-12`, nunca chamado).

## Falharam (re-correr quando reset)

- Moderação/eventos (correção) — a25515d4: ia ler funções-folha que processam input não confiável.
- Cobertura/tech-debt/ficheiros/docs — a0b5e38506: ia verificar abertura da DB (comment "read-only"
  mas painel escreve) + base de retenção dos tickets.

## Deps

`npm audit` 0. Outdated (majors sem CVE): better-sqlite3 12, @fastify/rate-limit 11, dotenv 17,
typescript 7, vitest 4, @types/node 26. Baixa prioridade (risco de breakage > ganho).
