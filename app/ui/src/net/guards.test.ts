import { describe, expect, it, vi } from "vitest";
import { IceQueue, once, tooBig } from "./guards";

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
