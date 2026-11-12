import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  invoke: vi.fn(async (..._a: unknown[]) => undefined as unknown),
  listeners: new Map<string, (e: { payload: unknown }) => void>(),
  startGuestNet: vi.fn(),
  stop: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: h.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    h.listeners.set(name, cb);
    return () => {};
  }),
}));
vi.mock("../net/bridge", () => ({ startGuestNet: h.startGuestNet, startHostNet: vi.fn() }));
vi.mock("../render/canvas", () => ({
  startRenderer: () => ({ update() {}, setRoster() {}, stop() {} }),
}));

import { startMainWindow } from "./main-window";

/** DOM mínimo: só o que `main-window` usa, para exercitar a fiação sem navegador. */
class FakeEl {
  children: FakeEl[] = [];
  id = "";
  className = "";
  textContent = "";
  value = "";
  placeholder = "";
  maxLength = 0;
  disabled = false;
  clientWidth = 0;
  clientHeight = 0;
  style = { cssText: "" };
  classList = { toggle() {} };
  onclick: (() => void) | null = null;
  oninput: (() => void) | null = null;
  onkeydown: ((e: unknown) => void) | null = null;
  constructor(readonly tag: string) {}
  appendChild(c: FakeEl) {
    this.children.push(c);
    return c;
  }
  replaceChildren(...c: FakeEl[]) {
    this.children = c;
  }
  querySelector() {
    return null;
  }
  remove() {}
}

let app: FakeEl;
/** Lembrança que o `last_join` devolve; `remember_join` a atualiza, como o Rust. */
let last: unknown = null;
let reload: ReturnType<typeof vi.fn>;

function walk(e: FakeEl): FakeEl[] {
  return [e, ...e.children.flatMap(walk)];
}
const title = () => walk(app).find((e) => e.tag === "h1")?.textContent;
const byText = (tag: string, text: string) =>
  walk(app).find((e) => e.tag === tag && e.textContent === text);
const codeInput = () => walk(app).find((e) => e.placeholder === "Código da sala")!;
const nameInput = () => walk(app).find((e) => e.placeholder === "Seu nome")!;
const flush = () => vi.advanceTimersByTimeAsync(0);
const emit = (name: string, payload: unknown = null) => h.listeners.get(name)!({ payload });
const chooses = () => h.invoke.mock.calls.filter((c) => c[0] === "guest_choose").map((c) => c[1]);
const remembers = () =>
  h.invoke.mock.calls.filter((c) => c[0] === "remember_join").map((c) => c[1]);

async function launch(): Promise<void> {
  startMainWindow();
  await flush();
}

/** Abre o launcher e entra na sala; `startGuestNet` resolve quando `net` resolver. */
async function enter(net: Promise<unknown> = Promise.resolve({ stop: h.stop }), nick = ""): Promise<void> {
  h.startGuestNet.mockReturnValue(net);
  await launch();
  nameInput().value = nick;
  const code = codeInput();
  code.value = "PATO-4821";
  code.oninput?.();
  byText("button", "Entrar")!.onclick?.();
  await flush();
}

