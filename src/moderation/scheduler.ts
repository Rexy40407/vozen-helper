import type Database from 'better-sqlite3';
import type { Client } from 'discord.js';
import { getDueActions, deleteScheduled, type ScheduledAction } from '../store/cases.js';
import { log } from '../log.js';

// Scheduler persistente de expirações (tempban→unban, timeout→untimeout, mute→unmute).
// As ações vivem na DB (sobrevivem a restart). Ao arrancar varremos as vencidas e,
// depois, um tick periódico trata das que forem vencendo.

export type ActionRunner = (client: Client, action: ScheduledAction) => Promise<void>;

export class ExpiryScheduler {
  private timer: NodeJS.Timeout | null = null;
  private running = false;

  constructor(
    private readonly db: Database.Database,
    private readonly client: Client,
    private readonly runner: ActionRunner,
    private readonly tickMs = 30_000,
  ) {}

  /** Corre uma passagem: executa todas as ações vencidas (execute_at <= now). */
  async runDue(now: number): Promise<void> {
    // Guard de reentrância: se uma passagem demorar mais que o tick, a seguinte (ou o
    // scan inicial em paralelo) NÃO reprocessa as mesmas linhas ainda-não-apagadas —
    // evita lembretes/bump enviados em duplicado.
    if (this.running) return;
    this.running = true;
    try {
      const due = getDueActions(this.db, now);
      for (const action of due) {
        try {
          await this.runner(this.client, action);
        } catch (err) {
          log.error(`Falha na ação agendada #${action.id} (${action.type}):`, err);
        } finally {
          // Apagar mesmo em erro: uma expiração falhada (ex.: já não está banido) não
          // deve repetir-se em loop a cada tick.
          deleteScheduled(this.db, action.id);
        }
      }
    } finally {
      this.running = false;
    }
  }

  /** Arranca a varredura inicial + o tick periódico. */
  start(nowProvider: () => number = Date.now): void {
    void this.runDue(nowProvider());
    this.timer = setInterval(() => void this.runDue(nowProvider()), this.tickMs);
    // Não segurar o event loop só por causa do timer.
    this.timer.unref?.();
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }
}
