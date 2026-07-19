# Política aplicada pelo Vozen Helper

Esta matriz é a fonte operacional que liga as regras públicas do **Vozen Support**
ao comportamento do bot. O comando `/violation` aplica a consequência correta
quando a infração precisa de confirmação humana.

| Regra                 | Deteção automática                                                                          | Consequência                                                                                                     |
| --------------------- | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| NSFW / NSFL           | Preset sexual do AutoMod para texto; filtro explícito máximo do Discord para media          | Ban imediato quando o AutoMod identifica texto; media bloqueada é revista pela staff e aplicada com `/violation` |
| Discord ToS           | Anti-raid, anti-scam e anti-nuke cobrem sinais mecânicos; contexto é revisto pela staff     | Ban imediato com `/violation`                                                                                    |
| Doxxing               | Não se bane por regex: um email/telefone isolado não prova que é informação de outra pessoa | Remover o conteúdo e ban imediato com `/violation` após confirmação                                              |
| Hate speech           | Preset de slurs do AutoMod                                                                  | Bloqueio antes da publicação + ban imediato                                                                      |
| Spam                  | Heat por flood, duplicados, canais, menções, caps, emojis, linhas e links                   | 5 min; reincidências em 30 dias sobem para 30 min, 3 h, 18 h e máximo 1 dia                                      |
| Desrespeito / assédio | Requer contexto humano                                                                      | Warn/strike com `/violation`; 3 strikes = 1 h, 5 = 1 dia, 7 = ban                                                |
| Desrespeito à staff   | Requer contexto humano                                                                      | Strike com `/violation`                                                                                          |
| Publicidade           | AutoMod bloqueia convites e links sociais/YouTube configurados                              | Primeira infração = timeout de 1 dia; segunda = ban                                                              |
| Linguagem             | Um uso casual isolado é ignorado; abuso dirigido, excessivo ou repetido é detetado          | Mensagem removida + warn/strike                                                                                  |
| Canal errado          | Requer contexto humano                                                                      | Warn/strike com `/violation`                                                                                     |

## Limites reais da plataforma

- Bots não conseguem ler DMs entre membros; anúncios por DM têm de ser reportados.
- Doxxing, assédio, desrespeito e grande parte das violações dos ToS dependem de
  contexto. Automatizar bans nestes casos criaria falsos positivos graves.
- O AutoMod isenta automaticamente bots, webhooks e membros com `Administrator` ou
  `Manage Server`. A conduta da staff continua sujeita às regras e é tratada pelos
  comandos e guidelines internos.
