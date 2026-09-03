// Deteção de nuke: conta ações destrutivas por EXECUTOR numa janela deslizante. Se um
// executor faz N ações do mesmo tipo (apagar canais, apagar cargos, banir, expulsar,
// criar webhooks) em poucos segundos, é anómalo. Puro (now injetável).
//
// Chave = `${executorId}:${actionKind}`. Estado em memória (nukes são de segundos).

export type NukeActionKind = 'channelDelete' | 'roleDelete' | 'ban' | 'kick' | 'webhookCreate';

export class NukeTracker {
  private readonly hits = new Map<string, number[]>();

  constructor(private readonly windowMs: number) {}

  /** Regista uma ação e devolve a contagem do executor para esse tipo na janela. */
  record(executorId: string, kind: NukeActionKind, now: number): number {
    const key = `${executorId}:${kind}`;
    const arr = (this.hits.get(key) ?? []).filter((t) => t >= now - this.windowMs);
    arr.push(now);
    this.hits.set(key, arr);
    return arr.length;
  }

  /** Limpa o registo de um executor (após quarentena, para não re-disparar). */
  reset(executorId: string): void {
    for (const key of [...this.hits.keys()]) {
      if (key.startsWith(`${executorId}:`)) this.hits.delete(key);
    }
  }
}
