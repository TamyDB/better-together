import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { chatBox, ChatLog, emoteBar, parseChat, parseEmote } from "./chat";

class Fake {
  children: Fake[] = [];
  className = "";
  textContent = "";
  value = "";
  placeholder = "";
  maxLength = 0;
  tabIndex = 0;
  title = "";
  onclick: (() => void) | null = null;
  handlers = new Map<string, (e: unknown) => void>();
  appendChild(c: Fake) {
    this.children.push(c);
  }
  replaceChildren(...c: Fake[]) {
    this.children = c;
  }
  addEventListener(n: string, cb: (e: unknown) => void) {
    this.handlers.set(n, cb);
  }
  removeEventListener(n: string) {
    this.handlers.delete(n);
  }
}

beforeEach(() => {
  vi.stubGlobal("document", { createElement: () => new Fake() });
});

describe("ChatLog", () => {
  it("mantém só 50 e recent(6) devolve as últimas", () => {
    const log = new ChatLog();
    for (let i = 0; i < 60; i++) log.push({ from: 1, text: `m${i}` });
    expect(log.recent(1000)).toHaveLength(50);
    expect(log.recent(6).map((m) => m.text)).toEqual(["m54", "m55", "m56", "m57", "m58", "m59"]);
  });
});

describe("chatBox", () => {
  const make = (names = new Map<number, string>([[1, "Ana"]])) => {
    const sent: string[] = [];
    let done = 0;
    const box = chatBox({
      names: () => names,
      onSend: (t) => sent.push(t),
      visible: 6,
      onDone: () => done++,
    });
    const lines = () => (box.el as unknown as Fake).children[0].children.map((c) => c.textContent);
    const key = (e: Partial<KeyboardEvent>) =>
      (box.input as unknown as Fake).handlers.get("keydown")!(e);
    return { box, sent, lines, key, done: () => done };
  };

  it("nome desconhecido vira Jogador e HTML fica literal", () => {
    const { box, lines } = make();
    box.push({ from: 1, text: "oi" });
    box.push({ from: 9, text: "<b>x</b>" });
    expect(lines()).toEqual(["Ana: oi", "Jogador: <b>x</b>"]);
  });

  it("campo com limite e placeholder", () => {
    const { box } = make();
    expect(box.input.maxLength).toBe(120);
    expect(box.input.placeholder).toBe("Enter para conversar");
  });

  it("Enter envia aparado; vazio só fecha", () => {
    const { box, sent } = make();
    const key = (box.input as unknown as Fake).handlers.get("keydown")!;
    box.input.value = "  oi  ";
    key({ key: "Enter" });
    box.input.value = "   ";
    key({ key: "Enter" });
    expect(sent).toEqual(["oi"]);
  });

  it("Enter segurado (autorepeat) não envia nem fecha", () => {
    const { box, sent, key, done } = make();
    box.input.value = "oi";
    key({ key: "Enter", repeat: true });
    expect(sent).toEqual([]);
    expect(box.input.value).toBe("oi");
    expect(done()).toBe(0);
  });

  it("Enter e Esc durante a composição do IME ficam com o IME", () => {
    const { box, sent, key, done } = make();
    box.input.value = "にほん";
    key({ key: "Enter", isComposing: true });
    key({ key: "Process", keyCode: 229 } as Partial<KeyboardEvent>);
    key({ key: "Enter", keyCode: 229 } as Partial<KeyboardEvent>);
    key({ key: "Escape", isComposing: true });
    expect(sent).toEqual([]);
    expect(box.input.value).toBe("にほん");
    expect(done()).toBe(0);
    key({ key: "Enter" });
    expect(sent).toEqual(["にほん"]);
  });
});

describe("emoteBar", () => {
  it("botões fora do Tab e sem guardar o foco no clique", () => {
    const got: string[] = [];
    const bar = emoteBar((e) => got.push(e)) as unknown as Fake;
    expect(bar.children).toHaveLength(6);
    for (const b of bar.children) {
      expect(b.tabIndex).toBe(-1);
      const preventDefault = vi.fn();
      b.handlers.get("mousedown")!({ preventDefault });
      expect(preventDefault).toHaveBeenCalledTimes(1);
    }
    bar.children[0].onclick!();
    expect(got).toEqual(["heart"]);
  });
});

describe("payloads", () => {
  it("rejeita lixo", () => {
    expect(parseChat({ from: "1", text: "a" })).toBeNull();
    expect(parseChat({ from: 1.5, text: "a" })).toBeNull();
    expect(parseChat({ from: 1, text: 3 })).toBeNull();
    expect(parseChat(null)).toBeNull();
    expect(parseChat({ from: 1, text: "a" })).toEqual({ from: 1, text: "a" });
    expect(parseEmote({ from: 1, emote: "nope" })).toBeNull();
    expect(parseEmote({ from: NaN, emote: "heart" })).toBeNull();
    expect(parseEmote({ from: 2, emote: "heart" })).toEqual({ from: 2, emote: "heart" });
  });
});
