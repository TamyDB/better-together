import { normalizeCode } from "../net/signaling";
import type { Animal, LastJoin } from "../types";

export const ANIMAL_LABEL: Record<Animal, string> = {
  duck: "Pato",
  capybara: "Capivara",
  horse: "Cavalo",
  chicken: "Galinha",
};

export type RejectReason = "version_mismatch" | "room_full" | "animal_taken";

export function rejectMessage(reason: RejectReason): string {
  switch (reason) {
    case "version_mismatch":
      return "Versão diferente da do Host: atualize o app";
    case "room_full":
      return "Sala cheia";
    case "animal_taken":
      return "Esse animal acabou de ser escolhido, escolha outro";
    default:
      return "Conexão recusada";
  }
}

/** Escolha guardada da última entrada (vem do IPC, mas validada) se for da mesma sala. */
export function rejoinChoice(last: LastJoin | null, code: string): { name: string; animal: Animal } | null {
  if (typeof last !== "object" || last === null) return null;
  const { code: c, name, animal } = last as unknown as Record<string, unknown>;
  if (c !== code || typeof name !== "string") return null;
  if (typeof animal !== "string" || !Object.hasOwn(ANIMAL_LABEL, animal)) return null;
  return { name, animal: animal as Animal };
}

/** Código da última sala (vem do IPC, mas validada), normalizado. */
export function lastCode(last: LastJoin | null): string | null {
  if (typeof last !== "object" || last === null) return null;
  const { code } = last as unknown as Record<string, unknown>;
  return typeof code === "string" ? normalizeCode(code) : null;
}
