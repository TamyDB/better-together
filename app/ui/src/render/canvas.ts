import type { Snapshot } from "../types";
import { backingSize, petAnchor } from "./coords";
import { drawPet, PET_SIZE } from "./placeholder";

export interface Renderer {
  update(snapshot: Snapshot): void;
}

/** Desenha o último snapshot a cada frame. */
export function startRenderer(canvas: HTMLCanvasElement): Renderer {
  const ctx = canvas.getContext("2d")!;
  let latest: Snapshot | null = null;
  let cssW = 0;
  let cssH = 0;

  const resize = () => {
    cssW = window.innerWidth;
    cssH = window.innerHeight;
    const { w, h } = backingSize(cssW, cssH, window.devicePixelRatio);
    canvas.width = w;
    canvas.height = h;
  };
  window.addEventListener("resize", resize);
  resize();

  const frame = () => {
    const dpr = window.devicePixelRatio;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);
    for (const pet of latest?.pets ?? []) {
      drawPet(ctx, pet, petAnchor(pet.pos, cssW, cssH, PET_SIZE));
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  return { update: (s) => (latest = s) };
}
