import { describe, expect, it } from "vitest";
import { CHANNELS } from "./channels";

describe("CHANNELS", () => {
  it("ids distintos e negotiated", () => {
    const ids = Object.values(CHANNELS).map((c) => c.id);
    expect(new Set(ids).size).toBe(3);
    expect(ids).toEqual([0, 1, 2]);
    for (const c of Object.values(CHANNELS)) expect(c.negotiated).toBe(true);
  });

  it("state/input não confiáveis, events confiável", () => {
    for (const k of ["state", "input"] as const) {
      expect(CHANNELS[k].ordered).toBe(false);
      expect(CHANNELS[k].maxRetransmits).toBe(0);
    }
    expect(CHANNELS.events.ordered).toBe(true);
    expect(CHANNELS.events.maxRetransmits).toBeUndefined();
  });
});
