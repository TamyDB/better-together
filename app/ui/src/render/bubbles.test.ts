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
    expect(board.active(1)).toMatchObject({ chat: "a", chatLines: ["a"], emote: "wow" });
    expect(board.active(2)).toEqual({ chat: "b", chatLines: ["b"] });
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
    expect(isEmote("toString")).toBe(false);
    expect(isEmote("__proto__")).toBe(false);
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
  it("vazio ou só espaços dá []", () => {
    expect(wrapText("", 10, 3)).toEqual([]);
    expect(wrapText("   \n ", 10, 3)).toEqual([]);
  });
  it("width <= 0 não trava", () => {
    expect(wrapText("abc", 0, 5)).toEqual(["a", "b", "c"]);
    expect(wrapText("ab", -3, 5)).toEqual(["a", "b"]);
  });
  it("não corta par substituto", () => {
    const lines = wrapText("a" + "😂".repeat(20), 4, 10);
    for (const l of lines) expect(l).not.toMatch(/[\uD800-\uDBFF]$|^[\uDC00-\uDFFF]/);
    expect(lines.join("")).toBe("a" + "😂".repeat(20));
    expect(lines[0]).toBe("a😂😂😂");
  });
  it("mantém VS16 junto do coração", () => {
    const lines = wrapText("❤️".repeat(40), 28, 5);
    expect(lines.join("")).toBe("❤️".repeat(40));
    for (const l of lines) expect(l.startsWith("\uFE0F")).toBe(false);
  });
  it("linha com reticências tem no máximo width grafemas", () => {
    const lines = wrapText("😂".repeat(100), 10, 3);
    expect(lines).toHaveLength(3);
    const seg = [...new Intl.Segmenter("pt", { granularity: "grapheme" }).segment(lines[2])];
    expect(seg.length).toBeLessThanOrEqual(10);
    expect(lines[2].endsWith("…")).toBe(true);
  });
});
