# Feature Scout — Vozen Helper — 2026-07-31

## Sumário executivo

O Vozen Helper já cobre grande parte do núcleo funcional de Carl-bot e MEE6:
moderação com casos, anti-spam/scam/raid/nuke, quarentena, tickets com transcript
e SLA, self-roles, XP, sugestões, giveaways, starboard, polls, eventos, workflows
e privacidade. A vantagem competitiva dos outros dois não é apenas “ter mais
features”: é tornar cada capacidade fácil de encontrar, compreender, configurar e
testar.

O MEE6 tem o modelo mais beginner-friendly: entrada com Discord, catálogo de
plugins por objetivo, configuração progressiva, defaults, templates e fluxos
`Draft → Publish`. Carl-bot é visualmente mais simples e oferece uma camada Free
forte, mas a profundidade de reaction roles, TagScript, modos, IDs e permissões
cria uma curva de aprendizagem maior.

Os cinco gaps mais relevantes do Helper são:

1. painel realmente configurável, organizado por objetivos e não por módulos internos;
2. preflight de permissões e hierarquia antes de guardar/publicar;
3. editor visual de painéis de roles, embeds e mensagens com preview;
4. Automation Studio com mais triggers, condições, ações e simulação;
5. alertas sociais (YouTube, Twitch e RSS primeiro).

O Helper já é mais forte nos temas que melhor sustentam confiança: anti-nuke,
quarentena, shadow mode, casos, SLA, audit trail, Safety Health Score, Permission
Passport, privacidade e export/import de configuração. A recomendação principal é
usar estas forças para criar a experiência de configuração mais segura e
explicável da categoria, sem copiar os paywalls agressivos do MEE6 nem a
complexidade exposta do Carl-bot.

## Método, âmbito e legenda

Data de verificação: **31 de julho de 2026**.

O inventário do Vozen Helper foi feito a partir do runtime Rust, comandos
registados, API, matriz de paridade e protótipo React. Planos antigos e código Node
de rollback não foram tratados como produto atual quando não existe paridade Rust.

- **[V]** — verificado numa fonte primária ou no código atual.
- **[?]** — fonte secundária, opinião, inferência ou ausência não conclusiva.
- **✅** — capacidade existente.
- **◐** — parcial, limitada ou existe uma capacidade adjacente.
- **❌** — gap confirmado no inventário do Helper.
- **—** — não encontrada nas fontes oficiais consultadas, ou não aplicável. Não é
  prova absoluta de inexistência.
- **Premium** — existe, mas está condicionada a um plano pago.

### Fontes do Vozen Helper

- **H1** — [README](../README.md)
- **H2** — [matriz de paridade Rust](../deploy/PARITY-MATRIX.md)
- **H3** — [comandos e handlers Discord](../crates/helper-discord/src/lib.rs)
- **H4** — [API Rust](../crates/helper-api/src/lib.rs)
- **H5** — [protótipo React local](../../vozen-helper-panel-local/panel/src/App.tsx)

### Fontes primárias dos concorrentes

