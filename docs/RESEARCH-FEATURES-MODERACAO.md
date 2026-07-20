# Research: Features de Bots de Moderação Discord (2025/2026)

> Research técnica realizada em 2026-07-13 sobre MEE6, Dyno, Carl-bot, Wick, Bleed, Sapphire, Fire, Zeppelin, Vortex, Beemo e o AutoMod nativo do Discord. Base do planeamento do **Vozen Helper**.

**Legenda:** ✅ = table stakes (praticamente todos os bots têm) · ⭐ = diferenciador (poucos bots têm / feature avançada)

---

## 0. Contexto importante: o AutoMod nativo do Discord

Antes de desenhar um bot, é preciso saber o que o Discord já faz nativamente (para não duplicar, ou para complementar):

- **Keyword filters**: até 6 regras custom por servidor, 1.000 keywords/wildcards por regra (60 chars cada), wildcards com `*` (`scam*` apanha "scammer")
- **Regex**: flavor Rust, até 10 patterns por regra, 260 chars por pattern
- **Presets**: spam content, mention spam (com limite configurável), harmful links, profanity/slurs/sexual content
- **Ações nativas**: block message (a mensagem nunca é publicada — nenhum bot consegue isto, pois bots só apagam _depois_ de publicada), alert para canal de mod-log, timeout do utilizador
- **Pause invites** para gerir surges de joins
- Cobre canais de texto, threads, voice text chat e fóruns

**Implicação de design**: um bot próprio deve _complementar_ o AutoMod nativo (o Sapphire, por exemplo, posiciona-se como "ações adicionais para o AutoMod do Discord"). O bloqueio pré-publicação de keywords deve ser delegado ao AutoMod nativo via API (`AUTO_MODERATION` rules podem ser criadas por bots); o bot trata do que o AutoMod não faz: heurísticas comportamentais, escalação, casos, anti-nuke, etc.

