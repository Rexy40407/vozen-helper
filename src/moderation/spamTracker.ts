import {
  capsRatio,
  countEmojis,
  countNewlines,
  countLinks,
  hasInvite,
  contentKey,
} from './spamMetrics.js';
import type { SpamConfig } from '../config.js';

// Sistema de "heat" adaptativo (inspirado no Wick, ver docs/RESEARCH). Em vez de
// regras isoladas, cada sinal de spam SOMA calor ao utilizador; o calor DECAI com o
// tempo; ao passar o limite, pune-se. Apanha spam "distribuído" (um pouco de cada) e
// reduz falsos positivos em conversa normal.
//
// Estado em memória (perde-se em restart — aceitável: spam é um fenómeno de segundos).

export interface SpamEvent {
  userId: string;
  channelId: string;
  content: string;
  mentionCount: number;
}

export interface SpamVerdict {
  /** Calor acumulado após esta mensagem. */
  heat: number;
  /** Sinais que contribuíram (para o log/motivo). */
  signals: string[];
  /** true se o calor passou o limite → punir (e o calor é reposto). */
  punish: boolean;
}

interface UserState {
  heat: number;
  lastUpdate: number;
  recent: Array<{ at: number; key: string; channelId: string }>;
}

export class SpamTracker {
  private readonly users = new Map<string, UserState>();

  constructor(private readonly cfg: SpamConfig) {}

  /** Regista uma mensagem e devolve o veredito. `now` injetável para testes. */
  record(ev: SpamEvent, now: number): SpamVerdict {
    const state = this.users.get(ev.userId) ?? { heat: 0, lastUpdate: now, recent: [] };

    // Decaimento do calor desde a última atualização.
    const elapsedSec = Math.max(0, (now - state.lastUpdate) / 1000);
    state.heat = Math.max(0, state.heat - elapsedSec * this.cfg.decayPerSec);

    // Podar a janela deslizante.
    const windowStart = now - this.cfg.windowMs;
    state.recent = state.recent.filter((r) => r.at >= windowStart);

    const key = contentKey(ev.content);
    const signals: string[] = [];
    let heat = 0;

    // Sinais de janela: flood e duplicados/multi-canal.
    const inWindow = state.recent.length;
    if (inWindow + 1 > this.cfg.floodCount) {
      heat += this.cfg.floodHeat;
      signals.push('flood');
    }
    const sameContent = state.recent.filter((r) => r.key === key);
    if (sameContent.length > 0) {
      heat += this.cfg.duplicateHeat;
      signals.push('duplicate');
      const channels = new Set(sameContent.map((r) => r.channelId));
      channels.add(ev.channelId);
      if (channels.size >= this.cfg.multiChannelThreshold) {
        heat += this.cfg.multiChannelHeat;
        signals.push('multi-channel');
      }
    }

    // Sinais por-mensagem.
    if (ev.mentionCount > this.cfg.mentionLimit) {
      heat += this.cfg.mentionHeat * (ev.mentionCount - this.cfg.mentionLimit);
      signals.push('mentions');
    }
    if (
      ev.content.length >= this.cfg.capsMinLength &&
      capsRatio(ev.content) >= this.cfg.capsRatio
    ) {
      heat += this.cfg.capsHeat;
      signals.push('caps');
    }
    if (countEmojis(ev.content) > this.cfg.emojiLimit) {
      heat += this.cfg.emojiHeat;
      signals.push('emojis');
    }
    if (countNewlines(ev.content) > this.cfg.newlineLimit) {
      heat += this.cfg.newlineHeat;
      signals.push('newlines');
    }
    if (countLinks(ev.content) > this.cfg.linkLimit) {
      heat += this.cfg.linkHeat;
      signals.push('links');
    }
    if (hasInvite(ev.content)) {
      heat += this.cfg.inviteHeat;
      signals.push('invite');
    }

    state.heat += heat;
    state.lastUpdate = now;
    state.recent.push({ at: now, key, channelId: ev.channelId });

    const punish = state.heat >= this.cfg.punishAt;
    if (punish) {
      state.heat = 0;
      state.recent = [];
    }
    this.users.set(ev.userId, state);

    return { heat: state.heat, signals, punish };
  }

  /** Esquece um utilizador (ex.: saiu do servidor). */
  forget(userId: string): void {
    this.users.delete(userId);
  }
}
