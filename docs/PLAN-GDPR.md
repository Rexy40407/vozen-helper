# Plano — Conformidade GDPR do Vozen Helper (site + API + bot)

> Planeado em 2026-07-15. ✅ EXECUTADO em 2026-07-15. Fases 1–4 feitas.
> Entregáveis: `docs/GDPR-INVENTARIO.md`; `site/privacidade.html` + link no gate (no ar);
> `src/store/gdpr.ts` (purga/export/apagar) ligado ao arranque e ao member-leave;
> comando `/privacidade dados|apagar`; `deploy/vozen-panel-logrotate.conf`.
> 9 testes novos em `tests/gdpr.test.ts`; suite 187 verdes; build + typecheck limpos.
> **Passos manuais no VPS** (não executáveis daqui): `npm run register` para publicar o
> comando `/privacidade`, e instalar o logrotate (instruções no .conf).

## Objetivo

Pôr o ecossistema Vozen Helper (site GitHub Pages + API no VPS + bot single-guild) em
conformidade prática com o GDPR: transparência sobre os dados tratados, minimização,
retenção definida e direitos dos titulares (acesso/apagamento) exequíveis.

**Enquadramento honesto:** o site quase não trata dados (login de 1 conta); o peso real
do GDPR está na **BD do bot** — casos, notas, infrações, quarentena, aniversários,
níveis/XP, stats por utilizador, votos, giveaways. Um plano "só do site" seria teatro
de conformidade. Tu és o responsável pelo tratamento (controller); Discord, GitHub
(Pages), Cloudflare (túnel) e o host do VPS são subcontratantes/terceiros a divulgar.

## Scope

### In

- Inventário de dados pessoais (registo de tratamento simplificado, art. 30.º).
- Página de **Política de Privacidade** no site (pt-PT) + link visível no gate.
- Retenção: purga automática de dados antigos na BD e rotação de logs no VPS.
- Direitos dos titulares: comandos `/privacidade dados` (exportar, art. 15.º) e
  `/privacidade apagar` (apagar, art. 17.º) no Discord, com exceções documentadas.
- Divulgação do cookie `vh_session` e do `localStorage` (estritamente necessários —
  sem banner de consentimento, mas com menção na política).

### Out

- Banner de cookies (não há trackers, analytics nem cookies não-essenciais — não é preciso).
- DPO, DPIA formal, representante na UE (escala não o exige).
- Contratos DPA formais com GitHub/Cloudflare/Discord (usam os termos standard deles; só divulgar).
- Encriptação da BD em repouso (fica como melhoria futura, não requisito).
- Tradução da política para inglês.

## Fases

### Fase 1 — Inventário de dados (registo de tratamento)

Deliverable: `docs/GDPR-INVENTARIO.md` — tabela: dado → onde vive (tabela BD / log /
localStorage / cookie) → finalidade → base legal → retenção proposta. Dep.: nenhuma.

- [ ] Mapear as ~20 tabelas da BD (`src/store/db.ts`) + `api.log` + logs do túnel +
      localStorage/cookie do painel.
- [ ] Classificar base legal por dado: **interesse legítimo** (moderação: cases, notes,
      infractions, quarantine, anti-raid), **consentimento por ato voluntário** (birthdays,
      AFK, lembretes, votos, giveaways), **interesse legítimo** (stats/levels — a validar).
- [ ] Propor retenção por categoria (ex.: cases 2 anos, stats 1 ano, api.log 30 dias).
- **Done:** cada dado pessoal identificado tem linha na tabela com os 5 campos preenchidos.

### Fase 2 — Política de privacidade no site

Deliverable: `site/privacidade.html` publicada + link no gate. Dep.: Fase 1 (a política
descreve o que o inventário apurou — sem inventário, a política mente).

- [ ] Redigir em pt-PT claro: quem é o responsável, que dados, para quê, base legal,
      retenção, com quem se partilha (Discord/GitHub/Cloudflare/VPS), direitos e como
      exercê-los (contacto Discord), cookie `vh_session` + localStorage.
