# Helper Rust: segunda passagem de auditoria, 2026-09-07

## Estado

Correções locais, sem commit, push ou deploy nesta passagem. Foram preservadas
as alterações da auditoria anterior. O relatório antigo `AUDIT-FINDINGS.md`
refere-se ao runtime TypeScript e não serve como certificação deste runtime Rust.

Foram corrigidos quatro problemas novos e adicionados 11 testes. Na retoma
de 2026-09-07, a **suite completa passou: 257 testes, zero falhas**,
incluindo os 58 testes de `helper-store` anteriormente bloqueados pelo Windows.
O bloqueio deixou de se reproduzir ao executar o mesmo comando de testes;
não foi necessário alterar as proteções do sistema.

Não é uma declaração de "zero bugs", nem uma validação ponta a ponta dos 47
módulos num servidor Discord. Não foram medidas percentagens de cobertura.

## Bugs reproduzidos e correções

| Área | Causa e impacto | Correção |
| --- | --- | --- |
| Sorteios | A leitura do estado aberto e a gravação da participação eram separadas; a consulta de cargos ao Discord podia permitir que o sorteio fechasse entretanto. As operações da base de dados aceitavam entradas e saídas depois do fecho. | Validação de existência, estado e prazo na gravação; toggle numa transação imediata; resposta explícita quando o sorteio já terminou. |
| Votações | A gravação não verificava estado, prazo ou índice da opção. Um pedido atrasado podia alterar os resultados de uma votação fechada. | INSERT/UPDATE condicional e atómico; índices fora das opções ou demasiado grandes são rejeitados; o bot só confirma um voto realmente guardado. |
| Canais ignorados | O preflight verificava permissões de entrega também nos canais excluídos da atuação do módulo. | As exclusões continuam sujeitas à validação de existência no servidor, mas não às permissões de entrega. Os canais de destino e de logs mantêm essas verificações. |
| TikTok OAuth | O gateway exigia o token global, mesmo com OAuth configurado. A API também rejeitava OAuth de produção sem token global quando o sandbox estava desligado. | Regra partilhada pela API e pelo gateway: OAuth funciona com aprovação de produção ou sandbox explícito; token global exige aprovação de produção. Sandbox não pode usar o token global como alternativa a uma conta ligada. |

Principais pontos de implementação:

- `crates/helper-store/src/lib.rs`: `toggle_giveaway_entry`, `vote_poll`, guardas de entrada/saída.
- `crates/helper-discord/src/lib.rs`: respostas de participações/votos, worker TikTok e seleção do cliente por servidor.
- `crates/helper-api/src/lib.rs`: `channel_dependency_issues` e verificações TikTok.
- `crates/helper-modules/src/lib.rs`: `tiktok_runtime_allowed`.

Não foram lidas credenciais reais nem alteradas permissões, contas ou configurações de produção.

## Evidência dos testes

Antes das correções:

- Três testes de alterações após fecho/expiração falharam. A votação fechada
  passou indevidamente de `[1, 0]` para `[0, 2]`.
- O teste de votos expirados/índices inválidos falhou: um voto expirado foi aceite.
- O teste de canal ignorado falhou com uma exigência indevida de `Send Messages`.
- O teste de TikTok OAuth sem token global falhou.

Depois das correções:

- Os sete testes novos da base de dados passaram: guardas de fecho/expiração,
  índices de voto, toggle de participações, concorrência e respostas de gravação.
- O preflight de canais ignorados passou, incluindo controlos que continuam a
  rejeitar destinos sem permissão de envio.
- Os testes TikTok passaram: OAuth de produção e sandbox, bloqueio sem
  aprovação/sandbox, credenciais em falta, isolamento entre servidores,
  desligar uma conta e proibição de fallback global em sandbox.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passou.
- `cargo fmt --all -- --check`: passou.
- `git diff --check`: passou; apenas avisos informativos de LF/CRLF do Git.

Última execução de `cargo test --workspace --locked --offline --quiet`
na retoma de 2026-09-07 (exit code 0):

| Suite | Resultado |
| --- | --- |
| helper-api | 57 passaram |
| Integração da sessão Vozen | 1 passou |
| helper-core | 70 passaram |
| helper-discord | 45 passaram |
| helper-modules | 26 passaram |
| helper-store | 58 passaram |
| Doc-tests finais | Todas as fases concluídas, sem testes definidos |
| Total de testes executados | 257 passaram, zero falhas, zero ignorados |

Bloqueio anterior: `os error 4551`, "Uma política de Controlo de Aplicações
bloqueou este ficheiro", no binário
`target/debug/deps/helper_store-ec3e7a41b538d7da.exe`.
Repetir com a execução autorizada fora do sandbox deu o mesmo resultado.
Os eventos Code Integrity 3077 das 13:43 e 13:44 confirmam o bloqueio anterior.
Na retoma, o executável foi aceite e os 58 testes passaram. Não foi determinada
a causa da mudança de avaliação do Windows. Não foram desligadas nem
contornadas proteções do sistema.

## Alcance da passagem pelos 47 módulos

Os testes do catálogo percorrem as 47 chaves, schemas, validações, projeções,
simulações limitadas, valores nulos, Unicode e limites numéricos. Os testes da
API verificam os detalhes do catálogo e isolamento entre servidores. Há também
guardas de código-fonte para referências e consumidores no gateway; essas
guardas **não** substituem testes reais de efeitos no Discord.

| Grupo | Módulos incluídos nas verificações do catálogo |
| --- | --- |
| Proteção (4) | antispam, antiscam, anti_raid, join_gate |
| Comunidade (10) | levels, leaderboard, starboard, suggestions, giveaways, role_panels, events, achievements, birthdays, economy |
| Suporte (3) | tickets, welcome, welcome_channel |
| Gestão (9) | nickname, workflows, polls, moderation, custom_commands, audit, privacy, templates, invite_tracker |
| Insights / Studio (2) | stats, rank_card |
| Utilitários (6) | help, reminders, emojis, embeds, search, temp_channels |
| Social (8) | twitch, youtube, instagram, tiktok, rss, podcasts, kick, bluesky |
| Crescimento (1) | monetization |
| Web3 (4) | crypto_stats, crypto_queries, gas_tracker, gating |

Continuam a passar os testes já existentes de permissões e menção do cargo de
suporte nos tickets, duração de timeout e respeito pelos interruptores dos
módulos. As correções anteriores de claim estão preservadas; os respetivos
testes da suite Store também passaram na execução final, incluindo concorrência.

## Antes de publicar

1. Validação automatizada concluída: 257 testes passaram. Se houver novas
   alterações de código, voltar a executar os testes antes de publicar.
2. Rever e publicar as alterações acumuladas do runtime. Nada foi enviado nesta passagem.
3. No servidor de testes, verificar tickets com o cargo de suporte, votação e
   sorteio a terminar com cliques em simultâneo, e gravação de um canal ignorado
   ao qual o bot não tem acesso.
4. Com uma conta TikTok autorizada, verificar OAuth, descoberta de um novo vídeo
   e entrega no Discord. Testes locais não equivalem a aprovação da aplicação
   pelo TikTok nem a entrega real.
5. Completar a checklist manual dos restantes módulos; anti-raid exige as
   entradas controladas que estavam pendentes na checklist do utilizador.
