import { describe, expect, it } from "vitest";
import { ANIMAL_LABEL, lastCode, rejectMessage, rejoinChoice } from "./labels";

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

describe("rejoinChoice", () => {
  const last = { code: "PATO-4821", name: "Ana", animal: "capybara" };

  it("devolve nome e animal quando o código bate", () => {
    expect(rejoinChoice(last, "PATO-4821")).toEqual({ name: "Ana", animal: "capybara" });
  });

  it("ignora outra sala", () => {
    expect(rejoinChoice(last, "GATO-1111")).toBeNull();
  });

  it("ignora lixo e null", () => {
    expect(rejoinChoice(null, "PATO-4821")).toBeNull();
    expect(rejoinChoice("x", "PATO-4821")).toBeNull();
    expect(rejoinChoice({ code: "PATO-4821" }, "PATO-4821")).toBeNull();
    expect(rejoinChoice({ ...last, name: 3 }, "PATO-4821")).toBeNull();
  });

  it("ignora animal inválido", () => {
    expect(rejoinChoice({ ...last, animal: "dragao" }, "PATO-4821")).toBeNull();
  });
});

describe("lastCode", () => {
  it("devolve o código normalizado", () => {
    expect(lastCode({ code: "pato4821" })).toBe("PATO-4821");
  });

  it("ignora dado ausente ou inválido", () => {
    expect(lastCode(null)).toBeNull();
    expect(lastCode("PATO-4821")).toBeNull();
    expect(lastCode({ code: 7 })).toBeNull();
    expect(lastCode({ code: "<img src=x>" })).toBeNull();
  });
});
