import { describe, expect, it } from "vitest";
import { CHAT_MS, EMOTE_MS, isEmote } from "../social";
import { BubbleBoard, wrapText } from "./bubbles";

function setup() {
  let t = 0;
  const board = new BubbleBoard(() => t);
  return { board, advance: (ms: number) => (t += ms) };
}

describe("BubbleBoard", () => {
  it("chat expira em CHAT_MS", () => {
    const { board, advance } = setup();
    board.chat(1, "oi");
    advance(CHAT_MS - 1);
    expect(board.active(1).chat).toBe("oi");
    advance(1);
    expect(board.active(1).chat).toBeUndefined();
  });

  it("emote expira em EMOTE_MS", () => {
    const { board, advance } = setup();
    board.emote(1, "heart");
    advance(EMOTE_MS - 1);
    expect(board.active(1).emote).toBe("heart");
    advance(1);
    expect(board.active(1).emote).toBeUndefined();
  });

  it("novo chat do mesmo jogador substitui e reinicia o prazo", () => {
    const { board, advance } = setup();
    board.chat(1, "a");
    advance(3000);
    board.chat(1, "b");
    advance(3000);
    expect(board.active(1).chat).toBe("b");
  });

  it("jogadores diferentes e tipos diferentes coexistem", () => {
    const { board } = setup();
    board.chat(1, "a");
    board.chat(2, "b");
    board.emote(1, "wow");
    expect(board.active(1)).toEqual({ chat: "a", emote: "wow" });
    expect(board.active(2)).toEqual({ chat: "b" });
  });

  it("prune descarta os vencidos", () => {
    const { board, advance } = setup();
    board.chat(1, "a");
    advance(CHAT_MS);
    board.prune();
    expect(board.active(1)).toEqual({});
  });
});

describe("isEmote", () => {
  it("aceita os 6 e recusa o resto", () => {
    for (const e of ["heart", "laugh", "wow", "angry", "music", "question"]) {
      expect(isEmote(e)).toBe(true);
    }
    expect(isEmote("HEART")).toBe(false);
    expect(isEmote("")).toBe(false);
    expect(isEmote(1)).toBe(false);
  });
});

describe("wrapText", () => {
  it("quebra por palavra", () => {
    expect(wrapText("aaa bbb ccc", 7, 3)).toEqual(["aaa bbb", "ccc"]);
  });
  it("corta palavra maior que a linha", () => {
    expect(wrapText("abcdefghij", 4, 3)).toEqual(["abcd", "efgh", "ij"]);
  });
  it("põe reticências quando passa do limite de linhas", () => {
    const lines = wrapText("aaaa bbbb cccc dddd", 4, 3);
    expect(lines).toEqual(["aaaa", "bbbb", "ccc…"]);
  });
  it("texto que cabe fica intacto", () => {
    expect(wrapText("oi", 28, 3)).toEqual(["oi"]);
  });
});
