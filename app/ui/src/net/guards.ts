// Peças puras do bridge/rtc, isoladas para testar sem WebRTC.

/** Mesmo limite de `MAX_MESSAGE_BYTES` do protocolo. */
export const MAX_MESSAGE_BYTES = 16384;

export function tooBig(bytes: Uint8Array): boolean {
  return bytes.byteLength > MAX_MESSAGE_BYTES;
}

/** Executa `f` só na primeira chamada. */
export function once(f: () => void): () => void {
  let done = false;
  return () => {
    if (done) return;
    done = true;
    f();
  };
}

/**
 * Candidatos ICE que chegam antes do `setRemoteDescription` esperam aqui.
 * Falha ao aplicar um candidato é ignorada (dado de sinalização não é confiável).
 */
export class IceQueue<T> {
  private pending: T[] = [];
  private ready = false;

  constructor(private readonly apply: (c: T) => Promise<void>) {}

  async add(c: T): Promise<void> {
    if (!this.ready) {
      this.pending.push(c);
      return;
    }
    await this.safeApply(c);
  }

  async remoteReady(): Promise<void> {
    this.ready = true;
    const batch = this.pending;
    this.pending = [];
    for (const c of batch) await this.safeApply(c);
  }

  private async safeApply(c: T): Promise<void> {
    try {
      await this.apply(c);
    } catch {
      // candidato inválido ou fora de hora: ignora
    }
  }
}
