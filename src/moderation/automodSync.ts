import {
  AutoModerationRuleTriggerType,
  AutoModerationRuleEventType,
  AutoModerationActionType,
  type Guild,
  type AutoModerationRuleCreateOptions,
} from 'discord.js';
import type { AutomodConfig } from '../config.js';
import { log } from '../log.js';

// Sync DECLARATIVO das regras do AutoMod nativo. A config descreve as regras
// desejadas; nós criamo-las/atualizamo-las via API. O nativo bloqueia ANTES de
// publicar (slurs/keywords/mention spam) — coisa que nenhum bot consegue fazer.
//
// Cada regra do Vozen Helper tem o nome prefixado (VH_) para não colidir com regras
// feitas à mão e para as reconhecermos no diff.

export const RULE_PREFIX = 'VH:';

export interface DesiredRuleDescriptor {
  /** Nome único (com prefixo). */
  name: string;
  build: (guildId: string) => AutoModerationRuleCreateOptions;
}

/** Constrói os descritores das regras desejadas a partir da config. */
export function buildDesiredRules(cfg: AutomodConfig): DesiredRuleDescriptor[] {
  const rules: DesiredRuleDescriptor[] = [];
  const blockAction = {
    type: AutoModerationActionType.BlockMessage as const,
  };

  if (cfg.keywords.length > 0) {
    rules.push({
      name: `${RULE_PREFIX} keywords`,
      build: () => ({
        name: `${RULE_PREFIX} keywords`,
        eventType: AutoModerationRuleEventType.MessageSend,
        triggerType: AutoModerationRuleTriggerType.Keyword,
        triggerMetadata: { keywordFilter: cfg.keywords },
        actions: [blockAction],
        enabled: true,
      }),
    });
  }

  if (cfg.enableSlurPreset || cfg.enableProfanityPreset || cfg.enableSexualPreset) {
    const presets: number[] = [];
    // KeywordPresetType: Profanity=1, SexualContent=2, Slurs=3
    if (cfg.enableProfanityPreset) presets.push(1);
    if (cfg.enableSexualPreset) presets.push(2);
    if (cfg.enableSlurPreset) presets.push(3);
    rules.push({
      name: `${RULE_PREFIX} presets`,
      build: () => ({
        name: `${RULE_PREFIX} presets`,
        eventType: AutoModerationRuleEventType.MessageSend,
        triggerType: AutoModerationRuleTriggerType.KeywordPreset,
        triggerMetadata: { presets },
        actions: [blockAction],
        enabled: true,
      }),
    });
  }

  if (cfg.mentionLimit && cfg.mentionLimit > 0) {
    rules.push({
      name: `${RULE_PREFIX} mention-spam`,
      build: () => ({
        name: `${RULE_PREFIX} mention-spam`,
        eventType: AutoModerationRuleEventType.MessageSend,
        triggerType: AutoModerationRuleTriggerType.MentionSpam,
        triggerMetadata: { mentionTotalLimit: cfg.mentionLimit! },
        actions: [blockAction],
        enabled: true,
      }),
    });
  }

  return rules;
}

/**
 * Decide, a partir dos nomes das regras já existentes, quais criar e quais já lá
 * estão. Puro (a aplicação real fica no `syncAutomod`). Só considera regras nossas
 * (prefixo VH:) para não mexer nas feitas à mão.
 */
export function planSync(
  existingNames: readonly string[],
  desired: readonly DesiredRuleDescriptor[],
): { toCreate: string[]; toUpdate: string[] } {
  const ours = new Set(existingNames.filter((n) => n.startsWith(RULE_PREFIX)));
  const toCreate: string[] = [];
  const toUpdate: string[] = [];
  for (const d of desired) {
    if (ours.has(d.name)) toUpdate.push(d.name);
    else toCreate.push(d.name);
  }
  return { toCreate, toUpdate };
}

/**
 * Aplica o sync no guild: cria as regras em falta e atualiza (recria) as nossas que
 * já existem. Best-effort — regista erros mas não rebenta o arranque.
 */
export async function syncAutomod(guild: Guild, cfg: AutomodConfig): Promise<void> {
  const desired = buildDesiredRules(cfg);
  if (desired.length === 0) return;

  let existing;
  try {
    existing = await guild.autoModerationRules.fetch();
  } catch (err) {
    log.warn('Não consegui ler as regras AutoMod (falta MANAGE_GUILD?):', (err as Error).message);
    return;
  }

  const byName = new Map([...existing.values()].map((r) => [r.name, r]));
  for (const d of desired) {
    try {
      const current = byName.get(d.name);
      const opts = d.build(guild.id);
      if (current) {
        await current.edit({
          triggerMetadata: opts.triggerMetadata,
          actions: opts.actions,
          enabled: true,
        });
      } else {
        await guild.autoModerationRules.create(opts);
      }
    } catch (err) {
      log.error(`Falha a sincronizar a regra AutoMod "${d.name}":`, (err as Error).message);
    }
  }
  log.info(`AutoMod sincronizado (${desired.length} regra(s) geridas pelo bot).`);
}
