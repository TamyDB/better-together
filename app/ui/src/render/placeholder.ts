import type { Animal, PetState } from "../types";

export const PET_SIZE = 64;

const LOOK: Record<Animal, { color: string; letter: string }> = {
  duck: { color: "#F2C230", letter: "P" },
  capybara: { color: "#A26B3F", letter: "C" },
  horse: { color: "#5B3A29", letter: "V" },
  chicken: { color: "#F4F1EA", letter: "G" },
};

/** Desenha o placeholder com os pés em `at` (pixels CSS). */
export function drawPet(ctx: CanvasRenderingContext2D, pet: PetState, at: { x: number; y: number }): void {
  const { color, letter } = LOOK[pet.animal];
  const r = PET_SIZE / 2;
  const bounce = pet.anim === "idle" ? 0 : Math.abs(Math.sin(performance.now() / 90)) * 4;

  ctx.save();
  ctx.translate(at.x, at.y - r - bounce);
  if (pet.facing === "left") ctx.scale(-1, 1);

  ctx.beginPath();
  ctx.arc(0, 0, r - 2, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();
  ctx.lineWidth = 3;
  ctx.strokeStyle = "#2B1B10";
  ctx.stroke();

  // "Olho" indica para onde o bichinho olha.
  ctx.beginPath();
  ctx.arc(r * 0.45, -r * 0.25, 4, 0, Math.PI * 2);
  ctx.fillStyle = "#2B1B10";
  ctx.fill();

  ctx.scale(pet.facing === "left" ? -1 : 1, 1);
  ctx.font = "bold 24px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(letter, 0, r * 0.2);
  ctx.restore();
}
