# Feature Scout — Carl-bot vs Vozen Helper — 2026-08-02

## Sumário executivo

O Vozen Helper tem uma fundação técnica mais segura e verificável do que a
documentação do Carl-bot demonstra: OAuth com PKCE, configuração por guild,
revisões, rollback, simulação, catálogo tipado e estados de maturidade honestos.
No entanto, o Carl-bot continua muito mais profundo nas ferramentas que um
administrador usa todos os dias. O catálogo atual do Helper contém 52 tópicos,
mas o runtime classifica apenas 13 como `operational`, 3 como `beta`, 20 como
`planned` e 16 como `blocked` [V:H1].

Os maiores gaps competitivos são: AutoMod granular e logging configurável;
cargos automáticos, temporários, persistentes e painéis avançados; recompensas e
administração de XP; tags/triggers expressivos; e um compositor reutilizável de
mensagens/embeds. O Helper já tem bons núcleos de moderação, tickets, eventos,
welcome, XP card, sugestões, sorteios e alertas, mas vários precisam de mais
opções, builders e validação de permissões para atingir a profundidade do
Carl-bot. `Games` foi excluído desta análise por decisão do proprietário.

## Fontes e âmbito

### Vozen Helper

- **H1** — [V] `crates/helper-core/src/lib.rs`: lista canónica das 52 chaves e
  maturidade operacional.
- **H2** — [V] `crates/helper-discord/src/lib.rs`: comandos, eventos e decisões
  efetivamente ligadas ao runtime Rust.
- **H3** — [V] `crates/helper-api/src/lib.rs`: configuração, contexto Discord,
  Quick Setup, providers, revisões, simulação e rollback.
- **H4** — [V] `panel/src/App.tsx`: navegação, catálogo, formulários e Quick
  Setup expostos ao utilizador.
- **H5** — [V] `deploy/PARITY-MATRIX.md`: limites de paridade assumidos pelo
  próprio projeto.

### Carl-bot

