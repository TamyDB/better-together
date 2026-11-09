import { describe, expect, it } from "vitest";
import { fitRect } from "./fit";

describe("fitRect", () => {
  it("16:9 em 1280x720 ocupa tudo", () => {
    expect(fitRect(1280, 720, 16 / 9)).toEqual({ x: 0, y: 0, w: 1280, h: 720 });
  });
  it("21:9 ganha barras em cima e embaixo", () => {
    const r = fitRect(1280, 720, 21 / 9);
    expect(r.w).toBeCloseTo(1280);
    expect(r.h).toBeCloseTo(548.57, 1);
    expect(r.x).toBeCloseTo(0);
    expect(r.y).toBeCloseTo(85.71, 1);
  });
  it("9:16 ganha barras dos lados", () => {
    const r = fitRect(1280, 720, 9 / 16);
    expect(r.h).toBeCloseTo(720);
    expect(r.w).toBeCloseTo(405);
    expect(r.x).toBeCloseTo(437.5);
    expect(r.y).toBeCloseTo(0);
  });
});
