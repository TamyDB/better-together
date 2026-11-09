import { afterEach, describe, expect, it, vi } from "vitest";
import { withTimeout } from "./timeout";

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
