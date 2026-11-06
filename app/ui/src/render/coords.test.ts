import { describe, expect, it } from "vitest";
import { backingSize, petAnchor, toPixels } from "./coords";

describe("toPixels", () => {
  it("mapeia a posição normalizada para pixels CSS", () => {
    expect(toPixels({ x: 0.5, y: 0.25 }, 1920, 1080)).toEqual({ x: 960, y: 270 });
  });

  it("mapeia os extremos para os cantos", () => {
    expect(toPixels({ x: 0, y: 0 }, 1920, 1080)).toEqual({ x: 0, y: 0 });
    expect(toPixels({ x: 1, y: 1 }, 1920, 1080)).toEqual({ x: 1920, y: 1080 });
  });

  it("funciona em tela vertical", () => {
    expect(toPixels({ x: 1, y: 1 }, 1080, 1920)).toEqual({ x: 1080, y: 1920 });
  });
});

describe("backingSize", () => {
  it("escala o canvas pelo devicePixelRatio para não borrar", () => {
    expect(backingSize(1536, 864, 1.25)).toEqual({ w: 1920, h: 1080 });
    expect(backingSize(1280, 720, 1.5)).toEqual({ w: 1920, h: 1080 });
  });
});

describe("petAnchor", () => {
  it("mantém o sprite inteiro na tela nas bordas", () => {
    // pés no centro-inferior do sprite 64×64
    expect(petAnchor({ x: 0, y: 0 }, 1920, 1080, 64)).toEqual({ x: 32, y: 64 });
    expect(petAnchor({ x: 1, y: 1 }, 1920, 1080, 64)).toEqual({ x: 1888, y: 1080 });
  });
});
