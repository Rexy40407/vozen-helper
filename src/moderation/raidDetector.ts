import type { RaidConfig } from '../config.js';

// Deteção de raid por TAXA de joins: se entrarem >= N contas numa janela de M
// segundos, é raid. Janela deslizante em memória. Puro (now injetável).
//
// Limitação (ver docs/RESEARCH): um bot single-server não tem o sinal cross-server do
// Beemo; compensa-se com join gate agressivo + este detetor + verificação.

export class RaidDetector {
  private joins: number[] = [];
  private raidUntil = 0;

  constructor(private readonly cfg: RaidConfig) {}

  /**
   * Regista um join e diz se ISTO desencadeia (ou mantém) o modo raid.
   * `justTriggered` é true só na transição para raid (para agir uma vez).
   */
  record(now: number): { isRaid: boolean; justTriggered: boolean; count: number } {
    const windowStart = now - this.cfg.joinWindowMs;
    this.joins = this.joins.filter((t) => t >= windowStart);
    this.joins.push(now);

    const count = this.joins.length;
    const alreadyRaiding = now < this.raidUntil;
    const over = count >= this.cfg.joinThreshold;

    let justTriggered = false;
    if (over && !alreadyRaiding) {
      justTriggered = true;
      this.raidUntil = now + this.cfg.raidModeMs;
    } else if (over) {
      // Prolonga o modo raid enquanto continuarem a chegar.
      this.raidUntil = now + this.cfg.raidModeMs;
    }

    return { isRaid: over || alreadyRaiding, justTriggered, count };
  }

  isRaiding(now: number): boolean {
    return now < this.raidUntil;
  }
}
