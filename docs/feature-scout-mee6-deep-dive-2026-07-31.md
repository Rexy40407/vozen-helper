# MEE6 deep dive — Vozen Helper — 31 de julho de 2026

## Decisão executiva

O Vozen Helper não está tão atrás do MEE6 em capacidade técnica como o painel faz parecer. O runtime já tem moderação profunda, casos, anti-raid, anti-nuke, quarentena, tickets com transcript e SLA, XP, rank card, self-roles, polls, giveaways, starboard, eventos, reminders, tags e workflows. O problema principal é de produto: quase tudo isto está escondido do principiante.

O MEE6 ganha sobretudo em quatro pontos:

1. catálogo organizado por tarefas reconhecíveis;
2. ativação em um clique e configuração progressiva;
3. defaults, templates e editores visuais;
4. marketing orientado ao resultado, não à arquitetura interna.

Os maiores gaps reais do Vozen são alertas sociais, achievements, economia, canais temporários, invite tracker completo, monetização, Web3, IA e personalização da identidade do bot. Nem todos devem ser copiados: Web3, monetização, IA e custom bot criam produtos e riscos operacionais próprios.

## Método e confiança

Esta investigação foi feita sem implementar código, sem editar o backlog e sem usar endpoints privados do MEE6. Cruzei:

- as onze capturas fornecidas pelo owner;
- documentação pública oficial do MEE6;
- termos oficiais do MEE6;
- fontes jurídicas oficiais da União Europeia e portuguesas;
- runtime Rust, API e painel React atuais do Vozen Helper.

Legenda:

- [V] verificado em fonte primária ou código atual;
- [?] observado na captura ou inferido, mas sem documentação oficial suficiente;
- Existe: capacidade funcional presente no Vozen;
- Parcial: existe uma base, mas faltam profundidade, UX ou paridade;
- Falta: não foi encontrada no runtime Rust e painel atuais.

## Como o MEE6 apresenta e vende o produto

### Arquitetura do dashboard

O MEE6 usa dois níveis de navegação ao mesmo tempo:

- uma sidebar persistente com servidor atual, páginas centrais e categorias expansíveis;
- uma grelha de cards que funciona como loja/catálogo de plugins.

As categorias são escritas na linguagem do dono do servidor: Essenciais, Gerir Servidor, Utilidades, Alertas Sociais, Engagement & Diversão, Monetização e Web3. Cada card tem ícone, nome curto, promessa de uma ou duas linhas, estado e um único CTA. Premium, Novo e Ativo são visíveis antes de abrir a feature.

O fluxo mental é consistente:

1. escolher o servidor;
2. descobrir uma feature pelo objetivo;
3. ativar;
4. começar de um default ou template;
5. preencher apenas o essencial;
6. guardar como rascunho ou publicar;
7. encontrar opções avançadas e ajuda dentro do contexto.

