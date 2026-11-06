import type { Vec2 } from "../types";

/** Converte posição normalizada [0,1] em pixels CSS de uma área `cssW × cssH`. */
export function toPixels(pos: Vec2, cssW: number, cssH: number): { x: number; y: number } {
  return { x: pos.x * cssW, y: pos.y * cssH };
}

/** Tamanho do backing store do canvas para ficar nítido com escala de DPI. */
export function backingSize(cssW: number, cssH: number, dpr: number): { w: number; h: number } {
  return { w: Math.round(cssW * dpr), h: Math.round(cssH * dpr) };
}

/** Ponto dos pés de um sprite `size × size`, com a área recuada para que o sprite
 *  inteiro caiba na tela mesmo com pos em 0 ou 1. */
export function petAnchor(pos: Vec2, cssW: number, cssH: number, size: number): { x: number; y: number } {
  const p = toPixels(pos, cssW - size, cssH - size);
  return { x: p.x + size / 2, y: p.y + size };
}
