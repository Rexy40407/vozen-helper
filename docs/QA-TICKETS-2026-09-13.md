# QA manual: Tickets, 2026-09-13

## Cenário: abrir ticket

- Estado: **falhou**, segundo o teste real do utilizador.
- Alvo previsto na checklist: servidor `adawdawdad`. O log não contém guild ou utilizador, pelo que não confirma essas identidades isoladamente.
- Identidade/cargo do participante: não confirmado nesta tentativa.
- Painel na captura: mensagem de 18/07/2026, botão `Open ticket`.
- Esperado: criar canal privado e responder com o respetivo link.
- Observado: resposta privada `Unable to complete this action.`. A captura não permite confirmar se chegou a ser criado um canal antes do erro.

## Evidência inicial consultada (antes do deploy, apenas leitura)

- Serviço `vozen-helper.service`: `active`.
- Release da VPS: `87bb1885f93568aada2c12df259716cfc09f0301`.
- Correções no GitHub/local: `fc8d48df29618fb3a6fa6ef955df3521abc7e7e2`; ainda não instaladas na VPS consultada.
- Journal, 2026-09-13 18:28:36 e 18:28:58 UTC: `component failed`, `Missing Permissions`. Horário compatível com a captura (19:28 em Lisboa).
- O botão legado e o runtime Rust usam `ticket:open`; a idade do painel, por si só, não demonstra incompatibilidade.
- A release instalada tenta atribuir `MENTION_EVERYONE` ao próprio bot ao criar o canal. A correção local remove essa atribuição. Isto é compatível com a falha observada, mas o log genérico não identifica a chamada Discord exata nem todas as permissões efetivas.

## Próximo passo registado antes da autorização

Solicitar autorização para instalar a release corrigida e repetir a abertura no servidor de teste. Se continuar a falhar, verificar a configuração de categoria/cargo e as permissões efetivas na operação que falhou. Não conceder Administrador ou permissões globais como solução genérica.

Nenhum deploy, reinício, mudança de permissões ou criação de mensagens foi efetuado nesta investigação. Claim, fecho e reabertura continuam pendentes. TikTok fica para último, conforme pedido do utilizador.

## Atualização após deploy autorizado — 18:40 UTC

- O utilizador autorizou o deploy e introduziu a autenticação sudo diretamente no PowerShell aberto para a ativação.
- Release fc8d48df29618fb3a6fa6ef955df3521abc7e7e2 ativa desde 18:40:11 UTC, confirmada pelo symlink, executável do PID 1487026 e SHA256 correspondente ao artefacto validado.
- Serviço active/running, NRestarts=0; health ok; /api/me sem sessão devolve 401.
- Gateway Discord ready às 18:40:13 UTC. Sem novos ERROR/WARN/panic no journal acessível desde a ativação até à verificação.
- Backup consistente e release anterior preservados. Nenhuma permissão Discord foi alterada nesta ativação.
- **Próximo teste:** o utilizador volta a clicar em Open ticket no servidor de teste. O cenário ainda não está aprovado; o sucesso do deploy não prova que a falha foi resolvida no Discord. Só depois avançar para menção do cargo, claim e fecho/reabertura.

## Reteste do utilizador e melhoria de notificação da staff

- **Abertura: passou**, segundo o utilizador e a captura com a mensagem inicial dentro do ticket.
- **Claim: passou nesta tentativa**, captura mostra `Ticket claimed by @Rexy`. Não prova a matriz de permissões para cada cargo; cargo e permissões efetivos do ator não foram confirmados.
- Pedido novo: escolher no site o cargo a mencionar e tornar a mensagem inicial explícita sobre a chamada da equipa.
- Implementação local, ainda não publicada: aproveita `staffRole` existente (mesma configuração de acesso/claim e ping), com label `Staff role to notify` / `Cargo da staff a notificar`, ajuda explicativa, opção vazia explícita e exclusão de @everyone no seletor.
- Mensagem com staff: `Hello <@user>! I've called <@&role> to help you.\nTell us what you need and the support team will reply here.` Sem cargo não afirma ter chamado a equipa.
- Allowed mentions limitado ao autor e ao cargo configurado, sem parse global de utilizadores/cargos/everyone; sem alterações automáticas a permissões Discord.
- Preflight avisa quando não há cargo ou quando o cargo não é mentionable e o bot não tem permissão de menção; bloqueia @everyone como cargo de staff. Exige contexto Discord da guild autorizada antes dessas verificações.
- Fonte do contrato: https://docs.discord.com/developers/resources/message — role mention requer mentionable ou MENTION_EVERYONE; preferências individuais podem suprimir push/som. Não é possível garantir entrega de notificações a todos os membros.
- TDD: novos testes inicialmente falharam por helpers ainda inexistentes; depois implementação e checks verdes. Total 258 testes Rust passou nos executáveis relevantes (58 API + 1 integração + 70 Core + 45 Discord + 26 Modules + 58 Store). A primeira execução de Store foi bloqueada pela política Windows 4551; uma repetição normal passou, sem alterar proteções. Clippy workspace/all-targets -D warnings e fmt check passaram.
- Painel: 27 testes Vitest passaram; TypeScript/Vite build e ui:check passaram. Build manteve avisos de fontes públicas resolvidas em runtime e chunk acima de 500 KB; não alterados neste âmbito. Percentagem de cobertura não medida.
- Navegador local (VITE_HELPER_LOCAL_PREVIEW=true, guild fictícia Demo server): label/ajuda/placeholder em EN/PT confirmados; escolha @Member refletida no controlo e confirmação de guardar em preview. O modo demo apenas guarda em memória e reinicia valores no reload; não constitui prova de persistência na API de produção. Nenhuma mensagem externa enviada neste teste.
- Alterações anteriores do repositório do site foram preservadas. Sem commit, push ou deploy desta melhoria. Ping real após publicação, fecho e reabertura continuam pendentes; TikTok continua para último.
