/** Maior retângulo com a proporção `aspect` (largura/altura) que cabe no contêiner, centralizado. */
export function fitRect(
  containerW: number,
  containerH: number,
  aspect: number,
): { x: number; y: number; w: number; h: number } {
  let w = containerW;
  let h = w / aspect;
  if (h > containerH) {
    h = containerH;
    w = h * aspect;
  }
  return { x: (containerW - w) / 2, y: (containerH - h) / 2, w, h };
}