A documentação oficial chama às duas zonas principais Plugin Index e Plugin Configuration. Isto confirma que a organização não é apenas visual: o produto inteiro é pensado como catálogo → editor. [Getting Started](https://help.mee6.xyz/support/solutions/articles/101000385394-getting-started-with-mee6)

### O que torna a experiência beginner-friendly

- Os nomes dizem o resultado: Receber membros, Níveis, Tickets, Alertas, Sorteios.
- Os plugins continuam visíveis quando estão bloqueados; o utilizador percebe o que existe.
- Mensagens default evitam o ecrã vazio em Welcome, Tickets, Levels, Reminders, Alertas, Birthdays e Achievements. [Defaults oficiais](https://help.mee6.xyz/support/solutions/articles/101000529703-default-settings-for-mee6-plugins)
- Reaction Roles oferece template de verificação e opção de começar do zero.
- Giveaways separa Save de Publish.
- Permissões de roles e canais aparecem por comando.
- A base de conhecimento repete exatamente os caminhos do dashboard e usa tarefas concretas.

### Onde o MEE6 também falha

- A mesma feature aparece em mais de uma categoria, criando duplicação.
- A sidebar fica muito longa.
- Premium, Pro, AI, AI Characters, Web3 e Monetize são produtos independentes, com entitlement confuso. O próprio suporte precisa de explicar a diferença. [Serviços MEE6](https://help.mee6.xyz/support/solutions/articles/101000539351-understanding-mee6-services-from-premium-to-ai-to-pro)
- Os cards escondem limites, permissões perigosas, dependências e restrições até depois do clique.
- A documentação sugere Administrator como atalho, embora publique uma matriz detalhada de permissões. [Permissões MEE6](https://help.mee6.xyz/support/solutions/articles/101000484903-what-permissions-does-mee6-need-)
- A grelha dá peso visual semelhante a onboarding essencial, IA, Web3 e upsells.

O Vozen deve copiar o princípio de clareza, não esta pressão comercial nem a confusão entre subscrições.

## Matriz completa das features mostradas

### Entrada, gestão e segurança

| Feature MEE6 | Como funciona no MEE6 | Estado no Vozen | O que falta ou deve melhorar |
|---|---|---|---|
| Dashboard | Servidor → catálogo → configuração de plugin | Parcial | O painel atual só expõe Overview e Rank card; precisa de catálogo, estados, pesquisa e configuração das capacidades existentes. |
| Welcome & Goodbye | Canal, DM, welcome card, farewell, verificação e auto-role, com variáveis e defaults [V] | Parcial | Welcome existe; falta um fluxo visual único para canal, DM, despedida, card, role e teste. |
| Welcome Channel | Destino estruturado para recém-chegados, gerido pelo bot [?] | Falta/adjacente | Welcome normal não substitui uma área guiada de regras, informação e primeiros passos. |
| Reaction Roles | Template ou zero; texto/embed; emoji, botão ou dropdown; múltiplas escolhas; publicar [V] | Parcial | Self-role panels existem, mas estão limitados e sem builder visual, modos verify/unique e escala. |
| Moderator | Comandos, AutoMod, immunity roles, filtros, razões e audit logs [V] | Existe, backend mais forte | Expor configuração por objetivo, permissões por ação, preview da consequência e preflight de hierarchy. |
| Automations | Trigger + condições + ações obrigatórias; Save/Publish [V] | Parcial | O MVP do Vozen cobre sobretudo mensagem + contains + reply. Faltam triggers de join/role/reação/voz, condições combinadas, ações e editor visual. |
| Custom Commands | Resposta, variáveis, argumentos, defaults, role actions e acesso por role/canal [V] | Parcial | Tags e workflows cobrem o básico; faltam argumentos, variáveis claras, defaults, ações e editor/teste. |
| Invite Tracker | Links próprios, /invites, /inviter e leaderboard [V] | Falta/paridade antiga não confirmada | Implementação Rust persistente, anti-abuso, atribuição transparente e painel de campanhas. |
| Tickets | Painel com botão cria canal privado e mensagem inicial [V] | Existe, mais forte | O Vozen já tem claim, close, transcript, routing, notas, SLA e rating; falta tornar a configuração e publicação acessíveis no painel. |
| Settings | Comandos globais e allow/deny por roles/canais; Bot Master delegado [V] | Parcial | Backend tem permissões, quotas, saúde, privacidade e import/export; falta uma área coerente, delegação RBAC e linguagem simples. |

### Progressão, comunidade e utilidades

| Feature MEE6 | Como funciona no MEE6 | Estado no Vozen | O que falta ou deve melhorar |
|---|---|---|---|
| Levels | XP por texto, exclusões, mensagem, role rewards e /rank [V] | Existe | Rank card já está implementado; faltam no painel regras de XP, cooldown/anti-farm, exclusões, role rewards, reset e explicação do cálculo. |
| XP Leaderboard | Página que pode ser pública e indexada; CTA de entrada Premium [V] | Existe no bot, painel parcial | Página pública própria, privacidade opt-out, filtros por período e melhor ligação entre leaderboard, rank e rewards. |
| Rank card | Default do servidor via Premium; cartão pessoal via Pro [V] | Existe | Continuar com banners e composição originais; acrescentar apenas UX própria, acessibilidade e publicação segura. |
| Achievements | Marcos com anúncio e variáveis de jogador/conquista [V] | Falta | Motor de objetivos, progresso, recompensas, anti-abuso e biblioteca original de achievements. |
| Starboards | Mensagens populares destacadas num canal [?] | Existe | Configuração visual de canal, emoji, threshold, conteúdo permitido, opt-out e moderação. |
| Emojis | Plugin confirmado; mecânica pública atual pouco documentada [V/?] | Falta | Baixa prioridade: definir primeiro o problema real — gestão, pack, sugestão ou comandos de emoji. |
| Polls | /poll, votação e /poll-end [V] | Existe | Builder visual, preview, calendário, permissões, resultados/export e templates. |
| Embed Messages | Composer de título, corpo, fields, links, markdown e mentions [V] | Parcial | A API Studio/templates existe; falta editor visual completo com preview Discord, teste, draft e publish. |
| Search Anything | Pesquisa externa dentro do Discord; providers atuais não documentados [?] | Falta | Só vale fazer com providers definidos, filtros de segurança e proposta clara; não é gap prioritário. |
| Help | Plugin/comando de ajuda [V] | Existe | Transformar /help em ajuda contextual também no painel, com pesquisa por intenção e exemplos. |
| Timers / Reminders | Mensagens configuráveis e agendadas/recorrentes [V/?] | Parcial | Reminders existem; falta calendário visual, recorrência, timezone, histórico e controlo de falhas. |
| Statistics Channels | Canais cujo nome mostra contadores [V] | Parcial | Generalizar o counter atual para membros, boosts, online, YouTube/Twitch e limites de atualização. |
| Temporary Channels | Hubs criam espaços temporários e gerem owner/permissões [V] | Falta | Lifecycle seguro, ownership transfer, limite por guild, limpeza automática e templates. |
| Giveaways | Builder, XP/coins como prémio, requisitos, weighted odds, Save/Publish e reroll [V] | Existe | Já tem persistência, eligibility, list, end e reroll; faltam draft/publicação visual, odds ponderadas, preview e campanhas. |
| Birthdays | Data, anúncio, idade opcional e birthday role [V] | Falta | Só deve entrar com privacidade por defeito, ano opcional, timezone e opt-out. |
| Economy | Coins por daily/work/jogos, streak, loja, roles/items e boosts [V] | Falta | É um produto novo: ledger, antifraude, balanceamento, loja, sinks, moderação e suporte. Não é quick win. |

### Alertas sociais

O padrão do MEE6 é provider → conta/feed público → canal Discord → mensagem/mention → guardar. As integrações dependem de atrasos e restrições dos providers. O MEE6 documenta limites Premium de 300 Twitch, 10 TikTok, 75 X, 10 Bluesky, 100 YouTube, Reddit, Instagram e RSS, 300 Kick e 100 Podcasts. [Limites oficiais](https://help.mee6.xyz/support/solutions/articles/101000490834-mee6-social-alerts-for-discord-limits-and-restrictions)

| Provider | MEE6 | Vozen | Leitura |
|---|---|---|---|
| Twitch | Live alert, mensagem/role/canal, preview atualizado e auto-delete [V] | Falta | Prioridade alta para comunidades de creators. |
| YouTube | Novo vídeo; limitações para lives, privados, agendados e members-only [V] | Falta | Prioridade alta; explicar claramente o que não é suportado. |
| RSS | Feed universal, até 100 no plano documentado [V] | Falta | Melhor terceiro conector: cobre sites, blogs e podcasts sem integração específica. |
| TikTok | Novo vídeo [V] | Falta | Prioridade média; limite e estabilidade do provider são riscos. |
| X / Twitter | Nova publicação [V] | Falta | Prioridade média/baixa devido a custo e alterações frequentes de API. |
| Bluesky | Provider suportado; filtros não documentados [V/?] | Falta | Pode ser mais simples e aberto, mas a procura deve ser validada. |
| Reddit | Novas publicações; requer webhook no MEE6 [V] | Falta | Prioridade média depois de RSS. |
| Instagram | Nova publicação [V] | Falta | APIs/restrições tornam suporte mais caro. |
| Kick | Live alert [V] | Falta | Útil para creators depois de Twitch. |
| Podcasts | Novo episódio com variáveis [V] | Falta | RSS pode entregar grande parte do valor primeiro. |

### Receita, Web3, identidade e IA

| Feature MEE6 | Como funciona | Estado no Vozen | Recomendação |
|---|---|---|---|
| Monetize | Memberships/donations dão roles, canais ou benefícios; Stripe trata billing [V] | Falta | Adiar. Pagamentos, IVA, refunds, chargebacks, KYC e suporte transformam isto num produto financeiro/comercial. |
| NFT/Crypto Statistics | Canais com OpenSea/CoinGecko, refresh de 20/5 minutos [V] | Falta | Nicho; não reforça o posicionamento atual do Vozen. |
| NFT queries | Pesquisa de coleções/ativos [?] | Falta | Adiar. |
| NFT sales/listings | Alertas de vendas e listagens [?] | Falta | Adiar. |
| Crypto statistics/queries | Preços e dados de moedas [V/?] | Falta | Adiar; provider e compliance de comunicação financeira. |
| Gas Tracker | Custo de transações; mecânica atual não documentada [?] | Falta | Adiar. |
| Gating | Wallet connect, regra de holdings e role automática [V] | Falta | Alto risco operacional, de segurança, fraude e privacidade; fora do core. |
| Bot Personalizer | Dono cria app Discord, fornece token/secret e muda avatar/nome/status [V] | Falta | Não prioritário. Guardar tokens de bots de terceiros aumenta muito o blast radius e suporte. |
| AI Characters | Personas instaladas por servidor e canais permitidos [V] | Falta | Produto separado; só considerar com objetivo e política de segurança próprios. |
| MEE6 AI | Geração de texto/imagem e backstory, com quotas [V] | Falta | Não copiar como checkbox. Procurar primeiro um caso Vozen específico, como explicação de moderação ou setup assistido. |
| Premium | Plugins pagos por servidor; Pro, AI, Characters e Web3 são separados [V] | Existe packaging próprio | Manter planos Vozen simples e limites explicáveis; não repetir a fragmentação do MEE6. |

## O que o Vozen tem, mas precisa de mostrar e melhorar

### 1. Moderação e segurança

Este é o maior ativo escondido. O Vozen tem mais profundidade operacional do que a promessa visível do MEE6: casos estruturados, notas e razões, anti-raid, anti-nuke por audit log, quarentena, restauro de roles, join gate, shadow mode, Safety Health, Permission Passport e audit trail.

Melhoria necessária: uma página “Proteger o servidor” com presets, risco explicado, permissões mínimas, preview da consequência, teste em shadow mode e histórico. O utilizador não deve precisar de conhecer os nomes internos Core, Security ou Insights.

### 2. Tickets

O backend do Vozen já é mais completo do que o fluxo básico do MEE6. A lacuna é um wizard para publicar o painel, escolher equipa/categoria/SLA, pré-visualizar permissões e testar um ticket.

### 3. Levels e Rank Card

XP, leaderboard e rank card existem. O editor do cartão resolve apenas a aparência. Ainda falta transformar Levels num produto completo no painel: regras, exclusões, anti-farm, role rewards, anúncio, privacidade e página pública.

### 4. Giveaways, Polls e Starboard

As três capacidades existem no runtime, mas não parecem produtos acabados para um admin principiante. Cada uma precisa de builder, preview, permissões, rascunho/publicação e resultados.

### 5. Welcome, self-roles, reminders e tags

As bases existem, mas estão fragmentadas em comandos. Devem tornar-se editores por objetivo, com defaults PT-PT, variáveis inseridas por botão e teste antes de publicar.

### 6. Automations

O Vozen tem uma vantagem que o MEE6 não evidencia: dry-run e execução bounded/auditável. Deve preservar essa segurança, mas alargar catálogo de triggers, condições e ações. O editor deve mostrar “Quando → Se → Então” e explicar por que um evento não executou.

### 7. Operações, confiança e privacidade

Quotas, saúde, analytics, export/delete/receipt, import/export de configuração, correlation IDs e isolamento de guild já existem no backend. Isto pode ser uma diferenciação forte, mas o painel não traduz esses sistemas em respostas simples: “está seguro?”, “o que vai mudar?”, “quem fez isto?” e “como volto atrás?”.

## O que falta de verdade

### Prioridade alta

1. Catálogo beginner-friendly ligado ao backend real.
2. Configuradores visuais para capacidades já existentes.
3. Reaction roles mais ricos e escaláveis.
4. Automation Studio completo e auditável.
5. Social Alerts inicial: YouTube, Twitch e RSS.
6. Invite Tracker persistente com anti-abuso.

### Prioridade média

- Achievements integrados com XP e eventos;
- Custom Commands com argumentos/variáveis;
- statistics channels;
- temporary channels;
- birthdays com privacidade;
- página pública de leaderboard com opt-out.

### Adiar até existir procura comprovada

- Economy;
- Monetize;
- Web3/NFT/crypto;
- AI Characters e geração genérica;
- Bot Personalizer;
- Search Anything sem um caso de uso específico.

## Proposta de UX original para o Vozen

Não replicar a sidebar ou a grelha do MEE6 pixel por pixel. Usar a identidade Vozen e um fluxo próprio em três níveis:

### Início

- checklist “Põe o servidor pronto em 10 minutos”;
- objetivos: Receber membros, Proteger, Dar suporte, Criar engagement, Automatizar;
- recomendações baseadas nas permissões e configuração atuais;
- cada passo mostra tempo estimado e resultado.

### Configurar

- catálogo pesquisável com estado: Por configurar, Rascunho, Publicado, Precisa de atenção;
- card explica benefício, permissões, plano e impacto antes de ativar;
- wizard com defaults PT-PT;
- barra fixa: Pré-visualizar, Testar, Publicar;
- Advanced só aparece quando pedido.

### Operar

- overview atual evoluído: saúde, casos, tickets, automações, falhas e mudanças recentes;
- cada aviso tem causa, consequência e botão de correção;
- audit trail e rollback acessíveis.

O princípio diferenciador deve ser: “tão fácil como o MEE6 para começar, muito mais explicável e seguro quando algo corre mal”.

## Direitos de autor, licença e risco de processo

Isto é informação geral, não substitui parecer jurídico para um lançamento comercial.

### A afirmação do owner está essencialmente correta, mas incompleta

[V] Na UE, o direito de autor de software protege a expressão do programa, não as ideias e princípios subjacentes, incluindo os princípios das interfaces. [Diretiva 2009/24/CE, artigo 1.º, n.º 2](https://eur-lex.europa.eu/eli/dir/2009/24/oj)

[V] O TJUE decidiu em SAS Institute v WPL que funcionalidade, linguagem de programação e formatos não são, por si, expressão protegida do programa. Uma implementação independente da mesma função pode ser lícita sem copiar código ou expressão original. [C-406/10](https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX%3A62010CJ0406)

Por isso, níveis, XP, welcome, reaction roles, automações, moderação, alertas, tickets e rank cards podem ser implementados de raiz pelo Vozen.

### O layout não está automaticamente livre

[V] Uma interface gráfica não é protegida como código do programa, mas pode ser protegida como obra autónoma se a composição for uma criação intelectual original. Componentes ditados apenas pela função têm proteção mais limitada. [BSA, C-393/09](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX%3A62009CJ0393)

Logo, copiar o conceito “sidebar + cards + ativar” é muito menos arriscado do que copiar:

- a mesma composição distintiva e proporções;
- ícones, ilustrações, mascote e crown badges;
- textos, descrições, defaults e help articles;
- templates, rank cards, banners e screenshots;
- nomes de marketing e sequência visual quase idêntica.

Uma cópia pixel-perfect pode ainda criar risco de design, marca e concorrência desleal, mesmo quando as features são funcionais.

### O que dizem os termos do MEE6

Os [Termos oficiais do MEE6](https://mee6.xyz/terms.pdf), servidos atualmente mas com última revisão declarada de 20 de fevereiro de 2023:

- dão apenas licença limitada, não exclusiva e para uso próprio;
- reservam software, estrutura, código, website, marcas, ilustrações, imagens e logos;
- proíbem copiar, modificar e criar derivados do conteúdo do site fora das exceções legais;
- proíbem acesso automatizado por robot/spider;
- proíbem copiar software/source, reverse engineering, decompilação e tentativa de descobrir source code;
- proíbem reproduzir ou explorar partes do site/serviço sem autorização.

Os termos não conseguem transformar a ideia de “sistema de níveis” em copyright. Porém, uma violação contratual pode ser uma causa separada e justificar suspensão ou litígio.

### Marca, dados e scraping

- Não usar MEE6 no nome, domínio, bot, avatar, logo ou identidade visual.
- Uma comparação factual “Vozen vs MEE6” é menos arriscada se for verdadeira, proporcional e disser claramente que não existe afiliação.
- Não fazer scraping do dashboard, leaderboards ou APIs não documentadas.
- Não copiar ou espelhar dados MEE6. Bases de dados podem ter proteção própria. [Diretiva 96/9/CE](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX%3A31996L0009)
- Uma futura migração de XP MEE6 deve aceitar apenas export oficial ou ficheiro fornecido por admin autorizado, com revisão de termos, autoridade, GDPR e proveniência.
- Uma pesquisa formal de marcas EUIPO/INPI e de patentes não foi feita; antes de marketing comparativo amplo ou migração MEE6, obter revisão jurídica.

### Regra clean-room recomendada

Para cada feature:

1. guardar apenas o facto funcional público;
2. escrever requisitos em linguagem Vozen;
3. desenhar fluxo, texto e aparência originais;
4. implementar de raiz, sem código ou endpoints MEE6;
5. usar apenas dados obtidos pelo bot/API Discord do Vozen;
6. manter proveniência de ícones, imagens, fontes, templates e banners;
7. fazer uma revisão visual final para garantir impressão global diferente.

| Conduta | Risco indicativo |
|---|---:|
| Implementar de raiz a mesma feature genérica | Baixo |
| Observar manualmente comportamento autorizado e escrever requisitos próprios | Baixo–médio |
| UI original inspirada apenas nos princípios de clareza | Baixo–médio |
| Fluxo, taxonomia, wording e output muito próximos | Médio |
| Importar dados MEE6 fornecidos por clientes sem validação | Médio–alto |
| Copiar código, textos, imagens, ícones, templates ou screenshots | Alto |
| Scraping, APIs privadas, decompilação ou contornar controlos | Alto |
| Branding que pareça oficial ou afiliado ao MEE6 | Alto |

## Fontes principais

### MEE6

- [Getting Started](https://help.mee6.xyz/support/solutions/articles/101000385394-getting-started-with-mee6)
- [Matriz de permissões e plugins](https://help.mee6.xyz/support/solutions/articles/101000484903-what-permissions-does-mee6-need-)
- [Defaults](https://help.mee6.xyz/support/solutions/articles/101000529703-default-settings-for-mee6-plugins)
- [Automations](https://help.mee6.xyz/support/solutions/articles/101000546996-getting-started-with-mee6-automations)
- [Reaction Roles](https://help.mee6.xyz/support/solutions/articles/101000473019-how-to-use-reaction-roles-as-a-verification-gate-for-your-community)
- [Invite Tracker](https://help.mee6.xyz/support/solutions/articles/101000549514-mee6-invite-tracker-plugin-for-discord)
- [Giveaways](https://help.mee6.xyz/support/solutions/articles/101000446107-mee6-giveaways-plugin-for-discord)
- [Economy](https://help.mee6.xyz/support/solutions/articles/101000536968-mee6-economy-plugin-features-and-limitations)
- [Social Alerts](https://help.mee6.xyz/support/solutions/articles/101000490834-mee6-social-alerts-for-discord-limits-and-restrictions)
- [Web3 Statistics](https://help.mee6.xyz/support/solutions/articles/101000433948-how-to-use-web3-statistic-channels)
- [Web3 Gating](https://help.mee6.xyz/support/solutions/articles/101000462453-how-to-gating)
- [Bot Personalizer](https://help.mee6.xyz/support/solutions/articles/101000442591-how-to-set-up-mee6-custom-bot)
- [Termos de utilização](https://mee6.xyz/terms.pdf)

### Jurídicas

- [Diretiva 2009/24/CE — software](https://eur-lex.europa.eu/eli/dir/2009/24/oj)
- [SAS Institute v WPL, C-406/10](https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX%3A62010CJ0406)
- [BSA, C-393/09 — interfaces gráficas](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX%3A62009CJ0393)
- [Decreto-Lei n.º 252/94 — programas de computador em Portugal](https://diariodarepublica.pt/dr/detalhe/decreto-lei/252-1994-625995)
- [Regulamento de Marca da UE 2017/1001](https://eur-lex.europa.eu/eli/reg/2017/1001)
- [Código da Propriedade Industrial português](https://diariodarepublica.pt/dr/legislacao-consolidada/decreto-lei/2018-117279941)
- [Diretiva 96/9/CE — bases de dados](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX%3A31996L0009)

## Conclusão

O melhor movimento não é construir todas as features que aparecem nas capturas. É primeiro fazer o produto existente parecer tão completo quanto realmente é. O Vozen deve ganhar a primeira utilização com clareza e ganhar a operação diária com segurança, auditabilidade e controlo — áreas onde já tem bases que o MEE6 não comunica com a mesma profundidade.

Nenhuma recomendação deste documento foi adicionada ao backlog. Depende de aprovação explícita do owner.
