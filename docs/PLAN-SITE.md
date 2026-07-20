# Plano — Site do Vozen Helper (GitHub Pages, público)

> Refeito em 2026-07-14. Site estático simples,
> publicado no GitHub Pages num URL `github.io` até haver domínio — o mesmo método do
> site do Vozen, mas mais enxuto (sem player de voz, pricing, i18n nem backend).

## Objetivo

Uma **landing de uma página** que apresenta o Vozen Helper (bot de moderação +
comunidade), com botão de convite/suporte e as páginas legais que um bot de Discord
deve ter (privacidade + termos). Online já, público, grátis, pronto para receber um
domínio depois.

## Decisão de host (fechada)

**GitHub Pages, repo público, URL `github.io`.** O Diogo escolheu público. O site é
visível a quem tiver o link (obscuro, não indexado até ser partilhado) — não é "só eu
vejo", e isso está assumido. Repo público = Pages grátis e simples.

## Scope

### In

- `site/` estático: `index.html` + `privacy.html` + `terms.html` + `css/` + `favicon.svg`.
- Design alinhado com a marca Vozen (mesmos tokens de cor/tipografia).
- Repo GitHub novo `vozen-helper-bot` (público) + workflow de deploy para o Pages.
- `CNAME` pronto (vazio) para o domínio futuro.

### Out

- Backend/API, painel, contas, login (o Helper não tem API — é um bot de 1 servidor).
- i18n, blog, player de áudio, tabela de preços (o Helper é grátis/privado, sem venda).
- Comprar/configurar domínio agora.
- Publicar o CÓDIGO do bot (o repo do site é só o site; o bot fica onde está, privado).

## UI Blueprint (tokens reais, reaproveitados do Vozen)

- **Tema:** escuro por defeito. Fundo `--panel: #10121f`; vidro `rgba(22,25,41,.72)`;
  linhas `rgba(255,255,255,.08)`.
- **Cores de marca:** blurple `#6672ff`, aqua `#2ee6c8`; gradiente principal
  `linear-gradient(115deg,#6672ff,#2ee6c8)`. Acentos: âmbar `#ffc24b`, rosa `#ff6ea9`.
- **Tipografia:** display **Unbounded**, corpo **Outfit**, mono **JetBrains Mono**
  (Google Fonts, como no Vozen).
