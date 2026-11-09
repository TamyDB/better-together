import { describe, expect, it } from "vitest";
import { bounceOffset } from "./placeholder";

describe("bounceOffset", () => {
  it("idle e sleep não quicam", () => {
    expect(bounceOffset("idle", 123)).toBe(0);
    expect(bounceOffset("sleep", 123)).toBe(0);
  });
  it("walk e run quicam", () => {
    expect(bounceOffset("walk", 90 * (Math.PI / 2))).toBeCloseTo(4);
  });
});
