# Plano — Redesenho visual das Definições do painel

> ✅ EXECUTADO (2026-07-14, Opus). Todas as fases feitas e no ar. Só `site/index.html`
> (front-end); zero backend. Verificado no browser (chips multi sem Ctrl, dropdown com
> pesquisa+teclado, barra Guardar/Descartar, save mockado com 2 PATCHes, mobile sem
> overflow, consola limpa). `cleanChannelName` com fallback a nomes só-emoji.

> Planeado com Fable 5 (2026-07-14). Execução: Opus. Só o front-end do painel
> (`vozen-helper-bot/site/index.html`) — a API já dá tudo o que é preciso
> (`/api/flags`, `/api/channels`, `/api/channel-settings`); **zero alterações de backend**.

## Objetivo

Tornar as Definições do painel agradáveis e óbvias de usar: (1) o "Excluir do XP" passa a
escolha múltipla com **um clique = marcar/desmarcar** (chips), matando o Ctrl+clique do
`<select multiple>` nativo; (2) settings agrupadas por área com nomes de canais limpos,
feedback de gravação e responsivo.

**Assunção:** mantém-se página única com JS vanilla inline (sem framework, sem build).

## Scope

### In
- Chips clicáveis no `xp.exclude` (multi) e dropdown custom com **pesquisa** nos restantes.
- Limpeza dos nomes de canal decorados (`₊˚ʚ💎୧﹕joins` → `#joins`).
- Agrupamento: **Moderação** e **Comunidade**, com o canal de cada feature junto ao toggle.
- **Modelo "pendente + Guardar"**: alterações acumulam localmente (marcadas como
  pendentes); uma **barra de guardar** fixa aparece com "Guardar alterações" e "Descartar".
  Só o Guardar envia os PATCHes (em série) para a API.
- Estados: pendente (por linha) / a guardar (barra) / guardado (✓) / erro parcial
  (indica quais falharam e mantém-nas pendentes).
- Responsivo < 720px e acessibilidade (foco, teclado, ≥44px).

### Out
- Alterações à API/bot; novas settings; frameworks/bundlers; refazer login/stats/casos;
  tema claro; endpoint de gravação em lote no backend (o Guardar envia os PATCHes
  existentes um a um).

## Fases

### Fase 1 — Chips multi-select + nomes limpos (a dor real)
Deliverable: `xp.exclude` com chips de canais clicáveis; nomes legíveis em todo o painel.
Dep.: nenhuma.
- [ ] `cleanChannelName(name)`: remove decorativos unicode/emoji, devolve `nome` legível;
  fallback ao nome original se ficar vazio. Aplicar em TODAS as UIs de canal.
- [ ] Substituir o `<select multiple>` por uma grelha de chips (checkbox visual): um clique
  alterna o estado LOCAL (pendente) — não grava logo.
- [ ] **Estado pendente + barra de guardar** (mínimo viável já nesta fase): `pending`
  (mapa key→valor novo), barra fixa em baixo com "Guardar alterações (N)" e "Descartar";
  Guardar envia os PATCHes em série e limpa os que passaram.
- **Done:** clicar em 3 chips seguidos marca os 3 (sem Ctrl) e a barra mostra "(1)";
  Guardar → a BD recebe o CSV certo; Descartar → volta ao estado do servidor; os nomes
  aparecem como `#joins`, não `₊˚ʚ💎୧﹕joins`.

### Fase 2 — Agrupamento por área
Deliverable: secção única "Definições" com dois grupos-card. Dep.: Fase 1.
- [ ] **Moderação**: anti-spam, anti-scam, anti-nuke, join gate, anti-raid + Canal de logs.
- [ ] **Comunidade**: níveis (toggle + canal de anúncio + excluir XP), starboard (toggle +
  canal), sugestões (toggle + canal), tickets, bump reminder.
- [ ] Cabeçalho de grupo (eyebrow + título); cada feature é uma linha com toggle à direita
  e a(s) opção(ões) de canal indentada(s) por baixo quando existirem.
- **Done:** as 10 flags e as 5 definições de canal aparecem TODAS, nos grupos certos, sem
  duplicados; desligar um toggle esbate (opacity) as opções de canal dessa feature.

### Fase 3 — Dropdown custom com pesquisa + feedback
Deliverable: seletor de canal próprio (botão → painel com input de pesquisa + lista).
Dep.: Fase 2 (vive no novo layout).
- [ ] Dropdown: fecha com Esc/clique fora; setas + Enter no teclado; opções "— default —"
  e "Onde a pessoa falou" (só levelup) no topo.
