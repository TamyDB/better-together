import type { RosterEntry, Snapshot } from "../types";
import { backingSize, clampCenter, petAnchor } from "./coords";
import { EMOTE_ICON, type Emote } from "../social";
import { BubbleBoard } from "./bubbles";
import { drawPet, PET_SIZE } from "./placeholder";

export interface Renderer {
  update(snapshot: Snapshot): void;
  setRoster(roster: RosterEntry[]): void;
  /** Balão de chat acima do pet `from` (some sozinho). */
  showChat(from: number, text: string): void;
  /** Emote acima do pet `from` (some sozinho). */
  showEmote(from: number, emote: Emote): void;
  /** Para o loop de frames e remove o listener de resize. */
  stop(): void;
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
  let running = true;
  const bubbles = new BubbleBoard(() => performance.now());

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
    if (!running) return;
    const dpr = window.devicePixelRatio;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);
    bubbles.prune();
    const area = opts.area?.() ?? { x: 0, y: 0, w: cssW, h: cssH };
    for (const pet of latest?.pets ?? []) {
      const at = petAnchor(pet.pos, area.w, area.h, PET_SIZE);
      const p = { x: area.x + at.x, y: area.y + at.y };
      drawPet(ctx, pet, p, names.get(pet.id));
      drawBubbles(ctx, p, bubbles.active(pet.id), cssW);
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  return {
    update: (s) => (latest = s),
    stop: () => {
      running = false;
      window.removeEventListener("resize", resize);
    },
    showChat: (from, text) => bubbles.chat(from, text),
    showEmote: (from, emote) => bubbles.emote(from, emote),
    setRoster: (r) => (names = new Map(r.map((e) => [e.id, e.name]))),
  };
}

const LINE_H = 16;
const EMOTE_H = 38;

/** Emote logo acima do nome e balão de chat acima dele, a pilha inteira dentro do canvas
 *  (`cssW` de largura). Só `fillText`: texto de outros jogadores nunca é interpretado como
 *  markup; o texto do balão é recortado no balão (marcas combinantes empilhadas não vazam). */
function drawBubbles(
  ctx: CanvasRenderingContext2D,
  at: { x: number; y: number },
  b: { chatLines?: string[]; emote?: Emote },
  cssW: number,
): void {
  const lines = b.chatLines;
  if (!b.emote && !lines?.length) return;
  ctx.save();
  const chatFont = "13px system-ui, sans-serif";
  ctx.font = chatFont;
  const w = lines?.length ? Math.max(...lines.map((l) => ctx.measureText(l).width)) + 16 : 0;
  const h = lines?.length ? lines.length * LINE_H + 10 : 0;
  // O nome ocupa de at.y - PET_SIZE - 20 até ~14px acima. Perto do topo, a pilha desce inteira.
  let top = Math.max(at.y - PET_SIZE - 38, (b.emote ? EMOTE_H : 0) + h);
  if (b.emote) {
    const icon = EMOTE_ICON[b.emote];
    ctx.font = "32px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.fillText(icon, clampCenter(at.x, ctx.measureText(icon).width, cssW), top);
    top -= EMOTE_H;
  }
  if (lines?.length) {
    ctx.font = chatFont;
    const cx = clampCenter(at.x, w, cssW);
    const x = cx - w / 2;
    const y = top - h;
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, 8);
    ctx.fillStyle = "#fffdf5";
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = "#2B1B10";
    ctx.stroke();
    ctx.clip();
    ctx.fillStyle = "#2B1B10";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    lines.forEach((l, i) => ctx.fillText(l, cx, y + 5 + LINE_H * (i + 0.5)));
  }
  ctx.restore();
}
