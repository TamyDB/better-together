import { afterEach, describe, expect, it, vi } from "vitest";
import { deadline, withTimeout } from "./timeout";

afterEach(() => vi.useRealTimers());

describe("withTimeout", () => {
  it("rejeita depois de ms", async () => {
    vi.useFakeTimers();
    const p = withTimeout(new Promise<void>(() => {}), 1000, "estourou");
    const assertion = expect(p).rejects.toThrow("estourou");
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
  });

  it("passa o valor se resolver antes", async () => {
    vi.useFakeTimers();
    await expect(withTimeout(Promise.resolve(7), 1000, "x")).resolves.toBe(7);
  });

  it("propaga a rejeição original", async () => {
    await expect(withTimeout(Promise.reject(new Error("a")), 1000, "x")).rejects.toThrow("a");
  });
});

describe("deadline", () => {
  it("dispara uma vez, só depois de armado", async () => {
    vi.useFakeTimers();
    const f = vi.fn();
    const d = deadline(1000, f);
    await vi.advanceTimersByTimeAsync(5000);
    expect(f).not.toHaveBeenCalled();
    d.arm();
    await vi.advanceTimersByTimeAsync(999);
    expect(f).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(f).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5000);
    expect(f).toHaveBeenCalledTimes(1);
  });

  it("settle cancela a contagem", async () => {
    vi.useFakeTimers();
    const f = vi.fn();
    const d = deadline(1000, f);
    d.arm();
    d.settle();
    await vi.advanceTimersByTimeAsync(5000);
    expect(f).not.toHaveBeenCalled();
  });

  it("settle antes de arm impede a contagem", async () => {
    vi.useFakeTimers();
    const f = vi.fn();
    const d = deadline(1000, f);
    d.settle();
    d.arm();
    await vi.advanceTimersByTimeAsync(5000);
    expect(f).not.toHaveBeenCalled();
  });

  it("arm repetido não cria um segundo timer", async () => {
    vi.useFakeTimers();
    const f = vi.fn();
    const d = deadline(1000, f);
    d.arm();
    d.arm();
    d.settle();
    await vi.advanceTimersByTimeAsync(5000);
    expect(f).not.toHaveBeenCalled();
  });
});