- [ ] Pesquisa filtra por nome limpo (case/emoji-insensitive).
- [ ] Toggles e dropdowns passam TODOS pelo modelo pendente (nenhum controlo grava direto);
  linha pendente ganha marca visual (ponto âmbar + valor antigo → novo).
- [ ] Barra de guardar: "A guardar… (i/N)" durante o envio → "✓ Guardado" 2s → esconde.
  Erro parcial: mantém os falhados pendentes + banner com quais falharam.
- **Done:** escrever "jo" filtra para `#joins`; mudar 2 toggles + 1 canal mostra "(3)" na
  barra; Guardar com API em baixo mantém os 3 pendentes e mostra o erro.

### Fase 4 — Responsivo + a11y + polish
Deliverable: painel utilizável em mobile e por teclado. Dep.: Fase 3.
- [ ] < 720px: linhas empilham (label em cima, controlo em baixo, largura total); chips
  quebram linha; dropdown ocupa a largura do cartão.
- [ ] Foco visível em chips/toggles/dropdown; alvos ≥44px; contraste AA nos novos estados.
- **Done:** a 375px não há scroll horizontal e tudo se opera por teclado (Tab/Enter/Esc).

## UI Blueprint

- **Direção visual:** a existente — dark neon, vidro, nítido, compacto ("mission control").
  Sem direção nova; consistência com o resto do painel é o objetivo.
- **Tokens:** reutilizar os do painel (`--panel`, `--line`, `--aqua #2ee6c8`,
  `--blurple #6672ff`, `--grad`, `--red #ff5c72`, Unbounded/Outfit/JetBrains Mono,
  radius 16/22px). Novo apenas: espaçamento de chips (gap 8px) e altura de linha de
  definição (min 52px). **Nenhuma cor nova.**
- **Componentes (primitivos → compostos):**
  - *Chip de canal*: default / selecionado (fundo grad suave + borda aqua) / hover /
    focus-visible / disabled(saving).
  - *Dropdown de canal*: fechado / aberto / a pesquisar / vazio ("sem resultados") /
    disabled(saving).
  - *Linha de definição*: default / **pendente** (ponto âmbar) / erro / esbatida (feature off).
  - *Barra de guardar*: escondida / visível "Guardar alterações (N)" + "Descartar" /
    a guardar (progresso i/N) / guardado (✓ 2s) / erro parcial.
  - *Grupo-card*: título + linhas; sem estado próprio.
- **Layout:** desktop-first (o painel usa-se sobretudo no PC), 1 coluna máx. 1080px;
  breakpoint único a 720px (empilhar).
- **Fluxos críticos:** (1) excluir 3 canais do XP em 3 cliques **+ Guardar**; (2) mudar o
  canal de logs via pesquisa + Guardar; (3) mexer em 3 coisas e **Descartar** volta tudo ao
  estado do servidor; (4) desligar "Níveis" e ver as opções dele esbatidas.
- **Acessibilidade:** AA sobre o fundo escuro; `:focus-visible` aqua; chips/toggles ≥44px
  de alvo; dropdown com `role="listbox"`/`aria-expanded`.
- **Ordem de implementação:** tokens (já existem) → primitivos (chip, dropdown) →
  compostos (linha, grupo) → página (reordenar secções) → polish. As fases refletem isto.

## Riscos

- **Nomes de canal 100% decorativos** (ex.: só emojis) podem ficar vazios após limpeza →
  fallback obrigatório ao nome original (Fase 1, onde se descobre barato).
- **Dropdown custom é o maior pedaço de JS** da página; teclado/fecho mal feitos irritam
  mais do que o select nativo. Mitigação: Fase 3 isolada, com "done" de teclado explícito.
- **Alterações pendentes perdem-se ao fechar/refrescar a página** (vivem só em memória).
  Mitigação: aviso `beforeunload` quando há pendentes. Persistir rascunho fica fora de âmbito.
- **Erro parcial no Guardar** (3 PATCHes, 1 falha) pode confundir; o "done" da Fase 3 exige
  que os falhados fiquem claramente pendentes e nomeados no banner.
- A página inline está a crescer; manter funções puras (ex.: `cleanChannelName`) separadas
  facilita um futuro split — mas o split em ficheiros fica FORA deste plano.

## MVP

Fim da **Fase 1**: chips clicáveis no "Excluir do XP" + nomes de canal limpos + **barra
"Guardar alterações"/"Descartar"** (mínima) — resolve a queixa real de hoje e instala o
modelo de gravação certo desde o início. O resto é qualidade visual incremental.

**Próxima ação concreta: implementar `cleanChannelName()` + chips no `xp.exclude` + a barra
de guardar mínima (pending → Guardar/Descartar) no `site/index.html` (Fase 1).**