- [ ] Página estática com os tokens visuais do painel; link "Privacidade" no rodapé do gate.
- [ ] Mencionar a política na descrição do bot / canal de regras do servidor (os membros
      do servidor são os titulares — têm de conseguir encontrá-la).
- **Done:** URL pública abre a política; gate tem link visível; membros conseguem chegar lá.

### Fase 3 — Retenção e minimização

Deliverable: purga automática na BD + rotação de logs. Dep.: Fase 1 (os prazos vêm do inventário).

- [ ] Job diário no bot: apagar/anonimizar registos além da retenção (cases antigos →
      manter contagem, apagar user_id? decidir na Fase 1; stats/levels de quem saiu do servidor).
- [ ] Rotação do `api.log` no VPS (logrotate ou truncagem no supervisor, 30 dias).
- [ ] Auditar o `api.log`: registar ações, não dados desnecessários.
- [ ] Testes (vitest) da função de purga: TDD como o resto do projeto.
- **Done:** inserir registo com timestamp antigo + correr purga → desaparece; log não cresce sem limite.

### Fase 4 — Direitos dos titulares

Deliverable: comandos Discord de exportação e apagamento. Dep.: Fases 1 e 3.

- [ ] `/privacidade dados` — DM com JSON de tudo o que a BD tem sobre o requerente.
- [ ] `/privacidade apagar` — apaga dados voluntários (birthday, AFK, lembretes, votos,
      XP…) com confirmação; **recusa fundamentada** para registos de moderação ativos
      (interesse legítimo prevalece — art. 17.º/3) com resposta clara a dizê-lo.
- [ ] Registar pedidos de apagamento (data + user) para prova de cumprimento.
- [ ] Testes de ambos os comandos.
- **Done:** conta de teste recebe o seu JSON completo; após apagar, `/privacidade dados`
  devolve vazio exceto casos de moderação, que aparecem com a justificação.

## UI Blueprint (confinado à Fase 2 — página estática de política)

- **Direção visual:** a existente do painel — dark neon, vidro, compacto. Nada novo.
- **Tokens:** reutilizar (`--panel`, `--line`, `--aqua #2ee6c8`, `--blurple #6672ff`,
  Unbounded/Outfit, radius 16/22px). Nenhuma cor nova.
- **Componentes:** página de texto única (h1/h2/p/listas) + link de rodapé no gate
  (estados: default/hover/focus-visible). Sem interações.
- **Layout:** coluna única máx. 720px, tipografia legível (16px+, line-height 1.6);
  responsivo herdado.
- **Acessibilidade:** contraste AA, foco visível no link — igual ao resto do site.

## Riscos

- **Isenção doméstica é ambígua:** um servidor Discord privado _pode_ cair fora do GDPR
  (uso pessoal, art. 2.º/2-c), mas a jurisprudência trata comunidades com membros como
  tratamento real. Assumimos que o GDPR se aplica — se não se aplicar, o trabalho fica
  a mais, nunca a menos. Custo baixo, risco eliminado.
- **Apagar casos de moderação a pedido** esvaziaria o propósito do bot; a recusa com base
  no interesse legítimo é defensável mas tem de estar **escrita na política** (Fase 2)
  antes de o comando existir (Fase 4).
- **URL da API é efémero** (trycloudflare) — a política não deve citar o URL, só
  "API própria via túnel Cloudflare".
- **Stats/levels de ex-membros** são o dado mais difícil de justificar reter; a decisão
  (apagar à saída vs. reter 1 ano) toma-se barata na Fase 1.
- A página do repo é pública: a política **não** pode expor IDs, nomes de canais internos
  nem detalhes de segurança.

## MVP

Fim da **Fase 2**: inventário feito + política de privacidade publicada e alcançável
pelos membros. É o mínimo que muda a situação legal (transparência); retenção e comandos
são conformidade material que se segue.

**Próxima ação concreta: escrever `docs/GDPR-INVENTARIO.md` mapeando as ~20 tabelas de
`src/store/db.ts` com dado → local → finalidade → base legal → retenção (Fase 1).**
