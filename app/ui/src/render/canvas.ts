import type { RosterEntry, Snapshot } from "../types";
import { backingSize, petAnchor } from "./coords";
import { drawPet, PET_SIZE } from "./placeholder";

export interface Renderer {
  update(snapshot: Snapshot): void;
  setRoster(roster: RosterEntry[]): void;
}

export interface Area {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface RendererOpts {
  /** Área (pixels CSS do canvas) onde os pets são desenhados. Padrão: o canvas inteiro. */
  area?: () => Area;
}

/** Desenha o último snapshot a cada frame. */
export function startRenderer(canvas: HTMLCanvasElement, opts: RendererOpts = {}): Renderer {
  const ctx = canvas.getContext("2d")!;
  let latest: Snapshot | null = null;
  let names = new Map<number, string>();
  let cssW = 0;
  let cssH = 0;

  const resize = () => {
    cssW = canvas.clientWidth || window.innerWidth;
    cssH = canvas.clientHeight || window.innerHeight;
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
    const area = opts.area?.() ?? { x: 0, y: 0, w: cssW, h: cssH };
    for (const pet of latest?.pets ?? []) {
      const at = petAnchor(pet.pos, area.w, area.h, PET_SIZE);
      drawPet(ctx, pet, { x: area.x + at.x, y: area.y + at.y }, names.get(pet.id));
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  return {
    update: (s) => (latest = s),
    setRoster: (r) => (names = new Map(r.map((e) => [e.id, e.name]))),
  };
}