Fontes: [Discord AutoMod FAQ](https://support.discord.com/hc/en-us/articles/4421269296535-AutoMod-FAQ), [Discord blog: anti-spam/raid AutoMod update](https://discord.com/blog/new-anti-spam-raid-automod-safety-update), [Auto Moderation in Discord](https://discord.com/safety/auto-moderation-in-discord)

---

## 1. Anti-Spam

### Features concretas

- ✅ **Flood de mensagens** (X mensagens em Y segundos por utilizador) — todos
- ✅ **Mensagens duplicadas/repetidas** (mesmo conteúdo repetido; Dyno tem filtro "Duplicate Text" dentro da própria mensagem e entre mensagens; Vortex tem anti-duplicate com threshold de delete e threshold de strike separados)
- ✅ **Mass mentions** (X menções por mensagem ou em janela de tempo; Vortex atribui um strike por cada menção acima do máximo)
- ✅ **Emoji spam** (X emojis por mensagem — Dyno, Carl-bot, Zeppelin)
- ✅ **Caps excessivo** (% de maiúsculas, com comprimento mínimo de mensagem — Dyno usa default 70%, Carl-bot exige ≥6 chars)
- ✅ **Spam de links** (rate limit de links, com blacklist/whitelist de domínios)
- ✅ **Invites do Discord** (deteção de `discord.gg/...`, com whitelist de servidores permitidos)
- ✅ **Spam de attachments/imagens** (Dyno: múltiplas imagens de uma vez ou em janela de 10s; sticker spam em janela de 60s)
- ✅ **Newlines/line spam** (X quebras de linha por mensagem — "chat clearing")
- ✅ **Comprimento de mensagem** (character count máximo)
- ⭐ **Zalgo text** (texto corrompido com combining characters — Dyno)
- ⭐ **Spam de threads** (thread_create_spam — Zeppelin)
- ⭐ **Spam multi-canal** (mesma mensagem+link em ≥3 canais diferentes — heurística clássica de deteção de contas comprometidas; usado pelo bot Phishy)
- ⭐ **Honeypot channels** (canal-armadilha invisível para humanos legítimos; quem posta lá é bot/spammer → ban automático — Carl-bot)
- ⭐ **Slowmode automático como resposta a spam** (Carl-bot trata message spam via rate limits tipo slowmode)

### Detalhe de implementação interessante: o Heat System do Wick ⭐

Em vez de regras independentes com thresholds fixos, o Wick usa um **algoritmo de "heat" adaptativo**: cada utilizador acumula pontuação de calor com base em múltiplos fatores combinados — frequência de mensagens, repetição, anúncios, conteúdo NSFW, links maliciosos, emojis, character count, line breaks, padrões de inatividade, menções, attachments e palavras blacklisted. Punição dispara quando o heat total excede o limite. Vantagens: apanha spam "distribuído" (um pouco de cada coisa) e reduz falsos positivos em membros regulares. Inclui **multiplicador de timeout** para reincidentes (a duração aumenta a cada violação, impedindo que raiders "esperem" a punição passar) e **Heat Panic Mode** (se vários utilizadores violarem regras na mesma janela, qualquer "raider" que envie mensagem é imediatamente silenciado).

Fontes: [Wick Docs — Features](https://docs.wickbot.com/intro/features/), [Dyno Automod](https://docs.dyno.gg/en/modules/automod), [Carl-bot automod docs](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/automod.md), [Vortex Auto Moderation wiki](https://github.com/jagrosh/Vortex/wiki/Auto-Moderation)

---

## 2. Filtros de Conteúdo

### Features concretas

- ✅ **Banned words — exact match** (palavra inteira, case-insensitive)
- ✅ **Banned words — wildcard/substring** (Dyno: "hi" banido em wildcard apanha "high"; Carl-bot faz substring matching case-insensitive)
- ✅ **Listas separadas por severidade** (ex.: lista "delete apenas" vs lista "delete + mute" — comum em Dyno/MEE6)
- ⭐ **Regex matching** (Zeppelin `match_regex`; AutoMod nativo suporta Rust regex)
- ⭐ **Normalização anti-evasão**: Zeppelin tem opções `normalize` (remove acentos/lookalikes Unicode), `strip_markdown`, `case_sensitive`, e matching dentro de **embeds** além do texto (`match_embeds`) — crucial porque scammers usam embeds e homóglifos (ex.: "ⓝⓘⓣⓡⓞ", cirílico)
- ✅ **Whitelist de roles e canais** (automod ignora staff e canais específicos — todos os bots)
- ⭐ **Deteção de toxicidade por ML/AI** (bots novos em 2025/26 usam classificadores; o AutoMod nativo tem preset de profanity/slurs mantido pelo Discord — para um bot privado, delegar slurs ao preset nativo e complementar com listas custom é o padrão)
- ⭐ **Filtro de attachments por tipo** (Zeppelin `match_attachment_type`; Carl-bot apaga "unsafe files" — .exe, .bat, .scr)
- ⭐ **Media-only channels** (canal onde só é permitido media, texto é apagado — Carl-bot)
- ⭐ **Filtro NSFW em imagens** (deteção de conteúdo NSFW via image classification — Wick inclui NSFW no heat system; bots de 2026 destacam deteção de scams _dentro de imagens_ porque scammers migraram de texto para screenshots)
- ⭐ **Deteção multilíngue** — nenhum dos bots clássicos faz deteção multilíngue nativa real; a prática é manter listas por língua + normalização Unicode. Diferenciador real para um bot custom com LLM.

### Nota de implementação

O Dyno permite **custom response** por regra (mensagem de aviso customizada quando a regra dispara) e **log channel por regra** — granularidade útil. A tendência 2026 é scams baseados em imagem a contornar filtros de texto ([PeakBot: image-based scams](https://peakbot.pro/blog/how-to-stop-image-scams-discord-2026)).

Fontes: [Dyno Automod](https://docs.dyno.gg/en/modules/automod), [MEE6 Moderator wiki](https://wiki.mee6.xyz/en/plugins/moderator), [Carl-bot automod](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/automod.md), [Zeppelin config examples](https://github.com/shoaibsajid1/Zeppelin)

---

## 3. Anti-Raid / Segurança

### 3a. Anti-raid (join floods)

- ✅ **Deteção de join surge** (X joins em Y segundos → modo raid; Zeppelin `member_join_spam` ex.: 10 joins/18s)
- ✅ **Ações de raid mode**: kick/ban automático dos joiners durante lockdown, aumento do verification level do servidor (Vortex faz ambos), pausar invites (nativo Discord)
- ⭐ **Níveis de antiraid escalonados** (Zeppelin: `set_antiraid_level` como _ação_ de automod — regras diferentes ativam níveis diferentes com respostas progressivamente mais duras)
- ⭐ **Join Raid multi-algoritmo do Wick**: monitoriza padrões de join em janelas customizáveis, avisa roles designados, pune os flagged e entrega **logs com lista de contas ligadas** ao raid
- ✅ **Filtros on-join (Join Gate — Wick)**: conta sem avatar, conta nova (idade mínima customizável), username com invite/anúncio, username matching critérios, bots não autorizados/não verificados — cada filtro com ação independente (timeout/kick/ban)
- ✅ **Verificação/captcha**: Wick oferece CAPTCHA, verificação web, ou verificação instantânea, com modo "só contas suspeitas" vs "todos"
- ✅ **Lockdown**: comando para trancar todos os canais (ou lista configurada) de uma vez; Wick tem **Auto Lockdown** que tranca canais quando não-whitelisted excedem threshold de menções numa janela

#### Detalhe de implementação: como o Beemo deteta userbots ⭐

O Beemo é o standard de anti-raid passivo. Heurística documentada: monitoriza tráfego global de joins; se **~50 contas entram em ~3 segundos**, analisa o lote e compara com padrões de userbots (contas automatizadas que parecem humanas) via **deteção comportamental**; se match, **mass-ban instantâneo**. O algoritmo **não é configurável de propósito** — é dinâmico e adapta-se, e o Beemo beneficia de ver raids em _milhares de servidores_ (a mesma botnet ataca vários servidores; deteção cross-server). Lição para bot privado: um bot single-server não tem este sinal cross-server — deve compensar com join-gate mais agressivo + verificação.
Fontes: [Beemo Docs](https://docs.beemo.gg/), [beemo.gg](https://beemo.gg/), [guia Beemo](https://discord-media.com/en/news/beemo-bot-guide-the-silent-guardian-against-raids.html)

### 3b. Anti-nuke (admins/bots comprometidos) ⭐ — o grande diferenciador do Wick e do Bleed

- ⭐ **Rate limits em ações administrativas**: monitorizar audit log e detetar mass-actions — mass channel create/delete, mass role create/delete, mass kick/ban, mass webhook create/delete, mass emoji delete (Wick monitoriza todas; webhooks/emojis são premium)
- ⭐ **Quarentena**: o autor da ação anómala é imediatamente despojado de roles/permissões ("zero power") em vez de banido — reversível se for falso positivo (Wick); o Bleed "trava as ações e faz strip das permissões" quando deteta ex.: 10 bans ou 5 channel deletes em segundos, com **thresholds customizáveis**
- ⭐ **Whitelist de admins confiáveis** (ações do owner/whitelisted não disparam)
- ⭐ **Backups + restore automático**: o Wick usa o backup mais recente para **reverter automaticamente** o que o nuke apagou (canais, roles); sem imaging, tenta reconstruir de memória/dados do Discord
- ⭐ **Panic Mode** (Wick): lockdown automático do servidor durante nuke rápido, com um "miniWick" separado a analisar o ataque
- ⭐ **Fake Permissions (Bleed)**: conceito interessante — moderadores recebem permissões _apenas dentro do bot_ (podem usar `/ban` do bot) mas **não têm a permissão nativa do Discord**, logo um script/token comprometido não consegue mass-ban via API. O bot torna-se o único caminho para ações destrutivas, com os seus rate limits
- ⭐ **Proteção contra webhooks maliciosos** (deteção de criação de webhooks + spam via webhook; deleção automática)
- ⭐ **Anti bot-add** (bot adicionado sem autorização → kick/ban imediato do bot e quarentena de quem o adicionou)
- ⭐ **Deteção de bypass** (alertar o owner quando alguém tenta contornar o anti-nuke — Wick)

Fontes: [Wick Docs](https://docs.wickbot.com/intro/features/), [wickbot.com](https://wickbot.com/), [Bleed Fake Permissions](https://docs.bleed.bot/security/fake-permissions), [bleed.bot](https://bleed.bot/)

---

## 4. Anti-Scam / Phishing

- ✅ **Deteção de links maliciosos por blacklist de domínios** (bases de dados de domínios de phishing conhecidos; Phishy valida via VirusTotal antes de adicionar à blacklist permanente)
- ✅ **Deteção de fake Nitro** (embeds que imitam o gift real; heurística: URL que não é `discord.gift`/`discord.com` mas parece — lookalike domains tipo `dlscord.gift`, `discorcl.com`)
- ⭐ **Link resolving / anti-redirect (Vortex)**: segue redirects (HTTP header, JavaScript e cadeias de redirects) até ao destino final antes de avaliar — derrota encurtadores e redirect chains usados para esconder phishing. Também **anti-referral links**
- ⭐ **Heurística multi-canal**: mesmo link postado em ≥3 canais em sucessão rápida = conta comprometida → mute imediato (Phishy)
- ⭐ **Impersonation de staff**: deteção de username/avatar iguais aos de staff (comparação de nome normalizado + avatar hash contra a lista de mods) → rename/kick automático; scams típicos: fake "Discord Staff", HypeSquad, "system support bots"
- ⭐ **Deteção de scam em imagens** (screenshots de fake giveaways — requer OCR/image classification; tendência 2026)
- ⭐ **Escalação por reincidência**: delete + warn ao 1.º link, kick/ban ao 3.º (Detective Discord)
- ✅ **Preset "harmful links" do AutoMod nativo** (delegável)

Fontes: [Comparitech Discord scams](https://www.comparitech.com/blog/information-security/discord-scams/), [Aura: 11 Discord scams](https://www.aura.com/learn/discord-scams), [Phishy bot](https://top.gg/bot/905051022848819231), [Detective-Discord](https://github.com/kanakmi/Detective-Discord), [NordVPN Discord scams](https://nordvpn.com/blog/discord-scams/)

---

## 5. Ações de Moderação

- ✅ **Warn** (com razão, DM opcional ao utilizador)
- ✅ **Mute/Timeout** (timeout nativo do Discord até 28 dias; mute por role para durações maiores — Zeppelin alterna automaticamente entre slowmode nativo ≤6h e enforced pelo bot para durações maiores; o mesmo padrão aplica-se a mutes)
- ✅ **Kick**
- ✅ **Ban / Tempban / Unban** (com auto-unban no fim do tempo — Vortex)
- ✅ **Softban** (ban+unban imediato para apagar mensagens sem expulsar permanentemente)
- ✅ **Purge de mensagens do infrator** ao banir (delete message days)
- ✅ **Histórico de casos** (case system: cada ação = caso numerado com autor, alvo, razão, timestamp; Zeppelin destaca-se com "detailed moderator action tracking and **notes**"; Sapphire tem gestão de casos + **reason aliases** + DM notifications customizáveis)
- ✅ **Modlogs por utilizador** (`/modlogs @user` mostra o cadastro completo — Fire, Dyno, Carl-bot)
- ✅ **Escalação automática por warns** (MEE6: X warns numa janela de tempo → mute temporário / ban temporário / ban permanente; Carl-bot: warn thresholds com punições custom)
- ⭐ **Sistema de strikes granular (Vortex)**: cada regra de automod atribui N strikes configuráveis; punições mapeadas a contagens de strikes (ex.: 3 strikes=mute 1h, 5=kick, 7=ban) com ações None/Mute/Kick/Softban/Ban e durações — mais flexível que warns simples
- ✅ **Razões obrigatórias/anexáveis + edição de razão de caso**
- ✅ **Protected roles / hierarquia** (mods não podem punir roles iguais/superiores — Dyno)
- ✅ **Moderator roles configuráveis** (quem pode usar comandos, independente de permissões nativas — Fire `/moderators add`)
- ⭐ **Ban appeals integrados** (Fire: banido recebe DM com instruções de appeal; mods aprovam/negam dentro do Discord)
- ⭐ **Drama channel (Carl-bot premium)**: em vez de punir automaticamente, a infração é enviada para um canal onde os mods **votam por reações** na punição
- ⭐ **Punições combináveis** (Carl-bot: "delete, warn, tempmute 1h" numa só regra)
- ✅ **DM ao punido** (com razão e duração; configurável)

Fontes: [Vortex Strikes wiki](https://github.com/jagrosh/Vortex/wiki/Strikes), [Dyno Moderation](https://docs.dyno.gg/en/modules/moderation), [MEE6 Moderator](https://wiki.mee6.xyz/en/plugins/moderator), [Fire moderation features](https://mintlify.wiki/FireDiscordBot/bot/features/moderation), [Carl-bot docs](https://docs.carl.gg/)

---

## 6. Auto-Roles

- ✅ **Role on join** (atribuição automática ao entrar; com delay opcional — anti-raid: só dar role após X minutos)
- ✅ **Reaction roles** (Carl-bot é a referência: até 250 roles/mensagem, modos **unique** (só uma), **verify**, **reversed**, **temporary**, **binding** (não removível))
- ✅ **Button/select-menu roles** (o padrão moderno pós-2022, substitui reactions)
- ✅ **Verification gate** (role de "verificado" só após captcha/clique/regras aceites; membros não verificados não veem canais; integra com o Membership Screening nativo)
- ⭐ **Roles por nível/atividade** (MEE6: level rewards — é XP/leveling, adjacente a moderação)
- ⭐ **Roles por tempo de permanência** (auto-role após X dias no servidor)
- ✅ **Sticky roles** (re-aplicar roles quando o membro sai e volta — evita evasão de mute por rejoin; **crítico para moderação**: mute persistence é table stakes em Dyno/Zeppelin)
- ⭐ **Timed/temporary roles** (role que expira)

Fontes: [Carl-bot docs](https://docs.carl.gg/), [carl.gg/about](https://carl.gg/about), [MEE6](https://mee6.xyz/plugins/management)

---

## 7. Logging / Auditoria

- ✅ **Mensagens apagadas** (com conteúdo original, autor, canal; incluindo bulk deletes com ficheiro/paste do conteúdo)
- ✅ **Mensagens editadas** (antes/depois)
- ✅ **Joins/leaves** (com idade da conta no join — sinal anti-raid; e roles que o membro tinha ao sair)
- ✅ **Voice events** (join/leave/move/mute de canais de voz)
- ✅ **Mudanças de membro** (nickname, roles adicionados/removidos, avatar)
- ✅ **Mudanças de servidor** (canais criados/apagados/editados, roles criados/apagados/editados, emojis, webhooks, invites criados)
- ✅ **Bans/unbans/kicks/timeouts** (do audit log, com quem executou)
- ✅ **Mod actions log separado** (canal de casos distinto do log de mensagens)
- ✅ **Canais de log separados por categoria de evento** (Carl-bot/Dyno: escolher destino por tipo de evento; Zeppelin: logs totalmente customizáveis com formato por evento)
- ⭐ **Ignore lists** (canais/utilizadores excluídos do logging — ex.: canais de staff)
- ⭐ **Log de automod dedicado** (o que o automod apanhou e porquê, com a mensagem original)
- ⭐ **Audit trail de atividade** (Sapphire: tracking de mensagens/edits/deletes/joins como trilha de auditoria)
- ⭐ **Member search granular** (Zeppelin: procurar membros por padrões de nome, data de join, roles — útil para limpar raids retroativamente)

Fontes: [Zeppelin GitHub](https://github.com/ZeppelinBot/Zeppelin), [Carl-bot](https://carl.gg/about), [Sapphire](https://sapph.xyz/)

---

## 8. Utilitários de Mod

- ✅ **Purge/clear** com filtros (Fire: por utilizador, conteúdo de texto, mensagens de bots, attachments; outros acrescentam: links, embeds, match regex, até mensagem X, entre mensagens)
- ✅ **Slowmode** (set/remove; Zeppelin: slowmodes geridos pelo bot para durações >6h que o Discord nativo não suporta)
- ✅ **Lock/unlock canal** (+ lockdown de servidor inteiro com lista de canais pré-configurada)
- ✅ **Nickname moderation**:
  - ⭐ **Auto-dehoist** (renomear quem usa `!`/`﹗`/chars especiais para aparecer no topo da lista de membros — Fire, Dyno)
  - ⭐ **Auto-decancer** (renomear nomes com Unicode não-legível/zalgo para um nome default — Fire)
  - ✅ Forcenick/setnick por comando
- ✅ **Userinfo/whois** (idade da conta, data de join, roles, casos anteriores — triagem de contas suspeitas)
- ✅ **Server info / role info / avatar grab**
- ⭐ **Slowmode por utilizador** (rate limit individual)
- ⭐ **Voice-mod** (mass move/disconnect de voz)
- ⭐ **Notas de staff sobre membros** (notes invisíveis ao utilizador — Zeppelin)
- ⭐ **Tags/custom commands** (respostas prontas para regras/FAQ — Zeppelin, Carl-bot)

Fontes: [Fire](https://getfire.bot/), [Alternative.me Fire commands](https://alternative.me/discord/bots/fire/commands), [Zeppelin](https://github.com/ZeppelinBot/Zeppelin)

---

## 9. Outras categorias descobertas (foco em moderação)

### 9a. Arquitetura de configuração (meta-feature, mas decisiva)

- ⭐ **Overrides granulares** (Zeppelin: qualquer config sobreponível por utilizador, canal, categoria, nível de permissão — ex.: automod mais brando em #off-topic)
- ✅ **Permission levels** (níveis 0-100 em vez de roles hardcoded — Zeppelin)
- ✅ **Dashboard web vs config-as-code** (Zeppelin usa YAML — para um bot privado, config em ficheiro versionado é mais simples que dashboard)
- ⭐ **Import de listas de outros bots** (Sapphire importa banned words de outros bots)

### 9b. Verificação de membros (gate de entrada)

- ✅ Captcha/DM verification, botão "verificar", idade mínima de conta
- ⭐ Verificação escalonada por risco (Wick: verificar só contas suspeitas)
- ⭐ Modo "verification raid": durante raid, subir automaticamente o nível de verificação exigido

### 9c. Deteção de contas suspeitas (contínua, não só no join)

- ⭐ Conta nova + primeiro post contém link = alto risco (heurística comum)
- ⭐ Padrões de lurk-then-spam (conta entra, fica inativa, depois spamma — Wick inclui "inactivity patterns" no heat)
- ⭐ Alt-account detection (correlação de padrões; limitada sem dados cross-server)

### 9d. Resiliência do próprio bot

- ⭐ Proteção do próprio bot contra remoção de permissões; alertas quando o bot perde permissões necessárias
- ⭐ Backups periódicos automáticos do servidor (roles, canais, permissões) independentes do anti-nuke — Wick

**Excluídos deliberadamente** (fora de moderação): starboard, tickets, leveling/XP, música, welcome messages decorativas, giveaways — presentes em Carl-bot/MEE6/Zeppelin mas não são moderação.

---

## Síntese: o núcleo mínimo vs diferenciadores

**Table stakes (qualquer bot de moderação em 2025/26 tem de ter):** warn/timeout/kick/ban/softban/tempban com casos numerados e histórico por utilizador; escalação automática warn→mute→ban; anti-spam de flood/duplicados/menções/caps/emoji/invites/links; banned words exact+wildcard com whitelist de roles/canais; logging completo (delete/edit/join/leave/voice/roles/mod-actions) com canais separados; purge com filtros; slowmode/lock; role-on-join + verification gate + sticky roles (mute persistence); DM ao punido.

**Diferenciadores de topo (por ordem de valor para um servidor privado):**

1. **Anti-nuke com quarentena + whitelist + restore** (Wick/Bleed) — raro e de alto valor
2. **Heat system** em vez de regras isoladas (Wick) — menos falsos positivos
3. **Strikes configuráveis por regra** (Vortex) — escalação fina
4. **Link resolving anti-redirect** (Vortex) — derrota phishing encurtado
5. **Fake permissions** (Bleed) — mods operam só via bot
6. **Overrides por canal/utilizador/nível** (Zeppelin) — config expressiva
7. **Honeypot channel** (Carl-bot) — barato de implementar, muito eficaz
8. **Auto-dehoist/decancer** (Fire/Dyno)
9. **Join gate multi-filtro com ação por filtro** (Wick)
10. **Integração com AutoMod nativo** para bloqueio pré-publicação (Sapphire)

### Fontes principais

- Wick: https://docs.wickbot.com/intro/features/ · https://wickbot.com/
- Beemo: https://docs.beemo.gg/ · https://beemo.gg/
- Zeppelin: https://github.com/ZeppelinBot/Zeppelin · https://zeppelin.gg/
- Dyno: https://docs.dyno.gg/en/modules/automod · https://docs.dyno.gg/en/modules/moderation
- Carl-bot: https://docs.carl.gg/ · https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/automod.md
- Vortex: https://github.com/jagrosh/Vortex/wiki/Auto-Moderation · https://github.com/jagrosh/Vortex/wiki/Strikes
- MEE6: https://wiki.mee6.xyz/en/plugins/moderator
- Bleed: https://docs.bleed.bot/security/fake-permissions · https://bleed.bot/
- Sapphire: https://sapph.xyz/
- Fire: https://getfire.bot/ · https://mintlify.wiki/FireDiscordBot/bot/features/moderation
- Discord AutoMod: https://support.discord.com/hc/en-us/articles/4421269296535-AutoMod-FAQ · https://discord.com/blog/new-anti-spam-raid-automod-safety-update
- Anti-phishing: https://www.comparitech.com/blog/information-security/discord-scams/ · https://www.aura.com/learn/discord-scams · https://github.com/kanakmi/Detective-Discord
