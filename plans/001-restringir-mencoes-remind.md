# Plan 001: `/remind` deixa de conseguir disparar menções em massa

> **Instruções do executor**: segue passo a passo. Corre cada comando de verificação
> e confirma o resultado esperado antes de avançar. Se ocorrer uma condição de STOP,
> para e reporta. No fim, atualiza a linha deste plano em `plans/README.md`.
>
> **Drift check**: não há git. Antes de editar, abre os ficheiros do "Estado atual" e
> confirma que os excertos batem com o código vivo; se não baterem, é STOP.

## Status

- **Prioridade**: P1
- **Esforço**: S
- **Risco**: LOW
- **Depende de**: nenhum
- **Categoria**: security
- **Planned at**: N/A (não é repo git), 2026-07-14

## Porque é que isto importa

O comando `/remind` é **público** (usável por qualquer membro em qualquer canal). O
texto do lembrete é guardado sem sanitização e, quando o lembrete dispara, é enviado
pelo bot com `channel.send(string)` — **sem** objeto `allowedMentions`. Sem esse
objeto, a API do Discord faz parse de todas as menções do conteúdo. Resultado: um
membro sem privilégios agenda uma mensagem, atribuída ao bot e com atraso arbitrário,
contendo menções a utilizadores/cargos mencionáveis (funcionam sempre) ou `@everyone`/
`@here` (se o bot tiver _Mention Everyone_). É o único vetor não-privilegiado desta
classe no projeto — o resto usa comandos com permissões.

## Estado atual

- `src/community/reminders.ts` — comando `/remind` (`public: true`), guarda `text` no
  payload da ação agendada:
  ```ts
  // src/community/reminders.ts (dentro de execute)
  const text = interaction.options.getString('texto', true);
  scheduleAction(ctx.db, {
    guildId: interaction.guildId,
    type: 'reminder',
    targetId: interaction.user.id,
    executeAt: Date.now() + ms,
    payload: JSON.stringify({ channelId: interaction.channelId, text }),
    caseId: null,
  });
  ```
- `src/community/scheduler.ts` — o caso `reminder` monta e envia a mensagem SEM
  `allowedMentions`:
  ```ts
  // src/community/scheduler.ts, case 'reminder':
  const { channelId, text } = safeJson(action.payload);
  const channel = channelId ? await ctx.client.channels.fetch(channelId).catch(() => null) : null;
  const msg = `⏰ <@${action.targetId}>, lembrete: ${text}`;
  if (channel && channel.isTextBased() && !channel.isDMBased()) {
    await channel.send(msg).catch(() => sendDm(ctx, action.targetId, msg));
  } else {
    await sendDm(ctx, action.targetId, msg);
  }
  ```
  E o `sendDm` no mesmo ficheiro:
  ```ts
  async function sendDm(ctx: AppContext, userId: string, msg: string): Promise<void> {
    const user = await ctx.client.users.fetch(userId).catch(() => null);
    await user?.send(msg).catch(() => undefined);
  }
  ```
- Convenção do repo confirmada em `src/bot/client.ts`: o `new Client({...})` **não**
  define um `allowedMentions` global, por isso a proteção tem de ser por-envio.
  O padrão já usado noutro sítio: `src/community/afk.ts` passa
  `allowedMentions: { repliedUser: false }` nos replies.

## Comandos que vais precisar

| Objetivo  | Comando             | Esperado          |
| --------- | ------------------- | ----------------- |
| Typecheck | `npm run typecheck` | exit 0, sem erros |
| Build     | `npm run build`     | exit 0            |
| Testes    | `npx vitest run`    | todos passam      |

(Correr a partir de `C:\Users\diogo\Videos\second brain\bots-discord\Vozen-helper`.)

## Scope

**In scope:**

- `src/community/scheduler.ts` (único ficheiro a alterar)

**Out of scope (NÃO tocar):**

- `src/community/reminders.ts` — o comando em si está correto; a correção é no envio.
- `src/community/tags.ts` — as tags exigem `ManageMessages` (staff), por isso NÃO é a
  mesma severidade; não alterar neste plano.

## Passos

### Passo 1: enviar o lembrete só com a menção do próprio destinatário

No `src/community/scheduler.ts`, no `case 'reminder'`, substitui o envio por texto
simples por um envio com `allowedMentions` que só permite mencionar o `targetId` (o
autor do lembrete). O `<@target>` no início continua a funcionar; qualquer `@everyone`/
menção de cargo/outro utilizador dentro de `text` deixa de notificar.

Forma alvo:

```ts
const opts = { content: msg, allowedMentions: { users: [action.targetId] } as const };
if (channel && channel.isTextBased() && !channel.isDMBased()) {
  await channel.send(opts).catch(() => sendDm(ctx, action.targetId, msg));
} else {
  await sendDm(ctx, action.targetId, msg);
}
```

O `sendDm` é para o próprio utilizador (DM), onde menções não notificam terceiros —
pode ficar como está, mas por consistência podes passar-lhe também `allowedMentions:
{ parse: [] }` (opcional, defesa em profundidade).

**Verify**: `npm run typecheck` → exit 0.

### Passo 2: teste de regressão da construção da opção de envio

O envio real depende do discord.js (I/O), por isso o teste incide na **construção**
do `allowedMentions`. Extrai (ou expõe) uma pequena função pura que, dado o `targetId`
e o `text`, devolve o objeto de opções, e testa que `allowedMentions.users` contém
apenas o `targetId` e que `parse` não está presente (logo `@everyone`/cargos não são
parseados). Alternativa aceitável se não quiseres refatorar: um teste que documente e
fixe o formato esperado do objeto.

Modela o teste por `tests/community.test.ts` (mesmo estilo, `describe`/`it`).

**Verify**: `npx vitest run` → todos passam, incluindo o novo teste.

## Done criteria

- [ ] `npm run typecheck` exit 0
- [ ] `npm run build` exit 0
- [ ] `npx vitest run` — todos passam, com ≥1 teste novo do formato de `allowedMentions`
- [ ] `grep -n "channel.send(msg)" src/community/scheduler.ts` → sem correspondência no `case 'reminder'` (já não envia string crua)
- [ ] Linha 001 de `plans/README.md` atualizada

## STOP conditions

- O `case 'reminder'` no código vivo não corresponde ao excerto de "Estado atual"
  (o scheduler foi reescrito) → STOP e reporta.
- Se descobrires que o `Client` passou a ter um `allowedMentions` global em
  `src/bot/client.ts` (que já mitigaria isto) → STOP e reporta (a correção pode ser
  redundante).

## Notas de manutenção

- Qualquer feature futura que envie mensagens com conteúdo de utilizador (novas tags,
  respostas automáticas) deve seguir o mesmo padrão `allowedMentions`.
- Alternativa mais robusta a longo prazo: definir um `allowedMentions` global no
  `Client` (`src/bot/client.ts`) como default seguro e só alargar onde for preciso —
  fica como follow-up, fora deste plano.
