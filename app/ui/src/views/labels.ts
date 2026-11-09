import type { Animal } from "../types";

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
