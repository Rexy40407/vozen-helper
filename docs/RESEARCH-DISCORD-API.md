# Research técnica: capacidades da plataforma Discord para um bot de moderação privado

> Research feita por agente Fable 5 (2026-07-13), focada em TypeScript + discord.js v14. Base do planeamento do **Vozen Helper**.

**Legenda:**
- **[NATIVO]** — o Discord já faz; o bot no máximo configura via API.
- **[BOT]** — a plataforma não cobre; o bot tem de implementar a lógica.
- **[HÍBRIDO]** — a plataforma dá a primitiva, o bot orquestra.

Nota geral: a documentação oficial migrou de `discord.com/developers/docs/...` para `https://docs.discord.com/developers/...` (redirect 301). Os links abaixo usam o domínio novo.

---

## 1. AutoMod nativo do Discord [NATIVO, configurável via API]

Fonte: https://docs.discord.com/developers/resources/auto-moderation

O AutoMod corre **server-side, antes de a mensagem ser publicada** — nenhum bot consegue replicar isto (um bot só age *depois* de a mensagem existir). É a primeira linha de defesa; o bot deve gerir as regras via API em vez de reimplementá-las.

### 1.1 Trigger types e limites por guild

| Trigger | Valor | O que faz | Máx. regras/guild |
|---|---|---|---|
| `KEYWORD` | 1 | Conteúdo contém palavras de lista definida (com wildcards e regex) | **6** |
| `SPAM` | 3 | Deteção genérica de spam do Discord (modelo interno) | 1 |
| `KEYWORD_PRESET` | 4 | Wordsets internos do Discord (profanity, sexual content, slurs) | 1 |
| `MENTION_SPAM` | 5 | Mais menções únicas do que o permitido | 1 |
| `MEMBER_PROFILE` | 6 | Filtra nome/perfil do membro contra keyword list | 1 |

### 1.2 Limites de trigger metadata

- `keyword_filter`: máx. **1.000 keywords**, 60 chars cada (KEYWORD, MEMBER_PROFILE). Suporta wildcards (`*` prefixo/sufixo).
- `regex_patterns`: máx. **10 padrões**, **260 chars cada**, **regex Rust-flavored apenas** (não é PCRE — sem lookbehind, por exemplo).
- `allow_list`: máx. 100 entradas (60 chars) em KEYWORD/MEMBER_PROFILE; máx. 1.000 em KEYWORD_PRESET.
- `mention_total_limit`: máx. **50** menções únicas.
- `mention_raid_protection_enabled`: booleano — proteção automática contra raids de menções.

### 1.3 Event types e action types

Event types: `MESSAGE_SEND` (1) e `MEMBER_UPDATE` (2, para MEMBER_PROFILE).

| Ação | Valor | Metadata | Notas |
|---|---|---|---|
| `BLOCK_MESSAGE` | 1 | `custom_message` (máx. 150 chars) | Bloqueia **antes** de publicar |
| `SEND_ALERT_MESSAGE` | 2 | `channel_id` | Log para canal de alertas |
| `TIMEOUT` | 3 | `duration_seconds` (máx. **2.419.200 s = 4 semanas**) | Só em KEYWORD e MENTION_SPAM; quem cria a regra precisa de `MODERATE_MEMBERS` |
| `BLOCK_MEMBER_INTERACTION` | 4 | — | "Quarentena": impede texto, voz e interações (corresponde ao audit log `AUTO_MODERATION_QUARANTINE_USER`) |

### 1.4 Isenções (importante para o desenho do bot)

- `exempt_roles`: máx. **20 por regra**; `exempt_channels`: máx. **50 por regra**.
- **Sempre isentos, sem opção de mudar**: utilizadores com `ADMINISTRATOR` ou `MANAGE_GUILD`, **bots** e **webhooks**. O AutoMod nunca analisa mensagens de bots/webhooks. Fontes: https://support.discord.com/hc/en-us/articles/4421269296535-AutoMod-FAQ , https://discord.com/safety/auto-moderation-in-discord

### 1.5 Gestão via API (o que o bot pode fazer)

