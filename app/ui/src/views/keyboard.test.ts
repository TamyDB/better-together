import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { bindGameKeys } from "./keyboard";

/** Campo falso: `focus()` muda o `activeElement` e dispara "focus", como o navegador. */
class FakeInput {
  listeners = new Map<string, Set<() => void>>();
  addEventListener(n: string, cb: () => void) {
    if (!this.listeners.has(n)) this.listeners.set(n, new Set());
    this.listeners.get(n)!.add(cb);
  }
  removeEventListener(n: string, cb: () => void) {
    this.listeners.get(n)?.delete(cb);
  }
  fire(n: string) {
    for (const cb of [...(this.listeners.get(n) ?? [])]) cb();
  }
  focus() {
    doc.activeElement = this;
    this.fire("focus");
  }
}

const doc = { activeElement: null as unknown };
const win = new Map<string, Set<(e?: unknown) => void>>();
const fireWin = (n: string, ev?: unknown) => {
  for (const cb of [...(win.get(n) ?? [])]) cb(ev);
};

let input: FakeInput;
let keys: { code: string; down: boolean }[];
let dispose: () => void;

/** Tecla no `window`, com o alvo no elemento focado (como o navegador entrega). */
const press = (type: "keydown" | "keyup", code: string, extra: Record<string, unknown> = {}) =>
  fireWin(type, { code, key: code, target: doc.activeElement, repeat: false, ...extra });

beforeEach(() => {
  doc.activeElement = null;
  win.clear();
  vi.stubGlobal("document", {
    get activeElement() {
      return doc.activeElement;
    },
  });
  vi.stubGlobal("window", {
    addEventListener: (n: string, cb: (e?: unknown) => void) => {
      if (!win.has(n)) win.set(n, new Set());
      win.get(n)!.add(cb);
    },
    removeEventListener: (n: string, cb: (e?: unknown) => void) => win.get(n)?.delete(cb),
  });
  input = new FakeInput();
  keys = [];
  dispose = bindGameKeys({
    input: input as unknown as HTMLInputElement,
    onKey: (code, down) => keys.push({ code, down }),
  });
});

afterEach(() => {
  dispose();
  vi.unstubAllGlobals();
});

describe("bindGameKeys", () => {
  it("fora do campo, as teclas vão para onKey", () => {
    press("keydown", "KeyW");
    press("keyup", "KeyW");
    expect(keys).toEqual([
      { code: "KeyW", down: true },
      { code: "KeyW", down: false },
    ]);
  });

  it("digitar no campo não chama onKey", () => {
    input.focus();
    for (const c of ["KeyW", "KeyA", "Digit1", "Escape"]) {
      press("keydown", c);
      press("keyup", c);
    }
    expect(keys).toEqual([]);
  });

  it("alvo no campo também conta como digitando", () => {
    fireWin("keydown", { code: "KeyW", key: "w", target: input, repeat: false });
    expect(keys).toEqual([]);
  });

  it("Enter sem foco foca o campo e não é repassado", () => {
    press("keydown", "Enter");
    expect(doc.activeElement).toBe(input);
    expect(keys).toEqual([]);
  });

  it("Enter segurado (autorepeat) não foca o campo", () => {
    press("keydown", "Enter", { repeat: true });
    expect(doc.activeElement).toBeNull();
    expect(keys).toEqual([]);
  });

  it("keyup de Enter fora do campo é ignorado", () => {
    press("keyup", "Enter");
    expect(keys).toEqual([]);
  });

  it("Enter solta as teclas pressionadas antes de focar", () => {
    press("keydown", "KeyW");
    press("keydown", "ShiftLeft");
    keys = [];
    press("keydown", "Enter");
    expect(keys).toEqual([
      { code: "KeyW", down: false },
      { code: "ShiftLeft", down: false },
    ]);
  });

  it("focar o campo por clique (evento focus) solta as teclas", () => {
    press("keydown", "KeyD");
    keys = [];
    doc.activeElement = input;
    input.fire("focus");
    expect(keys).toEqual([{ code: "KeyD", down: false }]);
  });

  it("blur da janela solta as teclas uma vez só", () => {
    press("keydown", "KeyA");
    keys = [];
    fireWin("blur");
    fireWin("blur");
    expect(keys).toEqual([{ code: "KeyA", down: false }]);
  });

  it("dispose solta as teclas e para de escutar", () => {
    press("keydown", "KeyS");
    keys = [];
    dispose();
    expect(keys).toEqual([{ code: "KeyS", down: false }]);
    press("keydown", "KeyW");
    press("keydown", "Enter");
    input.fire("focus");
    expect(keys).toEqual([{ code: "KeyS", down: false }]);
    expect(doc.activeElement).toBeNull();
  });
});
