// Peças puras do bridge/rtc, isoladas para testar sem WebRTC.

import type { ChannelName } from "./channels";

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

/** Mensagens por segundo aceitas de um convidado (a rajada é igual à taxa). `state` só desce. */
export const GUEST_RATE: Record<ChannelName, number> = { state: 0, input: 240, events: 32 };

/** Token bucket por peer e por canal. O relógio (ms, monotônico) é injetado. */
export class ChannelLimiter {
  private readonly peers = new Map<number, Map<ChannelName, { tokens: number; at: number }>>();

  constructor(private readonly now: () => number) {}

  allow(peer: number, channel: ChannelName): boolean {
    const rate = GUEST_RATE[channel];
    if (!rate) return false;
    let buckets = this.peers.get(peer);
    if (!buckets) this.peers.set(peer, (buckets = new Map()));
    const at = this.now();
    let b = buckets.get(channel);
    if (!b) buckets.set(channel, (b = { tokens: rate, at }));
    b.tokens = Math.min(rate, b.tokens + (Math.max(0, at - b.at) * rate) / 1000);
    b.at = at;
    if (b.tokens < 1) return false;
    b.tokens -= 1;
    return true;
  }

  forget(peer: number): void {
    this.peers.delete(peer);
  }
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