Endpoints (permissão `MANAGE_GUILD` para todos):
- `GET /guilds/{guild.id}/auto-moderation/rules` — listar
- `GET .../rules/{rule.id}` — obter
- `POST .../rules` — criar (aceita header `X-Audit-Log-Reason`)
- `PATCH .../rules/{rule.id}` — modificar
- `DELETE .../rules/{rule.id}` — apagar (204)

Gateway events: `AUTO_MODERATION_RULE_CREATE/UPDATE/DELETE` (intent `AUTO_MODERATION_CONFIGURATION`, 1<<20) e `AUTO_MODERATION_ACTION_EXECUTION` (intent `AUTO_MODERATION_EXECUTION`, 1<<21) — este último dá `rule_id`, `user_id`, `channel_id`, `matched_keyword`, `matched_content`; os campos `content`/`matched_content` **exigem também o intent `MESSAGE_CONTENT`**. Nenhum destes dois intents AutoMod é privileged.

Padrão recomendado: o bot cria/gere as regras AutoMod e escuta `AUTO_MODERATION_ACTION_EXECUTION` para alimentar o seu sistema de infrações/escalada (ex.: 3 blocks → timeout maior).

### 1.6 O que o AutoMod NÃO cobre → o BOT tem de implementar

- **Rate/flood spam configurável** (X mensagens em Y segundos, mensagens duplicadas cross-channel com parâmetros próprios) — o trigger SPAM é uma caixa preta não configurável. **[BOT]**
- **Deteção de raid por taxa de joins** (N joins em M segundos). O Discord tem "Raid Protection"/Activity Alerts nativos na UI de Safety Setup, mas **não são configuráveis nem observáveis via API de bot**. **[BOT]** (via `GUILD_MEMBER_ADD` + janela deslizante)
- **Anti-nuke** (mass channel/role delete, mass ban por moderador comprometido). **[BOT]** (via `GUILD_AUDIT_LOG_ENTRY_CREATE`)
- **Filtragem de anexos/imagens por conteúdo** (NSFW scanning de imagens é feito nativamente pelo filtro de media explícita do Discord, mas não é gerível por bots). **[BOT]** se quiser lógica própria
- **Escalada de sanções / histórico de infrações / warns** — o Discord não tem conceito de "warn". **[BOT]** (base de dados própria)
- **Filtro de convites/links com lógica contextual** (whitelist por canal, resolução de redirects) — parcialmente cobrível com keyword rules (`discord.gg/*`), mas sem contexto. **[HÍBRIDO]**
- **Mensagens de bots e webhooks** — invisíveis ao AutoMod; um anti-nuke de webhooks maliciosos é 100% **[BOT]**.
- **Moderação retroativa** (apagar mensagens já publicadas que violem regra nova). **[BOT]**

---

## 2. Primitivas de moderação da API

Fontes: https://docs.discord.com/developers/resources/guild , https://docs.discord.com/developers/resources/message

### 2.1 Timeout [NATIVO como mecanismo de enforcement]

