import { describe, expect, it } from "vitest";
import { ANIMAL_LABEL, rejectMessage } from "./labels";

describe("labels", () => {
  it("nomes dos animais em português", () => {
    expect(ANIMAL_LABEL).toEqual({
      duck: "Pato",
      capybara: "Capivara",
      horse: "Cavalo",
      chicken: "Galinha",
    });
  });
  it("mensagens de recusa", () => {
    expect(rejectMessage("version_mismatch")).toBe("Versão diferente da do Host: atualize o app");
    expect(rejectMessage("room_full")).toBe("Sala cheia");
    expect(rejectMessage("animal_taken")).toBe("Esse animal acabou de ser escolhido, escolha outro");
  });
});
