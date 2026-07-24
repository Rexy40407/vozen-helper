# Vozen Helper

Bot de moderação de Discord **privado** e **self-hosted**, desenhado para correr em
**um único servidor** (o servidor do Diogo). Irmão do Vozen (bot TTS), mas projeto
independente.

> ⚠️ **Proprietário / não open source.** Repositório privado, sem licença OSS
> (`"license": "UNLICENSED"`). Não distribuir nem publicar o código.

## O que faz (visão)

Cobertura completa de moderação para um servidor: ações de mod com casos e
escalação, anti-spam, filtros de conteúdo (delegando ao AutoMod nativo o bloqueio
pré-publicação de slurs/keywords), anti-raid com join gate, anti-nuke via audit
log, anti-scam, sticky roles e logging total. Tudo config-as-code, sem dashboard.

O plano de desenvolvimento completo está em [`docs/PLAN.md`](docs/PLAN.md); a
research que o fundamenta em [`docs/RESEARCH-FEATURES-MODERACAO.md`](docs/RESEARCH-FEATURES-MODERACAO.md)
e [`docs/RESEARCH-DISCORD-API.md`](docs/RESEARCH-DISCORD-API.md).

A correspondência entre as regras públicas, deteção e punições está em
[`docs/MODERATION-POLICY.md`](docs/MODERATION-POLICY.md).

## Stack

TypeScript (NodeNext, strict) · discord.js v14 · better-sqlite3 (WAL) · vitest.
Mesmas convenções do Vozen: TDD obrigatório, comentários de código em português.

## Arranque (dev)

```sh
npm install
cp .env.example .env   # preencher DISCORD_TOKEN, CLIENT_ID, GUILD_ID
npm run register        # regista os slash commands no guild
npm run dev             # arranca em watch mode
```

Antes de commitar: `npm run build` + `npm run typecheck` + `npm test` verdes.

## Segurança / privilégios

- Liga os **privileged intents** (`Server Members`, `Message Content`) no Developer
  Portal → Bot. Para um bot em <100 servidores não é preciso review.
- Dá ao bot um **role acima de todos os moderadores** (mas abaixo do owner): o
  anti-nuke silencia contas comprometidas removendo-lhes os roles, e o timeout não
  funciona em admins.

## Runtime público Rust

O runtime público está a ser migrado para Rust (Serenity + Axum + SQLite WAL),
mantendo o Node como rollback até os gates de memória, segurança, paridade e
soak serem aprovados. A instalação pública é multi-guild e separa os módulos
Core, Studio, Security, Support, Events, Community, Automate e Insights por
guild.

O painel local usa OAuth Discord com PKCE, estado de OAuth de uso único e
sessões HttpOnly. O Studio expõe Brand Kit e templates versionados por guild;
templates e painéis são limitados por quotas. O plano Plus (€1,99) é pessoal;
Premium é de servidor (€3,99 para 3 guilds ou €7,99 para 8 guilds), resolvido
através do serviço central de entitlements quando configurado.

Para staging, publicar primeiro o artefacto Linux e manter o symlink `current`
no release anterior. Nunca executar dois gateways com o mesmo token; o
procedimento de rollback está em [`deploy/ROLLBACK.md`](deploy/ROLLBACK.md).