- **Layout:** largura máx `1160px`, raio `22px`, nav fixa `70px`.
- **Componentes/estados:** hero com título em gradiente + 2 botões (primário "Adicionar
  ao servidor" com hover `translateY(-2px)`; ghost "Servidor de suporte"); cards de
  feature (hover: borda aqua + seta). Responsivo: 1 coluna < 720px.
- **Acessibilidade:** contraste AA sobre o fundo escuro, foco visível, `prefers-reduced-motion`
  respeitado nas animações de reveal.

## Árvore de ficheiros alvo (no repo do site)

```
vozen-helper-bot/
  site/
    index.html          # landing
    privacy.html        # política de privacidade (adaptada ao Helper)
    terms.html          # termos
    favicon.svg
    css/styles.css      # tokens + layout (1 ficheiro chega)
    CNAME               # vazio até haver domínio
  tools/minify-site.mjs # site/ -> site-dist/ (portado do Vozen)
  .github/workflows/pages.yml
  package.json          # só o build:site + a dep de minify
  .gitignore
  README.md
```

## Fases

### Fase 1 — Landing + páginas legais (o site em si)

Deliverable: `site/` completo, abrível no browser local. Dependências: nenhuma.

- [ ] `index.html` com as secções, por ordem:
  - **Hero**: nome "Vozen Helper", tagline (ex.: "Moderação e comunidade, num só bot —
    para o teu servidor."), 2 CTAs (Adicionar ao servidor · Suporte).
  - **Moderação**: cards — Anti-spam (heat), Anti-raid + join gate, Anti-nuke com
    quarentena, Anti-scam/phishing, AutoMod + filtros, Casos + escalação, Logging total.
  - **Comunidade**: cards — Níveis/XP, Sugestões com votos, Tickets, Giveaways,
    Self-roles, Starboard, `/serverstats`.
  - **Como funciona / setup**: 3 passos (convidar → dar cargo acima dos mods → pronto).
  - **Rodapé**: links Privacidade · Termos · (placeholder) Suporte.
- [ ] `privacy.html`: que dados o bot guarda — **IDs de utilizador/servidor, casos de
      moderação, XP, configs**; **NÃO** guarda conteúdo de mensagens; caminho de eliminação;
      contacto. Honesto e curto. (Adaptar de `Vozen-bot/site/privacy.html`, corrigindo o que
      é do TTS.)
- [ ] `terms.html`: uso aceitável, sem garantias, etc. (adaptar de `Vozen-bot/site/terms.html`).
- [ ] `css/styles.css` com os tokens do UI Blueprint; responsivo; dark.
- [ ] `favicon.svg` (tema do ícone do bot).
- **Done:** `site/index.html` abre no browser sem erros de consola, responsivo em
  mobile e desktop, links legais funcionam, botões apontam para placeholders claros
  (URL de convite OAuth e servidor de suporte — a preencher pelo Diogo).

### Fase 2 — Build (minify) + repositório

Deliverable: repo `vozen-helper-bot` (público) com o site e o build. Dependências: Fase 1.

- [ ] Portar `Vozen-bot/tools/minify-site.mjs` → minifica `site/` para `site-dist/`
      (HTML+CSS+JS; legais ficam legíveis). `package.json` com `"build:site"` e a dep de minify.
- [ ] `.gitignore` (node_modules, site-dist).
- [ ] Criar o repo **público** no GitHub (`gh repo create vozen-helper-bot --public` se o
      `gh` estiver autenticado; senão, mãos do Diogo no site) e `git push`.
- **Done:** `npm run build:site` gera `site-dist/` sem erros; o repo tem o site em `main`.

### Fase 3 — Publicar no GitHub Pages

Deliverable: site online no URL `github.io`. Dependências: Fase 2.

- [ ] Portar `Vozen-bot/.github/workflows/pages.yml` (build:site → publica `site-dist/`;
      `configure-pages enablement: true` liga o Pages sozinho na 1.ª corrida).
- [ ] Push → a Action publica; confirmar o URL `https://<user>.github.io/vozen-helper-bot/`.
- [ ] `site/CNAME` fica vazio/comentado até comprares o domínio.
- **Done:** o URL github.io serve a landing completa; um push a `site/**` redeploya em
  ~1 min (visível na aba Actions).

### Fase 4 (posterior, quando comprares domínio)

- [ ] Colar o domínio no `site/CNAME` + criar os registos DNS (A/CNAME) no registrar.
- [ ] Confirmar HTTPS automático do Pages no domínio próprio.
- **Done:** o domínio serve o site com certificado válido. (Fica para depois — não bloqueia.)

## Riscos

- **Público, não privado**: aceite pelo Diogo; o URL é obscuro mas visível a quem o tiver.
- **Placeholders de links**: o URL de convite (OAuth do bot) e o do servidor de suporte
  têm de ser colados pelo Diogo — o site deixa-os como `#` com nota até serem dados.
- **Páginas legais erradas se copiadas cegamente**: o Helper guarda dados diferentes do
  Vozen (sem voz, sem Premium/Ko-fi). A Fase 1 reescreve-as; não copiar tal e qual.
- **Criar repo/ativar Pages precisa da conta GitHub do Diogo**: por `gh` (se autenticado)
  ou pelos cliques no site — como no deploy do bot.
- **Google Fonts externas**: bloqueiam se o utilizador tiver a rede a filtrar; aceitável
  para uma landing (o Vozen usa-as na mesma). Alternativa (fora de âmbito): auto-hospedar.

## MVP

Fim da Fase 3: landing pública online no `github.io`, com moderação + comunidade
apresentadas, páginas legais e CTAs. O domínio próprio é um extra sem pressa (Fase 4).

**Próxima ação concreta: criar `site/css/styles.css` com os tokens (cores, fontes,
layout) e o `site/index.html` com o hero + as secções Moderação e Comunidade (Fase 1).**