- Campo `communication_disabled_until` (ISO8601) no `PATCH /guilds/{id}/members/{user.id}`; `null` remove.
- Máximo **28 dias** no futuro. Permissão: `MODERATE_MEMBERS`.
- **Não funciona** em quem tem `ADMINISTRATOR` nem no dono do servidor (403).
- Enquanto em timeout o membro não envia mensagens, não fala em voz, não reage, não usa interações — enforcement é da plataforma.
- Deteção de timeout aplicado/removido por terceiros: `GUILD_MEMBER_UPDATE` (o campo vem no payload) ou audit log `MEMBER_UPDATE` (24). Nota conhecida: o campo pode manter uma data já expirada em vez de voltar a `null` — comparar com `Date.now()`, não com null (https://github.com/discord/discord-api-docs/issues/6434).

### 2.2 Ban / kick / bulk ban

- **Ban**: `PUT /guilds/{id}/bans/{user.id}` — permissão `BAN_MEMBERS`. Parâmetro `delete_message_seconds`: **0 a 604.800 (7 dias)** de mensagens apagadas. `delete_message_days` está **deprecated**.
- **Unban**: `DELETE /guilds/{id}/bans/{user.id}` — `BAN_MEMBERS`.
- **Kick**: `DELETE /guilds/{id}/members/{user.id}` — permissão `KICK_MEMBERS` (204; dispara `GUILD_MEMBER_REMOVE`).
- **Bulk Guild Ban**: `POST /guilds/{id}/bulk-ban` — até **200 user IDs por request**, mesmo `delete_message_seconds` (0–604800). Requer `BAN_MEMBERS` **e** `MANAGE_GUILD`. Resposta: `banned_users` + `failed_users`; se **nenhum** for banido, erro **500000 BULK_BAN_FAILED**. Falham: já banidos, role superior à do bot, dono, o próprio bot. Fontes: docs guild + https://discord-api-types.dev/api/discord-api-types-v10/interface/RESTPostAPIGuildBulkBanResult
- **Get Guild Bans**: paginado (`limit` até 1000, `before`/`after`).
- **Prune**: `GET/POST /guilds/{id}/prune` para remoção de inativos (`KICK_MEMBERS`).

### 2.3 Audit logs

Fonte: https://docs.discord.com/developers/resources/audit-log

- `GET /guilds/{id}/audit-logs` — permissão `VIEW_AUDIT_LOG`. Filtros: `user_id`, `action_type`, `before`/`after`, `limit` 1–100 (default 50). Retenção: **45 dias**.
- Entrada: `user_id` (executor), `target_id`, `action_type`, `changes[]` (old/new), `options`, `reason` (1–512 chars).
- **`X-Audit-Log-Reason`** (1–512 chars, URL-encoded UTF-8): o bot deve enviá-lo em TODAS as ações de moderação — é assim que o "motivo" aparece no audit log nativo. discord.js faz isto automaticamente com a opção `reason`.
- Códigos relevantes para moderação/anti-nuke: `CHANNEL_CREATE` 10, `CHANNEL_DELETE` 12, `MEMBER_KICK` 20, `MEMBER_PRUNE` 21, `MEMBER_BAN_ADD` 22, `MEMBER_BAN_REMOVE` 23, `MEMBER_UPDATE` 24 (inclui timeouts), `ROLE_CREATE` 30, `ROLE_DELETE` 32, `WEBHOOK_CREATE` 50, `WEBHOOK_DELETE` 52, `MESSAGE_DELETE` 72, `MESSAGE_BULK_DELETE` 73, `AUTO_MODERATION_BLOCK_MESSAGE` 143, `AUTO_MODERATION_FLAG_TO_CHANNEL` 144, `AUTO_MODERATION_USER_COMMUNICATION_DISABLED` 145, `AUTO_MODERATION_QUARANTINE_USER` 146.
- **Consumo em tempo real**: gateway event `GUILD_AUDIT_LOG_ENTRY_CREATE` — requer intent `GUILD_MODERATION` (1<<2, não privileged) **e** que o bot tenha `VIEW_AUDIT_LOG`. É a espinha dorsal de qualquer anti-nuke: dá executor + ação em tempo quase real, sem polling.
- Limitação: apagar a **própria** mensagem não gera entrada de audit log — impossível saber "quem apagou" nesses casos.

### 2.4 Incident Actions [NATIVO, pouco conhecido, ótimo para anti-raid]

- `PUT /guilds/{id}/incident-actions` — permissão `MANAGE_GUILD`. Campos: `invites_disabled_until` e `dms_disabled_until` (pausa DMs entre membros não-amigos), **máx. 24 h no futuro** cada. É o "Pause Invites / Pause DMs" da UI de segurança, acionável pelo bot como resposta automática a raid. Fonte: docs guild + https://github.com/discord/discord-api-docs/issues/6797

### 2.5 Permissões necessárias — resumo por ação

| Ação | Permissão do bot |
|---|---|
| Timeout | `MODERATE_MEMBERS` |
| Kick | `KICK_MEMBERS` |
| Ban/unban/lista de bans | `BAN_MEMBERS` |
| Bulk ban | `BAN_MEMBERS` + `MANAGE_GUILD` |
| Apagar mensagens / bulk delete | `MANAGE_MESSAGES` |
| Gerir regras AutoMod | `MANAGE_GUILD` (+ `MODERATE_MEMBERS` p/ ação TIMEOUT) |
| Ler audit logs / evento gateway | `VIEW_AUDIT_LOG` |
| Incident actions, onboarding | `MANAGE_GUILD` (onboarding também `MANAGE_ROLES`) |
| Gerir roles (mute-role legacy, quarentena) | `MANAGE_ROLES` + hierarquia |

Regra transversal: **hierarquia de roles** — o bot só age sobre membros cujo role mais alto seja inferior ao role mais alto do bot; nunca sobre o dono.

---

## 3. Intents e privileged intents

Fonte: https://docs.discord.com/developers/events/gateway

### 3.1 Necessários para um bot de moderação

| Intent | Bit | Privileged? | Para quê |
|---|---|---|---|
| `GUILDS` | 1<<0 | Não | Canais/roles/threads (anti-nuke), cache base — praticamente obrigatório |
| `GUILD_MEMBERS` | 1<<1 | **Sim** | `GUILD_MEMBER_ADD/UPDATE/REMOVE` — deteção de raid, screening, perfil |
| `GUILD_MODERATION` | 1<<2 | Não | `GUILD_AUDIT_LOG_ENTRY_CREATE`, `GUILD_BAN_ADD/REMOVE` |
| `GUILD_MESSAGES` | 1<<9 | Não | `MESSAGE_CREATE/UPDATE/DELETE/DELETE_BULK` (sem conteúdo) |
| `MESSAGE_CONTENT` | 1<<15 | **Sim** | Popular `content`, `embeds`, `attachments`, `components` |
| `AUTO_MODERATION_CONFIGURATION` | 1<<20 | Não | Rule create/update/delete |
| `AUTO_MODERATION_EXECUTION` | 1<<21 | Não | `AUTO_MODERATION_ACTION_EXECUTION` |
| `GUILD_WEBHOOKS` | 1<<5 | Não | `WEBHOOKS_UPDATE` (anti-nuke) |
| `GUILD_INVITES` | 1<<6 | Não | Tracking de invites (atribuir joins a convites em raids) |
| `GUILD_PRESENCES` | 1<<8 | **Sim** | Geralmente desnecessário para moderação — evitar |

### 3.2 Bot não-verificado num só servidor — resposta direta

**Sim.** Um bot em **menos de 100 servidores** (não-verificado) pode ativar `GUILD_MEMBERS`, `MESSAGE_CONTENT` e `GUILD_PRESENCES` **livremente, sem review**: basta ligar os toggles em Developer Portal → Bot → "Privileged Gateway Intents". A aprovação da Discord só é exigida na verificação (≥100 servidores / a partir de ~75 recebem aviso). Para um bot privado self-hosted num único servidor não há nenhum obstáculo. Se o intent estiver no código mas desligado no portal, o gateway fecha com **erro 4014 (Disallowed intents)**.

### 3.3 Restrições do MESSAGE_CONTENT

Sem o intent, `content`/`embeds`/`attachments`/`components`/`poll` chegam **vazios**, exceto: mensagens do próprio bot, DMs ao bot, mensagens que mencionam o bot, e mensagens alvo de context menu commands. Para anti-spam/filtros próprios é indispensável.

### 3.4 Identify limits

1.000 `IDENTIFY`/24 h (irrelevante para 1 servidor; atenção apenas a crash-loops que re-identificam — exceder força reset do token).

---

## 4. Eventos relevantes (mapa para o bot)

Fonte: https://docs.discord.com/developers/events/gateway-events

| Evento | Intent | Uso no bot |
|---|---|---|
| `GUILD_MEMBER_ADD` | GUILD_MEMBERS (priv.) | **Anti-raid**: janela deslizante de joins; heurísticas de conta (idade da conta via snowflake, avatar default, padrões de username); campo `pending` para screening |
| `GUILD_MEMBER_UPDATE` | GUILD_MEMBERS | Deteção de `pending` false→ (fim do screening), timeouts (`communication_disabled_until`), mudanças de roles |
| `GUILD_MEMBER_REMOVE` | GUILD_MEMBERS | Correlacionar com audit log para distinguir leave/kick/ban |
| `MESSAGE_CREATE/UPDATE` | GUILD_MESSAGES + MESSAGE_CONTENT | Anti-spam próprio, filtros custom; UPDATE apanha edits para conteúdo proibido pós-facto (o AutoMod também verifica edits nativamente) |
| `MESSAGE_DELETE / DELETE_BULK` | GUILD_MESSAGES | Logging (conteúdo antigo só se estava em cache) |
| `GUILD_AUDIT_LOG_ENTRY_CREATE` | GUILD_MODERATION + perm. VIEW_AUDIT_LOG | **Anti-nuke**: contar `CHANNEL_DELETE`/`ROLE_DELETE`/`MEMBER_BAN_ADD`/`MEMBER_KICK`/`WEBHOOK_CREATE` por executor numa janela; atribuição de "quem fez o quê" |
| `GUILD_BAN_ADD/REMOVE` | GUILD_MODERATION | Log de bans; só enviado a bots com `BAN_MEMBERS` ou `VIEW_AUDIT_LOG` |
| `CHANNEL_CREATE/UPDATE/DELETE`, `GUILD_ROLE_CREATE/UPDATE/DELETE` | GUILDS | Anti-nuke (o payload não diz quem — cruzar com audit log) |
| `WEBHOOKS_UPDATE` | GUILD_WEBHOOKS | Só dá `guild_id`+`channel_id`; o executor vem do audit log `WEBHOOK_CREATE` (50) |
| `AUTO_MODERATION_ACTION_EXECUTION` | AUTO_MODERATION_EXECUTION | Alimentar sistema de infrações a partir dos triggers nativos |

Pegadinha estrutural: os eventos de entidade (channel/role delete, message delete) **nunca incluem o executor** — a atribuição vem sempre do audit log (evento ou fetch).

---

## 5. Limitações e rate limits

Fontes: https://docs.discord.com/developers/topics/rate-limits , https://github.com/discord/discord-api-docs/issues/208

- **Bulk delete** (`POST /channels/{id}/messages/bulk-delete`, `MANAGE_MESSAGES`): **2–100 mensagens por chamada**; qualquer ID com **mais de 14 dias** → erro **50034** ("You can only bulk delete messages that are under 14 days old"); IDs duplicados contam contra o total. Mensagens mais antigas só uma a uma (lento: rate limit por rota).
- **Global**: 50 requests/s por bot. **Per-route buckets** com headers `X-RateLimit-Limit/Remaining/Reset-After/Bucket/Scope`; sub-limites de rotas de moderação (bans, deletes) não são documentados — respeitar sempre os headers/`retry_after` (discord.js já enfileira automaticamente).
- **Invalid request limit**: 10.000 respostas 401/403/429 em 10 min → **ban temporário do IP pela Cloudflare**. Num anti-nuke agressivo, verificar permissões/hierarquia ANTES de disparar chamadas em massa que dariam 403.
- **Mass ban**: não há limite documentado específico; usar **Bulk Guild Ban (200/chamada)** em vez de loops de bans individuais. Relatos da comunidade referem throttling adicional após milhares de bans seguidos (não oficial): https://github.com/discord/discord-api-docs/issues/5002
- **Timeout**: máx. 28 dias; não renovável para além disso sem re-aplicar. Não funciona em admins.
- **delete_message_seconds** no ban: máx. 7 dias de histórico.
- **Audit log**: retenção 45 dias; `reason` máx. 512 chars.
- **AutoMod**: 6 regras KEYWORD (agregar keywords por regra, não uma regra por palavra), 10 regex de 260 chars, timeout de AutoMod máx. 4 semanas.

---

## 6. Verificação de membros / gating

Fontes: https://docs.discord.com/developers/resources/guild (Membership Screening, Guild Onboarding), https://github.com/discord/discord-api-docs/discussions/8016

### 6.1 Membership Screening (Rules Screening) [NATIVO]

- Membro entra com `pending: true` e fica bloqueado de interagir até aceitar as regras; ao aceitar, `GUILD_MEMBER_UPDATE` com `pending: false`.
- **A API de leitura/edição do objeto de screening foi removida dos docs** ("significant changes... endpoints have been removed") — o bot **não configura** o screening, só observa o `pending`.
- Bots **não podem aprovar** candidaturas "Apply to Join" (o endpoint responde "Bots cannot use this endpoint").
- Pegadinha histórica: atribuir um role automaticamente no join pode impedir o ecrã de screening de aparecer; o padrão correto é atribuir roles **quando `pending` passa a false**, não no `GUILD_MEMBER_ADD` (https://github.com/discordjs/discord.js/issues/5163).
- Integrações (Twitch/YouTube) fazem bypass do screening.

### 6.2 Guild Onboarding [NATIVO, configurável via API]

- `GET/PUT /guilds/{id}/onboarding` — permissões **`MANAGE_GUILD` + `MANAGE_ROLES`**. Prompts com opções que atribuem roles/canais; `default_channel_ids`; modos `ONBOARDING_DEFAULT` (0) / `ONBOARDING_ADVANCED` (1). O bot pode gerir isto programaticamente.

### 6.3 Verification Level [NATIVO]

- `verification_level` no Modify Guild (0 NONE → 4 VERY_HIGH = telefone verificado). Bot pode subi-lo automaticamente durante um raid (`MANAGE_GUILD`) e combiná-lo com incident actions (§2.4).

### 6.4 Gate/captcha próprio [BOT]

Não existe captcha nativo exposto a bots. Padrão habitual: role de "não verificado" ou canais trancados por role + botão/interação (ou DM) que o bot valida antes de atribuir o role de acesso. Tudo lógica do bot com `MANAGE_ROLES`. Nota: se usar DMs, membros com DMs fechadas ficam presos — prever fallback por botão no canal.

---

## 7. discord.js v14 — mapeamento concreto e pegadinhas

Fontes: https://discord.js.org/docs/packages/discord.js/main/AutoModerationRuleManager:Class , https://discord.js.org/docs/packages/discord.js/main/GuildBanManager:Class , https://discordjs.guide/legacy/popular-topics/audit-logs

### 7.1 API surface

| Funcionalidade | discord.js v14 |
|---|---|
| Regras AutoMod | `guild.autoModerationRules` (`AutoModerationRuleManager`): `.create()`, `.edit()`, `.delete()`, `.fetch()`; enums `AutoModerationRuleTriggerType`, `AutoModerationRuleEventType`, `AutoModerationActionType` |
| Eventos AutoMod | `Events.AutoModerationActionExecution`, `Events.AutoModerationRuleCreate/Update/Delete` |
| Timeout | `member.timeout(ms, reason)` / `member.disableCommunicationUntil(date)`; checks: `member.moderatable`, `member.isCommunicationDisabled()` |
| Ban/unban | `guild.bans` (`GuildBanManager`): `.create(user, { deleteMessageSeconds, reason })`, `.remove()`, `.fetch()`; **`guild.bans.bulkCreate(users, options)`** → `{ bannedUsers, failedUsers }` |
| Kick | `member.kick(reason)` / `guild.members.kick()`; check `member.kickable` / `member.bannable` |
| Bulk delete | `channel.bulkDelete(messagesOuNúmero, filterOld)` |
| Audit logs | `Events.GuildAuditLogEntryCreate` (entry, guild) + `guild.fetchAuditLogs({ type: AuditLogEvent.X, limit })` |
| Incident actions | `guild.setIncidentActions({ invitesDisabledUntil, dmsDisabledUntil })` |
| Onboarding | `guild.fetchOnboarding()` / `guild.editOnboarding()` |
| Intents | `GatewayIntentBits.Guilds, GuildMembers, GuildModeration, GuildMessages, MessageContent, AutoModerationConfiguration, AutoModerationExecution, GuildWebhooks, GuildInvites` |

### 7.2 Pegadinhas conhecidas

1. **`bulkDelete(x, true)`**: sem `filterOld: true` rebenta com DiscordAPIError 50034 se houver mensagens >14 dias; com `true` filtra silenciosamente (pode "apagar 0").
2. **Intents privileged em dois sítios**: ligar no Developer Portal **e** passar em `new Client({ intents })`. Faltar no portal → crash 4014 ("Used disallowed intents") no login.
3. **`Events.GuildAuditLogEntryCreate`**: disponível desde v14.8; a entry traz `executorId`/`targetId` mas o objeto `executor` pode não estar em cache — fazer `client.users.fetch(executorId)`. Requer intent `GuildModeration` + permissão `ViewAuditLog`.
4. **Quem apagou a mensagem**: `messageDelete` não diz o autor da ação; cruzar com audit log `MessageDelete` (72), e mesmo assim: (a) self-deletes não geram entrada; (b) o Discord agrega deletes consecutivos do mesmo executor/alvo numa só entrada com `count` incrementado — a heurística por evento não é 100% fiável.
5. **Conteúdo em `messageDelete`/`messageUpdate` (versão antiga)** só existe se a mensagem estava em cache — dimensionar cache/sweepers (`makeCache`, `Options.cacheWithLimits`) e considerar `Partials.Message` para receber eventos de mensagens não-cacheadas (com campos vazios).
6. **Hierarquia antes de agir**: verificar `member.moderatable/kickable/bannable` antes de cada ação para não queimar requests em 403 (contam para o limite Cloudflare).
7. **Timeout em admin**: `member.moderatable` é false para admins/dono — o timeout falha; para "silenciar" um moderador comprometido (anti-nuke) o caminho é remover roles, não timeout.
8. **`reason`**: passar sempre a opção `reason` — discord.js envia `X-Audit-Log-Reason` e o histórico nativo fica completo.
9. **`guild.bans.bulkCreate`** lança erro se **nenhum** utilizador puder ser banido (500000); tratar `failedUsers` como caso normal.
10. **AutoMod regex**: padrões são Rust regex — testar num engine Rust (não no JS `RegExp`), senão `PATCH/POST` devolve 400.
11. **Deteção do fim do screening**: usar `guildMemberUpdate` comparando `oldMember.pending && !newMember.pending` — exige membros em cache (com `GuildMembers` intent a cache de membros fica completa num servidor único, sem problema).
12. **Rate limits**: discord.js/`@discordjs/rest` gere buckets e 429 automaticamente (fila interna) — não implementar sleeps manuais, mas evitar disparar centenas de `.delete()` individuais quando `bulkDelete`/`bulkCreate` existem.

---

## 8. Síntese NATIVO vs BOT (para o planeamento)

**O Discord já faz (o bot apenas configura/observa):** filtragem de keywords/regex/presets pré-publicação, mention spam + mention raid protection, spam genérico, filtro de perfil, block/alert/timeout/quarantine automáticos (AutoMod); enforcement de timeout até 28 dias; ban com purga de 7 dias de mensagens; bulk ban de 200; audit log de 45 dias com evento em tempo real; screening de regras (`pending`); onboarding com roles; verification levels; pausa de invites/DMs por 24 h (incident actions).

**O bot tem de implementar:** sistema de warns/infrações/escalada com persistência; anti-spam paramétrico (rate, duplicados, embeds/anexos); anti-raid por taxa de joins e heurísticas de conta + resposta automática (subir verification level, `setIncidentActions`, lockdown de canais, kick/ban em massa); anti-nuke por contagem de ações destrutivas por executor via `GuildAuditLogEntryCreate` (channels/roles/webhooks/bans) com resposta de remoção de roles; captcha/gate interativo; logging enriquecido (quem apagou o quê, com conteúdo cacheado); moderação de conteúdo de bots/webhooks (invisível ao AutoMod); qualquer ação sobre mensagens >14 dias (uma a uma).

### Fontes principais
- AutoMod: https://docs.discord.com/developers/resources/auto-moderation ; https://support.discord.com/hc/en-us/articles/4421269296535-AutoMod-FAQ ; https://discord.com/safety/auto-moderation-in-discord
- Guild (timeout, bans, bulk ban, incident actions, screening, onboarding): https://docs.discord.com/developers/resources/guild
- Audit logs: https://docs.discord.com/developers/resources/audit-log
- Gateway/intents: https://docs.discord.com/developers/events/gateway ; eventos: https://docs.discord.com/developers/events/gateway-events
- Rate limits: https://docs.discord.com/developers/topics/rate-limits ; bulk delete 14 dias: https://github.com/discord/discord-api-docs/issues/208
- Bulk ban types: https://discord-api-types.dev/api/discord-api-types-v10/interface/RESTPostAPIGuildBulkBanResult
- discord.js: https://discord.js.org/docs/packages/discord.js/main/AutoModerationRuleManager:Class ; https://discord.js.org/docs/packages/discord.js/main/GuildBanManager:Class ; https://discordjs.guide/legacy/popular-topics/audit-logs
- Screening/pending gotchas: https://github.com/discordjs/discord.js/issues/5163 ; https://github.com/discord/discord-api-docs/discussions/8016 ; incident actions 24 h: https://github.com/discord/discord-api-docs/issues/6797