beforeEach(() => {
  vi.useFakeTimers();
  h.invoke.mockClear();
  last = null;
  h.invoke.mockImplementation(async (cmd: unknown, args: unknown) => {
    if (cmd === "last_join") return last;
    if (cmd === "remember_join") last = args;
    return undefined;
  });
  h.listeners.clear();
  h.startGuestNet.mockReset();
  h.stop.mockClear();
  reload = vi.fn();
  const body = new FakeEl("body");
  vi.stubGlobal("document", {
    body: {
      appendChild: (e: FakeEl) => {
        app = e;
        body.appendChild(e);
      },
    },
    createElement: (tag: string) => new FakeEl(tag),
    getElementById: () => app,
  });
  vi.stubGlobal("window", { addEventListener() {}, removeEventListener() {} });
  vi.stubGlobal("location", { reload });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("espera depois de conectar", () => {
  const TIMEOUT_MS = 15000;

  it("sem lobby em 15 s vai para a tela de erro", async () => {
    await enter();
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS - 1);
    expect(title()).toBe("Conectando...");
    await vi.advanceTimersByTimeAsync(1);
    expect(title()).toBe("Não foi possível entrar");
    expect(byText("div", "Não foi possível conectar ao Host")).toBeDefined();
    byText("button", "Voltar")!.onclick?.();
    await flush();
    expect(h.stop).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["session://lobby", { taken: [] }, "Escolha seu bichinho"],
    ["session://rejected", { reason: "room_full" }, "Escolha seu bichinho"],
    ["session://welcome", { animal: "duck", aspect: 2 }, undefined],
  ])("%s cancela o prazo", async (name, payload, expected) => {
    await enter();
    emit(name, payload);
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS * 2);
    expect(title()).toBe(expected);
  });

  it("lobby antes de startGuestNet resolver não deixa o prazo armar", async () => {
    let open!: (v: unknown) => void;
    await enter(new Promise((r) => (open = r)));
    emit("session://lobby", { taken: [] });
    open({ stop: h.stop });
    await flush();
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS * 2);
    expect(title()).toBe("Escolha seu bichinho");
  });

  it("host-lost durante a espera limpa o prazo", async () => {
    await enter();
    emit("session://host-lost");
    expect(title()).toBe("O Host saiu");
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS * 2);
    expect(title()).toBe("O Host saiu");
  });

  it("Cancelar limpa o prazo", async () => {
    await enter();
    byText("button", "Cancelar")!.onclick?.();
    await flush();
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS * 2);
    expect(title()).toBe("Conectando...");
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("Cancelar antes de startGuestNet resolver não arma o prazo", async () => {
    let open!: (v: unknown) => void;
    await enter(new Promise((r) => (open = r)));
    byText("button", "Cancelar")!.onclick?.();
    open({ stop: h.stop });
    await flush();
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS * 2);
    expect(title()).toBe("Conectando...");
  });
});

describe("lembrança de reconexão", () => {
  const SAVED = { code: "PATO-4821", name: "Tamy", animal: "duck" };

  it("reconexão automática guarda o nome enviado, não o do campo vazio", async () => {
    last = SAVED;
    await enter();
    emit("session://lobby", { taken: [] });
    await flush();
    expect(chooses()).toEqual([{ name: "Tamy", animal: "duck" }]);
    emit("session://welcome", { animal: "duck", aspect: 2 });
    expect(remembers()).toEqual([SAVED]);
  });

  it("dois lobbies durante a leitura da lembrança escolhem uma vez só", async () => {
    last = SAVED;
    await enter();
    emit("session://lobby", { taken: [] });
    emit("session://lobby", { taken: ["horse"] });
    await flush();
    expect(chooses()).toEqual([{ name: "Tamy", animal: "duck" }]);
  });

  it("escolha manual guarda o nome do campo", async () => {
    await enter(undefined, "Ana");
    emit("session://lobby", { taken: [] });
    await flush();
    byText("span", "Capivara")!;
    walk(app)
      .filter((e) => e.tag === "button" && e.className === "card")[1]
      .onclick?.();
    expect(chooses()).toEqual([{ name: "Ana", animal: "capybara" }]);
    emit("session://welcome", { animal: "capybara", aspect: 2 });
    expect(remembers()).toEqual([{ code: "PATO-4821", name: "Ana", animal: "capybara" }]);
  });

  it("depois de a tentativa automática ser recusada, guarda o nome da escolha manual", async () => {
    last = SAVED;
    await enter(undefined, "Ana");
    emit("session://lobby", { taken: [] });
    await flush();
    emit("session://rejected", { reason: "room_full" });
    walk(app)
      .filter((e) => e.tag === "button" && e.className === "card")[2]
      .onclick?.();
    emit("session://welcome", { animal: "horse", aspect: 2 });
    expect(remembers()).toEqual([{ code: "PATO-4821", name: "Ana", animal: "horse" }]);
  });

  it("pré-preenche o código da última sala e habilita Entrar", async () => {
    last = SAVED;
    await launch();
    expect(codeInput().value).toBe("PATO-4821");
    expect(byText("button", "Entrar")!.disabled).toBe(false);
  });

  it("sem lembrança válida o campo fica vazio", async () => {
    last = "{lixo";
    await launch();
    expect(codeInput().value).toBe("");
    expect(byText("button", "Entrar")!.disabled).toBe(true);
  });
});