- **C1** — [V] [Documentação oficial](https://docs.carl.gg/)
- **C2** — [V] [Configuração](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/config.md)
- **C3** — [V] [Personalização](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/personalization.md)
- **C4** — [V] [AutoMod](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/automod.md)
- **C5** — [V] [Moderação](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/moderation.md)
- **C6** — [V] [Logging](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/logging.md)
- **C7** — [V] [Cargos e reaction roles](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/roles.md)
- **C8** — [V] [Níveis](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/levels.md)
- **C9** — [V] [Greetings](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/greetings.md)
- **C10** — [V] [Embeds](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/embeds.md)
- **C11** — [V] [Tags e triggers](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/tagstriggers.md)
- **C12** — [V] [Utilities](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/utilities.md)
- **C13** — [V] [Feeds](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/feeds.md)
- **C14** — [V] [Notifications](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/notifications.md)
- **C15** — [V] [Starboard](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/starboard.md)
- **C16** — [V] [Suggestions](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/suggestions.md)

## Posição atual do Vozen Helper

| Estado canónico | Quantidade | Leitura |
| --- | ---: | --- |
| Operacional | 13 | Runtime real: anti-spam, anti-raid, join gate, níveis, starboard, sugestões, giveaways, role panels, eventos, tickets, welcome, polls e XP card [V:H1/H2] |
| Beta | 3 | YouTube, Twitch e RSS têm adapters e configuração, mas ainda não são considerados maduros [V:H1/H3] |
| Planeada | 20 | O cartão existe no roadmap, sem promessa de comportamento operacional [V:H1/H4] |
| Bloqueada | 16 | Falta provider aprovado, credenciais, contrato externo ou decisão de produto [V:H1] |

Esta distinção é importante: um cartão no catálogo não conta como paridade com o
Carl-bot se não tiver configuração persistida, aplicação no Discord, auditoria,
testes e rollback.

## Matriz de features

Legenda: ✅ forte/operacional · ◐ existe mas está fraco ou incompleto · ❌ falta ·
⏸ deliberadamente fora de âmbito.

| Família | Carl-bot | Vozen Helper | Diagnóstico |
| --- | --- | --- | --- |
| Quick Setup | ✅ fluxo guiado [V:C1] | ✅ implementado com welcome, roles, moderação e proteção [V:H3/H4] | Preservar; melhorar recomendações e resumo de impacto à medida que os módulos amadurecem. |
| Configuração global e comandos | ✅ prefixos, canais ignorados e restrições de comandos [V:C2] | ◐ slash-first e feature toggles, sem matriz madura por comando/canal/cargo [V:H2/H4] | Prefixos não são necessários; controlo granular de utilização é um gap real. |
| Personalização do bot | ✅ nickname e perfil Premium [V:C3] | ❌ `management.nickname` está planeado; XP card não personaliza o bot [V:H1] | Implementar nickname por guild; avaliar avatar/banner apenas se a API e o modelo de produto o justificarem. |
| AutoMod | ✅ spam, menções, links, convites, anexos, caps, palavras, media-only, exceções e punições combináveis [V:C4] | ◐ anti-spam operacional; anti-scam planeado; anti-raid/join gate operacionais [V:H1/H2] | Maior gap P0: faltam regras granulares, exceções, ações e preview por regra. |
| Moderação manual | ✅ suite extensa e ferramentas de massa [V:C5] | ◐ warn, note, reason, timeout, kick, ban, tempban, softban, unban, purge, quarantine, slowmode e lockdown [V:H2/H5] | Núcleo forte; faltam massban, report, setnick, filtros de purge, DM/appeal e escalada configurável. |
| Logging e modlogs | ✅ destinos por evento, ignores, AIO e histórico detalhado [V:C6] | ◐ casos, audit API e atividade; logging Discord configurável ainda sem paridade [V:H3/H5] | Construir routing por evento, retenção, ignore lists, busca/export e diagnóstico de lacunas de cache. |
| Autoroles e sticky roles | ✅ join, delayed, reassign e blacklist [V:C7] | ◐ auto-role no welcome e restauro limitado ligado a segurança [V:H2/H5] | Falta um módulo coerente de autoroles, timed roles e sticky roles com hierarchy preflight. |
| Painéis/reaction roles | ✅ setup, múltiplos pares, whitelist/blacklist, unique, verify, binding, reversed, drop, lock, temp, linked e limits [V:C7] | ◐ painel simples de botões, até cinco cargos por comando [V:H2] | Grande gap P0/P1: builder, dropdowns, escala, modos, edição e recuperação. |
| Gestão de cargos | ✅ criar, atribuir, remover, cor, info, diagnóstico, temporários e bulk [V:C7] | ❌ não existe como módulo completo [V:H1/H2] | Adicionar apenas com confirmação, hierarquia, auditoria e limites rigorosos. |
| Níveis e XP | ✅ texto/voz, cooldowns, blacklist, ajustes, import, reset/backup, leaderboard e rewards [V:C8] | ◐ texto, cooldown, leaderboard, anúncios e XP card operacionais [V:H1/H2] | Falta voice XP, exclusions, rewards por cargo, ajustes admin, import, reset e backup/restore. |
| Greetings | ✅ welcome, DM, farewell, ban, birthdays e testes [V:C9] | ◐ welcome público/DM/auto-role operacional; welcome channel e birthdays planeados [V:H1/H2] | Acrescentar farewell, ban message, testes e placeholders; depois birthdays com privacidade. |
| Embed builder | ✅ criar, editar, obter JSON/source [V:C10] | ❌ planeado [V:H1/H4] | Peça transversal para welcome, roles, tickets, alerts, giveaways, suggestions e automations. |
| Feeds/autofeeds | ✅ mensagens programadas recorrentes [V:C13] | ◐ reminders e scheduler existem, mas não há composer/autofeed equivalente [V:H2] | Unificar recorrência, templates, timezone, histórico e pause/resume. |
| Twitch/YouTube | ✅ notificações maduras e com quotas [V:C14] | ◐ adapters reais em beta; RSS é uma vantagem adicional [V:H1/H2/H3] | Graduar de beta com observabilidade, retries, quotas, teste de entrega e UX de erros. |
| Starboard | ✅ configuração e gestão aprofundadas [V:C15] | ◐ operacional, mas com superfície de configuração mais pequena [V:H1/H2] | Acrescentar exceções, múltiplos boards/regras se houver procura, edição e reparação. |
| Sugestões | ✅ canais, votação e lifecycle de moderação [V:C16] | ◐ submissão, votos e estado existem [V:H1/H2] | Melhorar builder, permissões, anonimato opcional, threads, motivos e analytics. |
| Tags | ✅ TagScript, variáveis, embeds, permissões e command blocks [V:C11] | ◐ tags de texto simples [V:H2] | Falta uma linguagem segura e limitada, preview, versionamento e permissões. |
| Triggers/automações | ✅ triggers com diferentes modos de correspondência [V:C11] | ◐ MVP `message → contains → reply`, embora existam dry-run e runs [V:H2/H5] | Expandir catálogo bounded de triggers, condições e ações sem criar execução arbitrária. |
| Utilities | ✅ highlights, member dump, informação, reminders, polls, giveaways e sticky messages [V:C12] | ◐ AFK, reminders, polls, giveaways, userinfo e server stats [V:H2] | Faltam highlights, sticky messages, member dump seguro e ferramentas de informação mais completas. |
| Diagnóstico/ajuda | ✅ FAQ e comandos de diagnóstico [V:C1] | ◐ help, status, Permission Passport e health APIs [V:H2/H3] | A fundação do Helper é boa; ligar erros a passos de correção no painel. |
| Premium | ✅ ativação e perks próprios | ◐ entitlements e quotas sólidos, apresentação de valor ainda pouco específica [V:H3/H4] | Melhorar packaging por módulo sem bloquear segurança essencial. |
| Fun sem Games | ✅ respostas e utilitários sociais leves | ❌ não existe como família | Baixa prioridade; só adicionar se houver procura e sem desviar do core de administração. |
| Games | ⏸ existe no Carl-bot | ⏸ excluído | Fora de âmbito por decisão explícita. |

## Gaps — o que falta

### P0 — Table stakes

1. **AutoMod Rule Studio** — filtros granulares, exceções por cargo/canal,
   múltiplas ações, simulação e logging explicável. **Esforço L**; viabilidade
   alta com o registry, store, simulation e runtime existentes.
2. **Logging configurável de ponta a ponta** — eventos, destinos, ignores,
   retenção, pesquisa e export. **Esforço L**; exige cache bounded e política
   explícita para conteúdo de mensagens.
3. **Role Studio completo** — autoroles, timed/sticky roles, builders com
   botões/dropdowns e modos seguros. **Esforço L**; Serenity e o store já dão
   base, mas a hierarquia tem de ser validada antes de publicar.
4. **Profundidade de níveis** — role rewards, exclusions, voice XP, ajustes e
   ferramentas de migração/backup. **Esforço M/L**.
5. **Composer partilhado de mensagens/embeds** — componente reutilizado por
   welcome, tickets, roles, alerts, giveaways, suggestions e workflows.
   **Esforço M** e alto efeito multiplicador.

### P1 — Diferenciadores com procura comprovada

1. **Tags e automações expressivas mas bounded** — variáveis permitidas,
   conditions/actions tipadas, preview, dry-run e histórico. **Esforço L**.
2. **Greetings lifecycle completo** — farewell, ban, testes e birthdays com
   consentimento. **Esforço M**.
3. **Moderação avançada** — filtros de purge, reports, mass actions com dupla
   confirmação, escalada e appeals. **Esforço M/L**.
4. **Graduar alerts de beta** — dashboards de saúde por provider, retries,
   quotas e testes de entrega. **Esforço M**.
5. **Utilities de administração** — highlights, sticky messages e member dump
   com limites e privacidade. **Esforço M**.

### P2 — Avaliar antes de construir

- Personalização de avatar/banner do bot por servidor.
- Vários starboards por guild e regras muito avançadas.
- Fun commands sem jogos.
- Role management em massa para todos os administradores.

### Irrelevante ou descartado

- Prefixos customizados como prioridade: o Helper é slash-first.
- Games: excluído pelo proprietário.
- Copiar UI, texto, nomes ou código do Carl-bot: as capacidades podem inspirar o
  produto, mas a implementação e a identidade devem ser originais.

## O que o Vozen deve preservar

- Estado operacional honesto; um cartão planeado nunca aparece como ativo.
- Configuração atómica por guild com revision conflict, histórico e rollback.
- Simulação sem efeitos antes de publicar.
- Permission Passport, hierarchy preflight e explicações acionáveis.
- OAuth PKCE, sessão HttpOnly e isolamento entre guilds.
- XP card com apenas banners curados ou cores sólidas.
- Quick Setup em linguagem beginner-friendly e identidade visual própria.

## Oportunidades próprias do Vozen

1. **Impact Preview unificado** — antes de publicar, mostrar canais/cargos a
   criar, permissões necessárias, membros afetados e como reverter.
2. **Replay Lab** — testar AutoMod, triggers e greetings contra fixtures sem
   atuar sobre membros reais.
3. **Saúde por funcionalidade** — distinguir configuração inválida, permissão
   em falta, provider indisponível e runtime degradado num único painel.
4. **Configuração portátil com diff** — importar um template e rever diferenças
   de cargos/canais antes de o aplicar a outra guild.
5. **Beginner/Advanced dual mode** — presets seguros primeiro e opções profundas
   só quando o administrador as procura.

## Top 5 recomendações

1. **AutoMod + logging como um único programa P0**, porque uma regra sem
   evidência e explicação não é administrável.
2. **Role Studio completo**, substituindo o painel limitado por um builder com
   hierarchy preflight, preview e rollback.
3. **Composer de mensagens/embeds partilhado**, para evitar sete editores
   inconsistentes e acelerar welcome, alerts, tickets e automações.
4. **Levels 2.0**, focado em rewards, exclusions, voice XP e controlo admin —
   não em mais cosméticos do XP card.
5. **Tags + Automation Studio bounded**, com poder semelhante ao Carl-bot sem
   permitir código arbitrário nem esconder consequências.

## Próximo passo recomendado

Pedir ao Sol um plano técnico e de produto, não implementação. O plano deve
validar novamente o código, agrupar dependências partilhadas, priorizar por
valor/risco e definir critérios binários para uma funcionalidade mudar para
`operational`.