- **C1** — [Carl-bot](https://carl.gg/)
- **C2** — [Carl-bot Premium](https://carl.gg/get-premium)
- **C3** — [Carl-bot Docs](https://docs.carl.gg/)
- **C4** — [Carl-bot Getting Started](https://github.com/botlabs-gg/carlbot-docs/blob/master/docs/getting-started.md)
- **C5** — [Carl-bot Reaction Roles](https://github.com/CarlGroth/carlbot-docs/blob/master/roles/reaction-roles.md)
- **C6** — [Carl-bot AutoMod](https://github.com/CarlGroth/carlbot-docs/blob/master/moderation/automod.md)
- **M1** — [MEE6](https://mee6.xyz/pt)
- **M2** — [MEE6 Getting Started](https://help.mee6.xyz/support/solutions/articles/101000385394-getting-started-with-mee6)
- **M3** — [plugins e permissões do MEE6](https://help.mee6.xyz/support/solutions/articles/101000484903-what-permissions-does-mee6-need-)
- **M4** — [MEE6 Automations](https://help.mee6.xyz/support/solutions/articles/101000546996-getting-started-with-mee6-automations)
- **M5** — [MEE6 Premium, AI, Pro e Characters](https://help.mee6.xyz/support/solutions/articles/101000539351-understanding-mee6-services-from-premium-to-ai-to-pro)
- **M6** — [MEE6 Premium](https://mee6.xyz/pt/premium)
- **M7** — [MEE6 defaults](https://help.mee6.xyz/support/solutions/articles/101000529703-default-settings-for-mee6-plugins)
- **M8** — [MEE6 Economy](https://help.mee6.xyz/support/solutions/articles/101000536968-mee6-economy-plugin-features-and-limitations)
- **M9** — [MEE6 Giveaways](https://help.mee6.xyz/support/solutions/articles/101000446107-mee6-giveaways-plugin-for-discord)
- **M10** — [MEE6 Invite Tracker](https://help.mee6.xyz/support/solutions/articles/101000549514-mee6-invite-tracker-plugin-for-discord)

## Concorrentes analisados

| Produto      | Popularidade declarada                                                  | Modelo e preço observado                                                                                                                                                 | Licença/serviço                                                              |
| ------------ | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------- |
| Vozen Helper | [V:H1] Produto em rollout; sem número público de guilds no repositório  | [V:H1] Free; Plus €1,99; Premium €3,99/3 guilds ou €7,99/8 guilds                                                                                                        | Proprietário                                                                 |
| Carl-bot     | [V:C1] 14 814 816 servidores mostrados na homepage no dia da pesquisa   | [V:C2] Free + Premium por servidor via Patreon ou Discord; preço não apresentado na página pública                                                                       | Serviço alojado; licença do runtime atual não indicada. Documentação pública |
| MEE6         | [V:M2] Mais de 21 milhões de servidores, segundo a documentação oficial | [V:M5/M6] Free + Premium por servidor e produtos separados (AI, Pro, Characters, Web3). A página PT mostrou uma campanha temporária; preços variam por campanha/contexto | Serviço proprietário                                                         |

Os números são claims dos próprios produtos, não métricas auditadas de atividade.

## UI e onboarding beginner-friendly

### O que os dois fazem melhor

| Dimensão                 | Vozen Helper atual                                                                                    | Carl-bot                                                                                | MEE6                                                                            | Leitura para o Helper                                                          |
| ------------------------ | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Primeira ação            | [V:H3/H5] `/setup` existe; o painel abre num overview operacional                                     | [V:C1/C4] CTA único “Add to Discord”; dashboard recomendado para evitar comandos        | [V:M1/M2] Login Discord → servidor → `Setup`                                    | Começar pelo objetivo do utilizador, não pelo estado do runtime                |
| Modelo mental            | [V:H1/H5] 8 módulos abstratos: Core, Studio, Security, Support, Events, Community, Automate, Insights | [V:C3] Domínios reconhecíveis: Roles, Moderation, Logging, Greetings, Suggestions, Tags | [V:M2] Plugins independentes num índice + área de configuração                  | “Proteger”, “Receber membros”, “Roles”, “Suporte”, “Engagement”, “Automatizar” |
| Descoberta               | [V:H5] Sidebar e cards mostram estado, mas não expõem todas as tarefas configuráveis                  | [V:C1] Grelha curta com benefício e “Learn more”                                        | [V:M1] Catálogo por Management, Utilities, Social, Engagement, AI               | Criar um catálogo pesquisável com estado e tempo estimado                      |
| Configuração progressiva | [V:H5] Maioria das páginas é leitura; settings completas não estão no protótipo React                 | [V:C4/C5] Dashboard para básico, comandos/modos para profundidade                       | [V:M2/M4] Ativar plugin, abrir tabs, toggles e opções avançadas                 | Mostrar Essentials primeiro e Advanced em disclosure                           |
| Templates/defaults       | [V:H3] Setup e alguns defaults no runtime; sem biblioteca visível de starter kits                     | [V:C5] Setup interativo; reaction roles têm vários modos                                | [V:M7] Mensagens e configurações default para welcome, tickets, alerts e outros | Starter kits PT-PT com preview do que será criado                              |
| Preview/teste            | [V:H3] `workflow-dry-run`; não há preview transversal no painel                                       | [?:C5] Embed/reaction-role builder, mas relatos de UI inconsistente                     | [V:M9] Giveaway pode ser guardado e publicado; vários editores usam preview     | Generalizar `Draft → Preview → Test → Publish`                                 |
| Diagnóstico              | [V:H3/H4] Safety Health, Permission Passport e audit trail são bases fortes                           | [?:C5] Falhas de roles/permissões surgem frequentemente como “não aconteceu nada”       | [V:M3] Documenta permissões, mas sugere Administrator como atalho               | Transformar o Permission Passport num preflight acionável                      |
| Ajuda                    | [V:H1] Documentação técnica no repo; pouca ajuda contextual no protótipo                              | [V:C3/C4] Docs extensas, mas muito centradas em sintaxe e comandos                      | [V:M2] Knowledge base orientada a tarefas e screenshots                         | Ajuda inline curta + link “ver passo a passo”                                  |
| Linguagem                | [V:H5] Painel PT-PT; comandos e vários labels ainda em inglês                                         | [V:C3] Inglês                                                                           | [V:M1] Site localizado, incluindo português do Brasil                           | PT-PT consistente como diferenciação                                           |
| Personalidade visual     | [V:H5] Dark neon “mission control”, forte em operações                                                | [V:C1/C2] Roxo, formas simples, mascote e feature cards diretos                         | [V:M1] Mascote expressiva, ilustrações e linguagem acessível                    | Manter a marca Vozen, mas reduzir o tom técnico nos primeiros passos           |
| Pressão comercial        | [V:H1] Packaging simples e barato                                                                     | [V:C2] Comparação Free/Premium clara e pouco intrusiva                                  | [V:M5/M6] Vários produtos, campanhas, countdown e upsells                       | Mostrar plano/limites sem interromper configuração ou desativação              |

### Diagnóstico visual

O Helper já tem melhor “UI de operações” do que “UI de setup”. O overview com
health score, métricas, módulos e casos transmite confiança depois de o servidor
estar configurado. Para um principiante, porém, “Core”, “Studio” ou “Insights” não
respondem à pergunta “o que devo fazer agora?”.

MEE6 vence no caminho feliz porque reduz o produto a cards, verbos e plugins. Carl
vence em simplicidade visual e generosidade do tier Free. O modelo recomendado é:

1. **modo Início** orientado a objetivos;
2. **modo Operações** semelhante ao painel atual;
3. **modo Avançado** com toda a profundidade, audit trail e limites.

## Matriz completa de features

### Produto, setup, configuração e confiança

| Feature                                       |                         Vozen Helper |                        Carl-bot |                                 MEE6 | Fontes/notas                          |
| --------------------------------------------- | -----------------------------------: | ------------------------------: | -----------------------------------: | ------------------------------------- |
| Login Discord/OAuth                           |                               ✅ [V] |                          ✅ [V] |                               ✅ [V] | H2/H4 · C1 · M2                       |
| Multi-guild                                   |                               ✅ [V] |                          ✅ [V] |                               ✅ [V] | H1/H2 · C1 · M2                       |
| Setup guiado                                  |                       ◐ [V] `/setup` |    ✅ [V] quick setup/dashboard |           ✅ [V] Setup + plugin flow | H3 · C4 · M2                          |
| Catálogo modular de features                  |    ◐ [V] módulos, pouca configuração |                          ✅ [V] |                               ✅ [V] | H5 · C1/C3 · M2                       |
| Ativar/desativar módulos                      |             ✅ [V] via setup/runtime |                          ✅ [V] |                               ✅ [V] | H3 · C3 · M2                          |
| Painel totalmente editável                    |  ◐ [V] protótipo sobretudo read-only |                          ✅ [V] |                               ✅ [V] | H4/H5 · C4 · M2                       |
| Configuração por objetivo                     |                               ❌ [V] |                           ◐ [V] |                               ✅ [V] | H5 · C3 · M2                          |
| Presets/starter kits                          |                               ❌ [V] |                           ◐ [V] |            ✅ [V] defaults/templates | H3/H5 · C5 · M7                       |
| Ajuda contextual no painel                    |                               ❌ [V] |             ◐ [V] docs externas |            ✅ [V] artigos task-first | H5 · C3 · M2                          |
| Pesquisa de features por intenção             |                               ❌ [V] |                               — |            ◐ [V] catálogo/categorias | H5 · C3 · M2                          |
| Draft antes de publicar                       |          ◐ [V] workflows têm dry-run |                           ◐ [?] |    ✅ [V] em editores como Giveaways | H3 · M9                               |
| Preview Discord de mensagens/embeds           |                   ❌ [V] na UI atual |                        ✅/◐ [?] |                               ✅ [V] | H5 · C5 · M9                          |
| Teste/simulação                               | ◐ [V] workflow dry-run e shadow mode |                               — |                                ◐ [V] | H3 · M4                               |
| Rollback de configuração                      |       ◐ [V] config versionada/import |                               — |                                    — | H2/H4                                 |
| Export/import de configuração                 |                               ✅ [V] |                               — |                                    — | H2/H4                                 |
| Permissões mínimas/Permission Passport        |               ✅ [V] backend/comando | ◐ [V] docs e regras por comando | ◐ [V] matriz; Administrator sugerido | H2/H3 · C3 · M3                       |
| Preflight de role hierarchy antes de publicar |               ◐ [V] checks nas ações |                           — [?] |                                — [?] | H3; ausência na UI pública consultada |
| Safety Health Score                           |                               ✅ [V] |                               — |                                    — | H2/H4                                 |
| Audit trail estruturado                       |        ✅ [V] correlation ID/outcome |             ✅ [V] logs/modlogs |                 ✅ [V] audit logging | H4 · C1/C4 · M3                       |
| Privacidade: export/delete/receipt            |                               ✅ [V] |                               — |                ◐ [V] política/portal | H2/H4                                 |
| RBAC delegado no dashboard                    |                               ❌ [V] |       ◐ [V] Manage Server/Admin |            ✅ Premium [V] Bot Master | H4/H5 · C4 · M2                       |
| PT-PT consistente                             |                                ◐ [V] |                   ❌ [V] inglês |                  ◐ [V] PT-BR no site | H3/H5 · C3 · M1                       |

### Moderação e segurança

| Feature                               |                        Vozen Helper |                    Carl-bot |                         MEE6 | Fontes/notas       |
| ------------------------------------- | ----------------------------------: | --------------------------: | ---------------------------: | ------------------ |
| Warn, kick, ban, timeout/mute         |                              ✅ [V] |                      ✅ [V] |               ✅ Premium [V] | H2/H3 · C1/C4 · M3 |
| Tempban/tempmute                      |                              ✅ [V] |                      ✅ [V] |               ✅ Premium [V] | H3 · C4 · M3       |
| Softban/unban/untimeout               |                              ✅ [V] |                    ✅/◐ [V] |                        ◐ [V] | H3 · C3 · M3       |
| Purge/clear                           |                              ✅ [V] |                      ✅ [V] |                       ✅ [V] | H3 · C2/C3 · M3    |
| Slowmode/lock/unlock/lockdown         |                              ✅ [V] |                      ✅ [V] |                     ✅/◐ [V] | H3 · C3 · M3       |
| Casos numerados e histórico           |                              ✅ [V] |  ✅ [V] infractions/modlogs |        ◐ [V] moderation logs | H2/H3/H4 · C1 · M3 |
| Notas e edição de razão               |                              ✅ [V] |                    ✅/◐ [V] |                            — | H3 · C3            |
| Escalação por reincidência/política   |                            ✅/◐ [V] |                    ✅/◐ [V] |                        ◐ [V] | H2/H3 · C6 · M3    |
| Anti-spam/flood/duplicados            |                              ✅ [V] |                      ✅ [V] |               ✅ Premium [V] | H2 · C1/C6 · M3    |
| Mass mentions/links/invites           |                              ✅ [V] |                      ✅ [V] |               ✅ Premium [V] | H2 · C1/C6 · M3    |
| Bad words/slurs/normalização          |                              ✅ [V] |                      ✅ [V] |               ✅ Premium [V] | H2 · C1/C6 · M3    |
| Attachment/media filtering            |                            ✅/◐ [V] |                      ✅ [V] |                        ◐ [V] | H2 · C1/C6 · M3    |
| Whitelist/exempt roles e canais       |                              ✅ [V] |                      ✅ [V] |                       ✅ [V] | H2/H3 · C6 · M3    |
| AutoMod nativo + audit                |                              ✅ [V] |                       ◐ [V] |                        ◐ [V] | H2 · C6 · M3       |
| Join gate/verificação                 |                              ✅ [V] |                       ◐ [V] |                       ✅ [V] | H2/H3 · C3 · M7    |
| Anti-raid por burst de joins          |                              ✅ [V] |               ◐ [V] AutoMod |              ◐ [V] Moderator | H2/H3 · C6 · M3    |
| Anti-nuke por audit log               |                              ✅ [V] |                           — |                            — | H2/H3              |
| Quarentena de executor suspeito       |                              ✅ [V] |                           — |                            — | H2/H3              |
| Restauro avançado de roles            |             ◐ [V] ainda em paridade | ✅ Premium [V] sticky roles | ◐ [V] autoroles/role actions | H2 · C2 · M3       |
| Shadow mode antes de aplicar          |                              ✅ [V] |                           — |                            — | H2/H3              |
| Drama Watcher/votação de mods         |      ◐ [V] revisão humana via casos |              ✅ Premium [V] |                            — | H3 · C2/C6         |
| Logs delete/edit/join/leave/roles/voz |                  ◐ [V] Rust parcial |             ✅ [V] granular |              ✅ [V] granular | H2 · C1/C4 · M3    |
| Canais de log separados               |                               ◐ [V] |                      ✅ [V] |                       ✅ [V] | H2 · C1/C4 · M3    |
| Honeypot                              | ◐ [V] legado/config; confirmar Rust |                           — |                            — | H1/H2              |
| Anti-phishing/redirects/lookalikes    |                            ✅/◐ [V] |          ✅/◐ [V] bad links |                  ◐ [V] links | H2/H3 · C1 · M3    |
| Impersonation/decancer/dehoist        |       ◐ [V] legado/paridade parcial |                           — |                            — | H2                 |

### Comunidade, suporte e engagement

| Feature                                  |                            Vozen Helper |                     Carl-bot |                    MEE6 | Fontes/notas       |
| ---------------------------------------- | --------------------------------------: | ---------------------------: | ----------------------: | ------------------ |
| Welcome em canal                         |                                  ✅ [V] |                       ✅ [V] |          ✅ Premium [V] | H2 · C1/C3 · M7    |
| Welcome por DM                           |                                ✅/◐ [V] |                       ✅ [V] |          ✅ Premium [V] | H2 · C1/C4 · M7    |
| Goodbye/farewell                         |                                   ◐ [V] |   ✅ [V] separado em Premium |          ✅ Premium [V] | H2 · C1/C2 · M7    |
| Autorole no join                         |                ◐ [V] gate/verified role |                       ✅ [V] |          ✅ Premium [V] | H3 · C1/C3 · M3    |
| Self-role panels                         |               ✅ [V] até 5 roles/painel |                       ✅ [V] |                  ✅ [V] | H3 · C1/C5 · M3    |
| Modos normal/unique/verify/reversed      |                                ❌/◐ [V] |                       ✅ [V] |                ✅/◐ [V] | H3 · C5 · M3       |
| Reaction roles em grande escala          |                   ❌ [V] limite atual 5 | ✅ [V] 250 Free/1000 Premium |                  ✅ [V] | H3 · C2/C5 · M3    |
| Timed reaction roles                     |                                  ❌ [V] |               ✅ Premium [V] |  ◐ [V] automation roles | H3 · C2 · M4       |
| Voice-role links                         |                                  ❌ [V] |               ✅ Premium [V] |                       — | H3 · C2            |
| XP, rank e leaderboard                   |                                  ✅ [V] |               ✅ Premium [V] |          ✅ Premium [V] | H2/H3 · C2 · M3    |
| Role rewards por nível                   |                                ✅/◐ [V] |               ✅ Premium [V] |          ✅ Premium [V] | H2/H3 · C2 · M3    |
| Rank card visual customizável            |                                  ❌ [V] |                            — |      ✅ Pro/Premium [V] | H3 · M5            |
| Achievements                             |                                  ❌ [V] |                            — |                  ✅ [V] | H3 · M7            |
| Economy/moeda/loja/jogos                 |                                  ❌ [V] |              ◐ [V] fun/games |                  ✅ [V] | H3 · C3 · M8       |
| Tickets privados                         |                                  ✅ [V] |                            — |                  ✅ [V] | H2/H3 · M3/M7      |
| Claim/close/transcript                   |                                  ✅ [V] |                            — |                ✅/◐ [V] | H2/H3 · M3         |
| Routing, prioridade e notas internas     |                                  ✅ [V] |                            — |                   ◐ [V] | H3/H4 · M3         |
| SLA reminders                            |                                  ✅ [V] |                            — |                       — | H2/H3              |
| Avaliação de ticket 1–5                  |                                  ✅ [V] |                            — |                       — | H3                 |
| Sugestões com votos e decisão            |                                  ✅ [V] |                       ✅ [V] |                       — | H2/H3 · C1         |
| Sugestões anónimas/decision log          |                                   ◐ [V] |                       ✅ [V] |                       — | H3 · C1            |
| Giveaways persistentes                   |                                  ✅ [V] |            —/não evidenciado |                  ✅ [V] | H2/H3 · M9         |
| Role eligibility/reroll/list             |                                  ✅ [V] |                            — |                  ✅ [V] | H3 · M9            |
| Weighted odds/draft/publicação           |                                ❌/◐ [V] |                            — |                  ✅ [V] | H3 · M9            |
| Polls                                    |                                  ✅ [V] |            ◐/não evidenciado |          ✅ Premium [V] | H2/H3 · M3         |
| Starboard                                |                                  ✅ [V] |                       ✅ [V] |                       — | H2/H3 · C1/C2      |
| AFK                                      |                                  ✅ [V] |                            — |                       — | H2/H3              |
| Reminders                                |                                  ✅ [V] |                       ✅ [V] |                  ✅ [V] | H2/H3 · C1 · M7    |
| Tags/respostas guardadas                 |                          ✅ [V] simples |             ✅ [V] TagScript |  ✅ [V] custom commands | H2/H3 · C1/C3 · M2 |
| Custom commands com argumentos/variáveis |                   ◐ [V] tags + workflow |                       ✅ [V] |                  ✅ [V] | H3 · C1/C3 · M2    |
| Sticky messages                          |                    ◐ [V] legado/parcial |               ✅ Premium [V] | ◐ [V] timers/automation | H2 · C2 · M4       |
| Member/server stats                      |                                  ✅ [V] |                     ✅/◐ [V] |                  ✅ [V] | H2/H3/H4 · C3 · M3 |
| Statistics channels                      |            ◐ [V] member counter parcial |                            — |                  ✅ [V] | H2 · M3            |
| Invite tracker/leaderboard               | ❌/◐ [V] legado sem paridade confirmada |                            — |                  ✅ [V] | H2 · M10           |
| Birthdays                                |    ❌ [V] excluído por decisão anterior |                            — |                  ✅ [V] | H3 · M3/M7         |
| Music quiz                               |                                  ❌ [V] |                            — |                  ✅ [V] | H3 · M3            |
| Voice recording                          |                                  ❌ [V] |                            — |                  ✅ [V] | H3 · M3            |

### Automação, conteúdo, eventos e integrações

| Feature                                    |               Vozen Helper |              Carl-bot |              MEE6 | Fontes/notas          |
| ------------------------------------------ | -------------------------: | --------------------: | ----------------: | --------------------- |
| Workflows persistentes                     |                 ✅ [V] MVP | ✅ [V] triggers/feeds |            ✅ [V] | H2/H3/H4 · C1/C3 · M4 |
| Trigger por mensagem                       |                     ✅ [V] |                ✅ [V] |            ✅ [V] | H3 · C1/C3 · M4       |
| Condição “contém texto”                    |                     ✅ [V] |                ✅ [V] |            ✅ [V] | H3 · C3 · M4          |
| Vários triggers (join, role, reação, voz)  |                   ❌/◐ [V] |              ✅/◐ [V] |            ✅ [V] | H3 · C3 · M4          |
| Várias condições combinadas                |                     ❌ [V] |      ✅ [V] TagScript |            ✅ [V] | H3 · C3 · M4          |
| Ações role/channel/delete/delay            |                   ❌/◐ [V] |                ✅ [V] |            ✅ [V] | H3 · C3 · M4          |
| Botões em automações                       |                     ❌ [V] |                 ◐ [V] |            ✅ [V] | H3 · C3 · M4          |
| Dry-run                                    |                     ✅ [V] |                     — | —/não evidenciado | H3                    |
| Editor visual trigger-condition-action     |                     ❌ [V] |                 ◐ [V] |            ✅ [V] | H5 · C3 · M4          |
| Embed/message builder                      | ◐ [V] Studio/templates API |                ✅ [V] |            ✅ [V] | H4/H5 · C3 · M3       |
| Biblioteca de templates                    | ◐ [V] API, sem UI completa |    ✅ [V] tags/import |   ✅ [V] defaults | H4/H5 · C3 · M7       |
| Repeating/scheduled messages               |  ◐ [V] scheduler/reminders |                ✅ [V] |     ✅ [V] timers | H2/H3 · C1 · M3       |
| Twitch alerts                              |                     ❌ [V] |                ✅ [V] |    ✅ Premium [V] | H3 · C1/C2 · M3       |
| YouTube alerts                             |                     ❌ [V] |                ✅ [V] |    ✅ Premium [V] | H3 · C1/C2 · M3       |
| RSS/Reddit/X/Instagram/TikTok/Kick/Podcast |                     ❌ [V] |       —/parcial feeds |            ✅ [V] | H3 · C3 · M3          |
| Native Discord Scheduled Events            |  ✅ [V] lifecycle completo |                     — |                 — | H2/H3                 |
| Event registration/check-in/attendees      |                     ✅ [V] |                     — |                 — | H3                    |
| Temporary text/voice channels              |                     ❌ [V] |        ✅ Premium [V] |            ✅ [V] | H3 · C2 · M3          |
| Game deals/notifications                   |                     ❌ [V] |                ✅ [V] |                 — | H3 · C1/C3            |
| Public server discovery                    |                     ❌ [V] |                ✅ [V] |                 — | H3 · C1               |

### IA, branding, monetização e operações

| Feature                      |            Vozen Helper |                     Carl-bot |                         MEE6 | Fontes/notas       |
| ---------------------------- | ----------------------: | ---------------------------: | ---------------------------: | ------------------ |
| Brand/templates por guild    |     ✅/◐ [V] API Studio |  ◐ [V] embeds/personalização |                       ✅ [V] | H4/H5 · C1/C3 · M3 |
| Custom avatar/banner do bot  | ❌ [V] identidade única |               ✅ Premium [V] |                  ✅ pago [V] | H1/H4 · C2 · M5    |
| AI text/image generation     |                  ❌ [V] |                            — |            ✅ produto AI [V] | H3 · M5            |
| AI Characters/Backstory      |                  ❌ [V] |                            — |      ✅ produto separado [V] | H3 · M5            |
| IA aplicada à moderação      |                  ❌ [V] |                            — |         — [V] não encontrada | H3 · C3 · M5       |
| Monetização de membros/perks |                  ❌ [V] |                            — |                       ✅ [V] | H3 · M3            |
| Web3/NFT gating              |                  ❌ [V] |                            — |      ✅ produto separado [V] | H3 · M3/M5         |
| Quotas por plano             |                  ✅ [V] |                       ✅ [V] |                       ✅ [V] | H4 · C2 · M5       |
| Status/health operacional    |      ✅ [V] API/runtime |           ✅ [V] status page |           ✅ [V] status page | H2/H4 · C1 · M2    |
| API autenticada por guild    |                  ✅ [V] |                            — |                            — | H4                 |
| Isolamento tenant testado    |                  ✅ [V] | não verificável publicamente | não verificável publicamente | H2/H4              |
| Correlation IDs e outcomes   |                  ✅ [V] |                            — |                            — | H4                 |
| Configuração versionada      |                  ✅ [V] |                            — |                            — | H2/H4              |

## Gaps — o que falta ao Vozen Helper

### P0 — Table stakes de produto

| Gap                                      | Quem tem                                                                          | Porque importa                                                                                          | Esforço | Viabilidade                                                       |
| ---------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------: | ----------------------------------------------------------------- |
| Control plane editável por objetivos     | Carl e MEE6                                                                       | O Helper já tem capacidades, mas o iniciante não as consegue configurar de ponta a ponta no React panel |       L | Alta; React/Axum/Store já existem, faltam endpoints e editores    |
| Preflight de permissões e role hierarchy | Os concorrentes documentam o problema, mas não demonstram uma solução transversal | Evita o erro mais comum: guardar e nada acontecer                                                       |       M | Alta; Permission Passport e checks de hierarquia já existem       |
| `Draft → Preview → Test → Publish`       | MEE6 parcialmente                                                                 | Cria confiança e reduz alterações acidentais no Discord                                                 |     M/L | Alta; workflow dry-run, audit e config versionada são bases úteis |

### P1 — Gaps funcionais com valor claro

| Gap                                | Quem tem        | Porque importa                                                                                  | Esforço | Viabilidade                                                 |
| ---------------------------------- | --------------- | ----------------------------------------------------------------------------------------------- | ------: | ----------------------------------------------------------- |
| Role/Panel Studio avançado         | Carl e MEE6     | Self-roles são table stakes, mas o Helper está limitado a 5 roles e sem modos avançados/preview |       M | Alta; Serenity components e store atual suportam evolução   |
| Embed/message composer             | Carl e MEE6     | É peça transversal a welcome, roles, tickets, alerts, giveaways e automations                   |       M | Alta; Studio templates API já existe                        |
| Automation Studio completo         | Carl e MEE6     | O MVP atual só cobre mensagem + contains + reply                                                |       L | Alta, desde que triggers/actions sejam bounded e auditáveis |
| Social Alerts (YouTube/Twitch/RSS) | Carl e MEE6     | Relevante para creators e comunidades sem desviar do core                                       |     M/L | Média/alta; reqwest + scheduler existem, APIs variam        |
| RBAC delegado no painel            | MEE6 Bot Master | Equipas precisam de separar owner, admin, mod e support                                         |       M | Alta; sessões/guild isolation/audit já existem              |

### P2 — Avaliar consoante o público-alvo

| Gap                           | Quem tem    | Leitura                                                        | Esforço | Decisão sugerida               |
| ----------------------------- | ----------- | -------------------------------------------------------------- | ------: | ------------------------------ |
| Temporary voice hubs          | Carl e MEE6 | Útil para gaming/estudo; não é universal                       |       M | Validar procura                |
| Invite tracker/referrals      | MEE6        | Bom para growth e reward loops                                 |       M | Validar creators/comunidades   |
| Economy + shop + achievements | MEE6        | Engagement alto, mas cria uma nova economia para operar        |       L | Só após definir posicionamento |
| White-label/custom bot        | Carl e MEE6 | Potencial add-on, mas multiplica risco de tokens e suporte     |       L | Adiar                          |
| Rank cards                    | MEE6        | Polimento visível, valor menor do que o editor de configuração |       M | P2/P3                          |

### Irrelevante ou a descartar por agora

- **AI Characters e geração de imagem/texto** — produto adjacente; não reforça
  segurança, operações ou comunidade administrável.
- **Web3/NFT gating** — nicho, dependências externas e risco de suporte.
- **Voice recording** — risco de consentimento/privacidade sem um caso de uso forte.
- **Music quiz e game deals** — divertidos, mas desalinhados com o posicionamento.
- **Server discovery** — exige moderar um marketplace/diretório adicional.
- **Birthdays** — já foi removido deliberadamente do scope do Helper.
- **Monetização de membros** — payments, impostos, refunds e compliance são um produto
  próprio; não deve bloquear a maturidade do Helper.

## Queixas dos utilizadores da concorrência

As queixas abaixo são sinais de oportunidade, não factos universais.

### MEE6

- **[V]** O packaging é fragmentado entre Premium, AI, Pro, Characters e Web3,
  cada um com âmbito próprio. [Fonte oficial](https://help.mee6.xyz/support/solutions/articles/101000539351-understanding-mee6-services-from-premium-to-ai-to-pro)
- **[V]** Polls começou gratuito e passou para Premium; a documentação explica que
  a desativação pode exigir contornar a modal Premium.
  [Fonte oficial](https://help.mee6.xyz/support/solutions/articles/101000490535-how-to-disable-polls-plugin-and-commands)
- **[?]** Threads de 2024–2026 repetem frustração com features básicas atrás de
  paywall e novos produtos não incluídos em “lifetime”.
  [Reddit 2025](https://www.reddit.com/r/discordapp/comments/1kon2am/literally_everything_mee6_is_used_for_is_now/),
  [Reddit 2025](https://www.reddit.com/r/discordapp/comments/1izj5k2/am_i_not_understanding_whats_going_on_or_are/)
- **[V]** Alguns fluxos atravessam dashboard, Discord Integrations e Developer
  Portal; o Custom Bot exige tokens, intents, redirect URI e role hierarchy.
  [Fonte oficial](https://help.mee6.xyz/support/solutions/articles/101000442591-how-to-set-up-mee6-custom-bot)
- **[V]** A documentação recomenda Administrator como atalho para permissões. É
  fácil, mas contrário a least privilege.
  [Fonte oficial](https://help.mee6.xyz/support/solutions/articles/101000484903-what-permissions-does-mee6-need-)

**Oportunidade:** packaging estável, permitir sempre ver/desativar/exportar uma
configuração e explicar permissões/dados antes de ativar.

### Carl-bot

- **[V]** Reaction Roles expõe muitos modos e conceitos: unique, verify, reversed,
  binding, temporary, limits, blacklist/whitelist e IDs.
  [Fonte oficial](https://github.com/CarlGroth/carlbot-docs/blob/master/roles/reaction-roles.md)
- **[?]** Há relatos recorrentes de `Create`/`Save` não produzir efeito na UI de
  Reaction Roles, misturando bugs reais com role hierarchy/permissões incorretas.
  [Issue #50](https://github.com/CarlGroth/Carl-Bot/issues/50),
  [Reddit](https://www.reddit.com/r/Discord_Bots/comments/1dz6xdg/carl_bot_reaction_roles_not_working/)
- **[?]** Iniciantes descrevem comandos, IDs e automações recorrentes como
  confusos.
  [Reddit](https://www.reddit.com/r/carlbot/comments/1avtbi3)
- **[V]** A documentação encaminha suporte para um Discord externo.
  [Fonte oficial](https://docs.carl.gg/)

**Oportunidade:** preservar profundidade, mas esconder complexidade até ser pedida;
diagnosticar roles/permissões antes de publicar; manter suporte acionável dentro do
painel.

## Features inovadoras — o que nenhum dos dois demonstra de forma completa

As propostas abaixo não foram encontradas como experiências transversais nos
catálogos oficiais consultados. Isto é uma observação de pesquisa, não prova
absoluta de inexistência.

### 1. SafeStart — setup por objetivo com least privilege

- **Descrição:** wizard “Proteger”, “Receber membros”, “Criar roles”, “Suporte” ou
  “Engagement”; recomenda módulos, calcula permissões mínimas, verifica role
  hierarchy e só então publica.
- **Evidência:** MEE6 prova o valor do plugin flow; queixas de ambos mostram falhas
  opacas de permissões.
- **Porque encaixa:** Permission Passport e hierarchy checks já existem.
- **Esforço:** M.
- **Definition of Done:** um servidor novo completa um starter kit PT-PT, vê todas
  as permissões pedidas com motivo, corrige bloqueios e termina com checklist verde.

### 2. Replay Lab para segurança e automações

- **Descrição:** simular mensagens, joins, bursts, alterações de roles e workflows
  contra a configuração, sem punir ninguém.
- **Evidência:** MEE6 documenta limitações/rate limits; Carl expõe complexidade;
  o Helper já tem shadow mode e workflow dry-run.
- **Porque encaixa:** diferencia a profundidade de segurança sem a tornar perigosa.
- **Esforço:** M/L.
- **Definition of Done:** o utilizador fornece fixtures seguras, vê que regra
  dispararia, ação, quota, log e razão, sem side effects.

### 3. Trust Center por módulo

- **Descrição:** dados lidos, dados guardados, retenção, permissões atuais/necessárias,
  subprocessadores, export/delete e botão “reduzir permissões”.
- **Evidência:** MEE6 tem permissões extensas e packaging complexo; suspeitas de
  privacidade surgem mesmo sem prova técnica.
- **Porque encaixa:** o Helper já tem GDPR, privacy receipt e Permission Passport.
- **Esforço:** M.
- **Definition of Done:** cada módulo tem uma ficha gerada do comportamento real e
  um admin consegue exportar/apagar os dados elegíveis.

### 4. Config Migration Concierge

- **Descrição:** importar estruturas autorizadas (roles, canais, welcome, automod,
  reaction-role mappings), apresentar diff e criar um plano de migração.
- **Evidência:** paywalls do MEE6 criam intenção de mudança; painéis do Carl têm
  custo de reconstrução.
- **Porque encaixa:** config import/export versionado já existe.
- **Esforço:** L, dependente de APIs e formatos disponíveis.
- **Definition of Done:** nenhuma alteração sem preview; cada item mostra origem,
  destino, conflito e rollback.

### 5. Explainable Incident Timeline

- **Descrição:** uma timeline liga trigger, regra, mensagem/evento redigido, ação,
  caso, moderator override, outcome e recomendação.
- **Evidência:** logs dos concorrentes são amplos, mas o Helper já tem correlation IDs,
  casos e health score.
- **Porque encaixa:** transforma segurança avançada em linguagem operacional.
- **Esforço:** M.
- **Definition of Done:** um incidente pode ser compreendido e exportado sem expor
  tokens ou conteúdo desnecessário.

### 6. Plan Impact Simulator

- **Descrição:** antes de upgrade/downgrade, mostrar o que continua ativo, o que
  pausa, quotas afetadas e garantir que desativar/exportar nunca fica bloqueado.
- **Evidência:** backlash do MEE6 concentra-se em contrato percebido e paywalls;
  Carl explica que dados Premium ficam guardados.
- **Porque encaixa:** entitlements e quotas já são centrais no ecossistema Vozen.
- **Esforço:** S/M.
- **Definition of Done:** cada plano tem matriz pública estável e preview do impacto
  por guild antes de confirmar.

## Top 5 recomendações

1. **SafeStart + catálogo por objetivos** — maior ganho de perceção, ativação e
   beginner-friendliness; aproveita funcionalidades já existentes.
2. **Preflight de permissões/role hierarchy** — elimina a falha “guardei e não
   aconteceu nada” e reforça o diferencial de segurança.
3. **Fluxo `Draft → Preview → Test → Publish`** — padrão transversal para roles,
   welcome, tickets, embeds, automations e alerts.
4. **Role & Message Studio** — painéis com mais de 5 roles, select menus, modos
   normal/unique/verify, embed builder e preview.
5. **Automation Studio + Social Alerts inicial** — expandir triggers/condições/ações
   de forma bounded e lançar YouTube/Twitch/RSS como primeiros conectores.

## Decisão pedida

Este relatório não implementa nenhuma feature nem altera o backlog. Para avançar,
o owner deve aprovar explicitamente quais recomendações entram no `BACKLOG.md` e
em que ordem.
