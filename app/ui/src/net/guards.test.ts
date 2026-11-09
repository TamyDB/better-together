import { describe, expect, it, vi } from "vitest";
import { ChannelLimiter, IceQueue, once, tooBig } from "./guards";

describe("once", () => {
  it("executa uma vez só", () => {
    const f = vi.fn();
    const g = once(f);
    g();
    g();
    expect(f).toHaveBeenCalledTimes(1);
  });
});

describe("tooBig", () => {
  it("limita em 16384 bytes", () => {
    expect(tooBig(new Uint8Array(16384))).toBe(false);
    expect(tooBig(new Uint8Array(16385))).toBe(true);
  });
});

describe("IceQueue", () => {
  it("enfileira até haver descrição remota e depois aplica na ordem", async () => {
    const applied: number[] = [];
    const q = new IceQueue<number>(async (c) => void applied.push(c));
    await q.add(1);
    await q.add(2);
    expect(applied).toEqual([]);
    await q.remoteReady();
    expect(applied).toEqual([1, 2]);
    await q.add(3);
    expect(applied).toEqual([1, 2, 3]);
  });

  it("falha ao aplicar um candidato não derruba os demais", async () => {
    const applied: number[] = [];
    const q = new IceQueue<number>(async (c) => {
      if (c === 1) throw new Error("ruim");
      applied.push(c);
    });
    await q.add(1);
    await q.add(2);
    await q.remoteReady();
    expect(applied).toEqual([2]);
  });
});

describe("ChannelLimiter", () => {
  function limiter() {
    const clock = { ms: 1000 };
    return { clock, l: new ChannelLimiter(() => clock.ms) };
  }
  const accepted = (l: ChannelLimiter, peer: number, channel: "input" | "events" | "state", n: number) => {
    let ok = 0;
    for (let i = 0; i < n; i++) if (l.allow(peer, channel)) ok++;
    return ok;
  };

  it("aceita a rajada igual à taxa e descarta o excesso", () => {
    const { l } = limiter();
    expect(accepted(l, 1, "input", 300)).toBe(240);
    expect(accepted(l, 1, "events", 50)).toBe(32);
  });

  it("recarrega com o tempo, sem passar da rajada", () => {
    const { l, clock } = limiter();
    accepted(l, 1, "events", 32);
    expect(l.allow(1, "events")).toBe(false);
    clock.ms += 500;
    expect(accepted(l, 1, "events", 50)).toBe(16);
    clock.ms += 60_000;
    expect(accepted(l, 1, "events", 50)).toBe(32);
  });

  it("buckets independentes por peer e por canal", () => {
    const { l } = limiter();
    accepted(l, 1, "events", 32);
    expect(l.allow(1, "events")).toBe(false);
    expect(l.allow(2, "events")).toBe(true);
    expect(l.allow(1, "input")).toBe(true);
  });

  it("state vindo de convidado é sempre descartado", () => {
    const { l, clock } = limiter();
    expect(accepted(l, 1, "state", 10)).toBe(0);
    clock.ms += 60_000;
    expect(accepted(l, 1, "state", 10)).toBe(0);
  });

  it("relógio que volta não recarrega nem quebra", () => {
    const { l, clock } = limiter();
    accepted(l, 1, "events", 32);
    clock.ms -= 500;
    expect(l.allow(1, "events")).toBe(false);
  });

  it("forget devolve o peer ao estado inicial", () => {
    const { l } = limiter();
    accepted(l, 1, "events", 32);
    l.forget(1);
    expect(accepted(l, 1, "events", 50)).toBe(32);
  });
});
