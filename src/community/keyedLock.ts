// Lock de exclusão mútua por chave (in-process). Serializa secções críticas
// read-modify-write disparadas fire-and-forget por eventos do Discord (ex.: várias
// reações à mesma mensagem, ou dois cliques no mesmo botão), onde um `await` no meio
// deixaria duas execuções lerem o mesmo estado "vazio" e ambas criarem um registo.
// `tryAcquire` devolve false se a chave já está em uso; liberta-se sempre em `finally`.

export interface KeyedLock {
  tryAcquire(key: string): boolean;
  release(key: string): void;
}

export function createKeyedLock(): KeyedLock {
  const active = new Set<string>();
  return {
    tryAcquire(key: string): boolean {
      if (active.has(key)) return false;
      active.add(key);
      return true;
    },
    release(key: string): void {
      active.delete(key);
    },
  };
}
