# Helper Rust: duracoes, enquetes, privacidade e quarentena

## Estado e ambito

Revisao local sobre a base `9a918aa8702999bbfd5ad8ac2fbc258f51571d11`,
branch `migration/helper-rust-public-secure`. Foram preservados os outros
checkouts. Esta passagem nao certifica todos os modulos nem comportamento
ponta a ponta no Discord.

O utilizador pediu GPT-6.1-Sol. O modelo consta do catalogo local, mas a
invocacao isolada falhou antes de executar: `Could not find home directory`.
As correcoes abaixo sao da inspecao local, nao de uma revisao concluida pelo Sol.

## Problemas e correcoes locais

- **Crash em duracoes Unicode:** `parse_duration("é")` reproduziu um panic
  por corte dentro de um caracter UTF-8. O parser usa agora corte verificado
  e rejeita sufixos invalidos, valores negativos, overflow e valores fora do limite.
- **Duracao de enquete alterada silenciosamente:** o teste de uma duracao
  explicita de 10 segundos falhou, pois era substituida pelo padrao. O comando
  rejeita texto invalido e o avaliador rejeita valores fora de 1 minuto a 7 dias.
- **Publicacao e fecho inconsistentes de enquetes:** enquete e job de fecho
  sao gravados na mesma transacao. Uma falha de publicacao remove apenas o
  rascunho do servidor correto. Uma falha ao editar os resultados devolve erro
  e conserva o job; a nova tentativa edita a mesma mensagem com votos fechados.
- **Mencoes indesejadas em enquetes:** pergunta e opcoes eram colocadas em
  mensagens sem restricao explicita de mencoes. Publicacao e resultados usam
  `allowed_mentions` vazio, validado por serializacao dos builders reais.
- **Limpeza incompleta do servidor:** o teste demonstrou que a configuracao
  revisionada sobrevivia a `purge_guild`. A operacao inclui agora configuracao,
  historico, inscricoes de feeds, grants, convites, eventos e resultados de
  sorteios. Testes verificam isolamento, idempotencia e preservacao de sessoes
  e entitlements. Nenhuma limpeza de producao foi executada.
- **Resultados de sorteios orfaos:** a retencao remove resultados antes de
  eliminar sorteios antigos e tambem resultados sem sorteio pai. A remocao de
  um rascunho elimina os resultados e entradas ligados. Testes conservam o
  resultado recente e removem um orfao de uma versao anterior.
- **Cargos perdidos no restauro da quarentena:** o handler apagava o snapshot
  depois de falhas de restauro. Agora guarda os cargos pendentes e informa o
  resultado parcial. Cargos invalidos nao chegam ao Discord. Uma quarentena
  repetida nao substitui o snapshot original; falhas de remocao sao reportadas.

## Validacao

- Suite Rust completa numa execucao desta passagem: **300 testes passaram**, zero falhas.
  API: 68; integracao de sessao: 1; core: 79; Discord: 56;
  modules: 26; store: 70.
- `cargo test --workspace --locked --offline`: passou nessa execucao. Depois
  de reforcar as fixtures de sessao e resultados orfaos, a repeticao final
  foi parcialmente bloqueada pelo Windows Application Control, erro 4551,
  antes de executar alguns binarios. Nao equivale a uma falha de assertions;
  as fixtures finais afetadas passaram posteriormente no runner Linux,
  no CI `36651086533` e na release `36651086557` do commit `0b7d6b1`.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passou.
- Formatacao verificada com o executavel `rustfmt` da toolchain 1.97.1,
  edition 2024, nos fontes Rust. O launcher `cargo fmt` foi bloqueado pelo
  Windows Application Control; nao foram alteradas protecoes do sistema.
- `git diff --check`: passou.
- Fixtures: bases SQLite em memoria, falha forçada na gravacao do job,
  restauro de cargos simulado, builders de mensagens serializados e erro
  de resultados sem mensagem publicada. Nao houve canario real no Discord.

## Bloqueios e trabalho restante

- SSH para o VPS foi recusado por este ambiente: `Permission denied` ao
  conectar a porta 22. O pedido de permissao de rede nao concedeu acesso.
  Nao foi possivel verificar o processo atual, criar backup remoto ou fazer
  deploy. O alvo registado no Second Brain em 30 de setembro e
  `ubuntu@146.59.147.110`; o IP Hetzner antigo nao deve ser usado.
  A release Linux do commit `0b7d6b1` foi publicada pelo workflow
  `36651086557`, mas isso nao prova que esteja ativa no VPS.
- **Retencao nao ligada ao runtime:** `start_scheduler` existe em
  `helper-modules`, mas nao tem chamadas no workspace. `Serve` nao inicia
  essa rotina. A funcao tambem confirma jobs apenas depois de os registar em
  logs, sem executar a acao Discord; nao deve ser ligada tal como esta.
  O gateway tem o seu proprio executor de jobs. A solucao futura deve separar
  retencao de entrega, testar o ciclo de vida e confirmar backup/politica antes
  de ativar eliminacao de dados antigos em producao. Nao foi ativada nesta passagem.
- Mantida a semantica existente para sorteios/enquetes quando o modulo e
  desligado; a decisao anterior do utilizador continua pendente.
- A verificacao modulo a modulo nao terminou. Premium continua perto do fim
  e a integracao TikTok fica por ultimo. A limpeza de dados partilhada nao
  constitui uma auditoria dos providers Premium.

## Dependencias e CI

- O CI geral de `0b7d6b1` passou audit, formatacao, testes e Clippy Rust,
  mas parou no audit npm: `undici 6.28.0` tinha um advisory high. Os checks
  Node e panel seguintes ficaram por executar nesse run.
- Override e lockfile elevados a `undici 6.28.1`, a correcao indicada em
  https://github.com/nodejs/undici/security/advisories/GHSA-rfgv-xxqx-mfg5.
  Como o registry esta bloqueado neste ambiente, o manifest foi confirmado
  no tag oficial `v6.28.1` e a integridade no lockfile primario Apache:
  https://github.com/apache/streampipes/blob/ee3f344914f04516cbbfb1755271dcb9965c86fe/ui/package-lock.json.
  A instalacao deste pacote e o audit atual precisam de verificacao pelo CI.
- Vitest e os pacotes associados elevados a `4.1.11`, usando metadata e
  artefactos ja presentes na cache npm, para o advisory moderate
  https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9.
  Vite mantido em `7.3.6`; nao foi adotado Vite 8 incidentalmente.
- Removido `minWorkers`, que deixou de integrar a configuracao Vitest 4,
  e o shebang desnecessario do modulo de audit: os workflows invocam-no
  com `node`, e o shebang causava `SyntaxError` ao importar nos testes.
- Lint, typecheck e build Node passaram. **260 testes em 29 ficheiros
  passaram** com `npm test -- --configLoader native`. O loader native
  evitou um bloqueio de leitura do bundler de config neste ambiente Windows;
  nao foram alteradas protecoes nem excluidos testes.
- Nao foi reduzido o limiar do audit, nem usado `npm audit fix --force`.
  O deploy continua condicionado a CI geral verde e acesso ao VPS.
