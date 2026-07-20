# Research: top servidores de Discord — bots, features e o que recriar no Vozen Helper

> Research técnica realizada em 2026-07-13. Base para a próxima vaga de features
> (comunidade/engagement) do Vozen Helper. A implementação será feita com Opus.

## 1. Como estão montados os maiores servidores (2025/2026)

Nota: os servidores gigantes raramente publicam a stack; abaixo separa-se facto confirmado de padrão inferido.

### Por servidor

- **Midjourney (~20M, #1):** o bot central é o próprio produto. Escala por replicação de canais (`newbie-1..10`, `general-#`) com comandos restringidos por canal. Suporte via canal + role de helpers. Moderação embutida no produto.
- **Valorant, LoL, Apex, OSRS, Lost Ark, Battlefield, r/teenagers:** todos usam **Beemo** (anti-raid passivo) — confirmado na página do Beemo. Segurança em camadas (Beemo + Wick + AutoMod nativo + anti-scam). Onboarding nativo do Discord.
- **Fortnite (oficial):** bot custom — renomeia nicknames não-conformes automaticamente, sistema de report por comando. No ecossistema, **Yunite** (verificação Epic, torneios).
- **Genshin/HoYoverse:** self-roles por língua/região, canais de builds/lore/fan-art; stack não divulgada (custom + AutoMod).
- **Minecraft/Roblox/Marvel Rivals:** onboarding curado + eventos diários = retenção ("a batalha é ativação, não aquisição"). Roblox: **Bloxlink/RoVer** (verificação de conta de jogo → roles).
- **Lofi Girl:** comunidade temática com **sistema de níveis/rank** para engagement.
- **OpenAI:** gate por verificação de conta OpenAI (portal próprio) — verificação por conta de produto.
- **Streamers:** integração nativa Twitch/YT (sub sync), Streamcord (live alerts), perks atrás de self-roles.

### Padrões estruturais dos grandes

1. Gate em camadas: verificação nativa → onboarding com perguntas → não-verificados só veem #rules.
2. Replicação de canais + slowmode agressivo nos canais quentes.
3. Categorias: INFO → NEWS → GERAL → LFG → CRIATIVO → SUPORTE (fórum/tickets) → VOICE.
4. Segurança em camadas, cada bot no seu nicho (MEE6 caiu em desuso; Dyno/Carl-bot são os cavalos de trabalho).
5. Bots = enforcement; humanos = julgamento. O diferenciador é a OPERAÇÃO (eventos diários, resposta rápida), não a stack.

### Tabela feature → prevalência

| Feature          | Fornecida por                     | Prevalência nos grandes                    |
| ---------------- | --------------------------------- | ------------------------------------------ |
| Anti-raid        | Beemo, Wick                       | Todos ✅ (Vozen Helper já tem)             |
| Anti-nuke        | Wick                              | Muito comum ✅ (já tem)                    |
| AutoMod conteúdo | Nativo + custom                   | Todos ✅ (já tem)                          |
| Anti-scam        | Fish, custom                      | Todos ✅ (já tem)                          |
| Verificação/gate | Onboarding nativo, portais custom | Todos ✅ (já tem verify-panel + auto-role) |
| Self-roles       | Carl-bot, onboarding nativo       | Todos ⬜ (a fazer)                         |
| Níveis/XP        | MEE6/Carl/custom                  | Comum em comunidades ⬜ (a fazer)          |
| Tickets          | Ticket Tool, ModMail              | Muito comum ⬜ (a fazer)                   |
| Sugestões        | Carl-bot, suggestions.gg          | Comum ⬜ (a fazer)                         |
| Giveaways        | GiveawayBot                       | Comum ⬜ (a fazer)                         |
| Welcome/counter  | Welcome bots, Statbot             | Comum ⬜ (a fazer)                         |
| Starboard        | Carl-bot                          | Nicho/médio ⬜ (opcional)                  |
| Eventos          | Scheduled Events NATIVOS          | Todos → usar nativo, não implementar       |
| Polls            | Polls NATIVOS                     | Usar nativo, não implementar               |

## 2. Especificações das features a recriar (detalhe de implementação)

### Leveling/XP (modelo MEE6 — standard de facto)

- 15–25 XP aleatório por mensagem, cooldown 60s/user (cooldown em Map na memória).
- Fórmula: `5·lvl² + 50·lvl + 100` XP para subir do nível lvl.
- Level roles em marcos (5/10/20/30/50), modo stack ou replace; anúncio em canal dedicado.
- Canais/roles sem XP (blacklist). `/rank` (embed com nível/XP/posição) + `/leaderboard` paginado.
- Pegadinhas: não fazer rank-card em imagem (canvas nativo) — embed dá 90% do valor; verificar hierarquia antes de dar level role. Esforço: MÉDIO.

### Button/Select roles (modelo Carl-bot, modernizado)

- Preferir BOTÕES/select menus a reações (sem partials, sem rate-limit hell): 25 botões máx por mensagem; select com min_values:0 dá "unique+remoção" de graça.
- Modos essenciais: normal (toggle), unique (só 1 do grupo), verify (dá e nunca tira).
- Painéis típicos: pings (@announcements/@events/@giveaways), plataforma, região, cores.
- DB: painel → [{custom_id, role_id, mode}]. Nunca roles com permissões perigosas. Esforço: BAIXO/MÉDIO.

### Tickets (modelo Ticket Tool, versão threads)

- Painel com botão "Abrir ticket" → private thread `ticket-username`, adiciona user + staff role, 1 ticket aberto por user.
- Botões Close (com confirmação) e Claim (marca quem atende).
- Transcript .txt ao fechar → canal de transcripts (paginar messages.fetch a 100).
- custom_ids estáveis (sobreviver a restarts). Esforço: MÉDIO (threads = mais barato que canais).

### Sugestões (melhor rácio valor/custo)

- `/suggest` → embed numerado num canal com botões 👍/👎 (votos na DB, 1/user, pode trocar).
- `/approve|/deny|/consider <id> [razão]` → recolore o embed + razão + DM ao autor (falha silenciosa).
- Esforço: BAIXO.

### Giveaways (modelo GiveawayBot)

- `/gstart <duração> <winners> <prémio>` → embed 🎉 com botão de entrada (entries na DB) e `<t:unix:R>` (countdown sem edits).
- `/gend`, `/greroll`, `/glist`. Requisito de role opcional.
- Persistência: end_timestamp na DB, re-hidratar no boot (usar o scheduler existente). Esforço: BAIXO/MÉDIO.

### Welcome + member counter

- Embed de boas-vindas (avatar, membro nº N) no canal de welcome; suprimir durante raid mode (integrar com o RaidDetector).
- Counter: canal de voz trancado "Membros: N" — rename tem rate limit 2/10min → job com debounce 10 min, só chama API se mudou. Esforço: BAIXO.

### Starboard (opcional)

- ⭐ ≥ threshold → repost em #starboard com jump link + contador. Anti-self-star.
- Mapear original→starboard msg na DB (UNIQUE no original_message_id contra race). Atualizar contador; decidir política quando cai abaixo do threshold. Precisa de partials + intent GuildMessageReactions. Esforço: MÉDIO.

### Utilidades pequenas

- **AFK** (modelo Dyno): `/afk [razão]`; responder a menções; remover na 1ª mensagem (30s tolerância). BAIXO.
- **Lembretes**: `/remind <tempo> <texto>` (usa o scheduler). BAIXO.
- **Tags**: `nome → resposta` com {user}/{server} (NÃO replicar TagScript). BAIXO.
- **Aniversários**: `/birthday set dia mês` (nunca o ano), cron diário, birthday role 24h. BAIXO.
- **Polls**: usar os NATIVOS do Discord (10 respostas, 1h–1sem) — não implementar.
- **Stats**: `/serverstats` básico (joins/leaves/mensagens do logging); dashboard não vale a pena.

### Peça transversal

**Scheduler persistente já existe** (`scheduled_actions` + ExpiryScheduler) — estender com novos tipos (giveaway_end, reminder, birthday_role_remove) em vez de criar outro.

## 3. Proposta de fases de implementação

- **Vaga A (baixo esforço, alto valor):** sugestões · AFK · lembretes · tags · welcome embed + counter · aniversários.
- **Vaga B:** button/select roles (painéis self-role) · giveaways.
- **Vaga C:** leveling/XP com level roles · tickets em threads com transcript.
- **Vaga D (opcional):** starboard · /serverstats · bump reminder.

Tudo single-guild, config-as-code, TDD, comentários PT — as convenções existentes do projeto.

### Fontes principais

beemo.gg · docs.midjourney.com · discord.verify.openai.com · gamediscover.co (who's winning video game discords) · Discord Wiki (Official Fortnite) · yunite.xyz · bloxlink/rover.link · docs.wickbot.com · blog.communityone.io (mod bots 2025, mod team guide 2026) · support.discord.com (Onboarding FAQ, Scheduled Events, Polls FAQ) · streamcord.io · peakbot.pro (anti-raid 2026, MEE6 vs Dyno vs Carl 2026) · github.com/Mee6/Mee6-documentation (levels_xp) · docs.carl.gg + carlbot-docs (reaction roles, starboard) · docs.tickettool.xyz · docs.suggestions.gg · giveawaybot.party + GiveawayBot GitHub · docs.dyno.gg (AFK) · docs.mimu.bot · membercount.net · docs.statbot.net · sesh.fyi
