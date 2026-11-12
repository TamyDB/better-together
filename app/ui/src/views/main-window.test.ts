import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  invoke: vi.fn(async (..._a: unknown[]) => undefined as unknown),
  listeners: new Map<string, (e: { payload: unknown }) => void>(),
  startGuestNet: vi.fn(),
  startHostNet: vi.fn(),
  stop: vi.fn(async () => {}),
  setVideo: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: h.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    h.listeners.set(name, cb);
    return () => {};
  }),
}));
vi.mock("../net/bridge", () => ({ startGuestNet: h.startGuestNet, startHostNet: h.startHostNet }));
vi.mock("../render/canvas", () => ({
  startRenderer: () => ({ update() {}, setRoster() {}, stop() {} }),
}));

import { GUEST_PAUSED_TEXT, GUEST_WAITING_TEXT } from "../net/video";
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
  hidden = false;
  srcObject: unknown = null;
  muted = false;
  autoplay = false;
  playsInline = false;
  attrs = new Map<string, string>();
  listeners = new Map<string, Set<() => void>>();
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
  setAttribute(name: string, value: string) {
    this.attrs.set(name, value);
  }
  addEventListener(name: string, cb: () => void) {
    if (!this.listeners.has(name)) this.listeners.set(name, new Set());
    this.listeners.get(name)!.add(cb);
  }
  removeEventListener(name: string, cb: () => void) {
    this.listeners.get(name)?.delete(cb);
  }
  fire(name: string) {
    for (const cb of this.listeners.get(name) ?? []) cb();
  }
}

/** Track falsa de captura (Host) ou recebida (convidado). */
function fakeTrack() {
  const listeners = new Map<string, Set<() => void>>();
  const t = {
    contentHint: "",
    onended: null as (() => void) | null,
    stop: vi.fn(),
    getSettings: (): MediaTrackSettings => ({ displaySurface: "monitor", width: 1280, height: 720 }),
    addEventListener(name: string, cb: () => void) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name)!.add(cb);
    },
    removeEventListener(name: string, cb: () => void) {
      listeners.get(name)?.delete(cb);
    },
    fire(name: string) {
      for (const cb of listeners.get(name) ?? []) cb();
    },
    listenerCount: () => [...listeners.values()].reduce((n, s) => n + s.size, 0),
  };
  return t;
}
type FakeTrack = ReturnType<typeof fakeTrack>;
const streamOf = (...tracks: FakeTrack[]) => ({
  getVideoTracks: () => tracks,
  getTracks: () => tracks,
});

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

/** Resposta do `host_start` nos testes. */
let started: unknown = { aspect: 16 / 9, overlay_in_video: false };
/** Ordem de `getDisplayMedia` e dos `invoke`s, para conferir o gesto do usuário. */
let log: string[] = [];
let getDisplayMedia: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.useFakeTimers();
  h.invoke.mockClear();
  last = null;
  started = { aspect: 16 / 9, overlay_in_video: false };
  log = [];
  h.invoke.mockImplementation(async (cmd: unknown, args: unknown) => {
    log.push(String(cmd));
    if (cmd === "last_join") return last;
    if (cmd === "remember_join") last = args;
    if (cmd === "host_start") return started;
    return undefined;
  });
  h.listeners.clear();
  h.startGuestNet.mockReset();
  h.startHostNet.mockReset();
  h.startHostNet.mockImplementation(async (_cfg: unknown, onCode: (c: string) => void) => {
    onCode("PATO-4821");
    return { stop: h.stop, setVideo: h.setVideo };
  });
  h.stop.mockClear();
  h.setVideo.mockClear();
  getDisplayMedia = vi.fn(async () => {
    log.push("capture");
    throw new DOMException("recusado", "NotAllowedError");
  });
  vi.stubGlobal("navigator", { mediaDevices: { getDisplayMedia }, clipboard: {} });
  vi.stubGlobal(
    "MediaStream",
    class {
      constructor(readonly tracks: unknown[]) {}
      getTracks() {
        return this.tracks;
      }
    },
  );
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

describe("lobby com a lembrança ainda sendo lida", () => {
  const SAVED = { code: "PATO-4821", name: "Tamy", animal: "duck" };

  it("Cancelar durante a leitura não escolhe nada", async () => {
    last = SAVED;
    await enter();
    emit("session://lobby", { taken: [] });
    byText("button", "Cancelar")!.onclick?.();
    await flush();
    expect(chooses()).toEqual([]);
  });

  it("rejeição durante a leitura deixa o lobby manual", async () => {
    last = SAVED;
    await enter();
    emit("session://lobby", { taken: [] });
    emit("session://rejected", { reason: "room_full" });
    await flush();
    expect(chooses()).toEqual([]);
    expect(title()).toBe("Escolha seu bichinho");
  });
});

/** Escolhe o primeiro bichinho e clica em "Criar sala" (sem esperar nada). */
async function clickCreate(): Promise<void> {
  await launch();
  walk(app)
    .filter((e) => e.tag === "button" && e.className === "card")[0]
    .onclick?.();
  log = [];
  byText("button", "Criar sala")!.onclick?.();
}

/** Captura que só resolve quando o teste mandar. */
function pendingCapture() {
  let resolve!: (s: unknown) => void;
  getDisplayMedia.mockImplementation(() => new Promise((r) => (resolve = r)));
  return (t: FakeTrack) => resolve(streamOf(t));
}

const videoStatus = () => walk(app).find((e) => e.className === "video-status");

describe("compartilhamento de tela no Host", () => {
  it("Criar sala abre o seletor antes do primeiro invoke", async () => {
    await clickCreate();
    expect(getDisplayMedia).toHaveBeenCalledTimes(1);
    expect(log[0]).toBe("capture");
    await flush();
    expect(log.slice(0, 2)).toEqual(["capture", "host_start"]);
  });

  it("com o seletor recusado a sala abre e oferece Compartilhar tela", async () => {
    await clickCreate();
    await flush();
    expect(byText("div", "PATO-4821")).toBeDefined();
    expect(h.startHostNet.mock.calls[0][3]).toBeNull();
    expect(byText("div", "Tela não compartilhada")).toBeDefined();
    expect(byText("button", "Compartilhar tela")).toBeDefined();
    expect(byText("button", "Pausar vídeo")).toBeUndefined();
  });

  it("sem navigator.mediaDevices a sala abre sem vídeo", async () => {
    vi.stubGlobal("navigator", {});
    await clickCreate();
    await flush();
    expect(byText("div", "PATO-4821")).toBeDefined();
    expect(byText("button", "Compartilhar tela")).toBeDefined();
  });

  it("track escolhida antes da sala vai para o startHostNet", async () => {
    const t = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(t));
    await clickCreate();
    await flush();
    expect(h.startHostNet.mock.calls[0][3]).toBe(t);
    expect(t.contentHint).toBe("detail");
    expect(byText("div", "Compartilhando a tela")).toBeDefined();
    expect(byText("button", "Pausar vídeo")).toBeDefined();
    expect(byText("button", "Trocar tela")).toBeDefined();
  });

  it("track escolhida depois da sala vai por setVideo", async () => {
    const pick = pendingCapture();
    await clickCreate();
    await flush();
    expect(h.startHostNet.mock.calls[0][3]).toBeNull();
    const t = fakeTrack();
    pick(t);
    await flush();
    expect(h.setVideo).toHaveBeenLastCalledWith(t);
    expect(byText("button", "Trocar tela")).toBeDefined();
  });

  it("Encerrar para a track antes do session.stop()", async () => {
    const t = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(t));
    await clickCreate();
    await flush();
    byText("button", "Encerrar")!.onclick?.();
    await flush();
    expect(t.stop).toHaveBeenCalledTimes(1);
    expect(h.stop).toHaveBeenCalledTimes(1);
    expect(t.stop.mock.invocationCallOrder[0]).toBeLessThan(h.stop.mock.invocationCallOrder[0]);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("Encerrar com o seletor ainda aberto para a track que chegar depois", async () => {
    const pick = pendingCapture();
    await clickCreate();
    await flush();
    byText("button", "Encerrar")!.onclick?.();
    await flush();
    const setVideoCalls = h.setVideo.mock.calls.length;
    const t = fakeTrack();
    pick(t);
    await flush();
    expect(t.stop).toHaveBeenCalledTimes(1);
    expect(h.setVideo).toHaveBeenCalledTimes(setVideoCalls);
  });

  it("erro ao criar a sala para a track", async () => {
    const t = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(t));
    h.startHostNet.mockRejectedValue(new Error("Conexão com o servidor perdida"));
    await clickCreate();
    await flush();
    expect(title()).toBe("Não foi possível criar a sala");
    expect(t.stop).toHaveBeenCalledTimes(1);
  });

  it("Pausar e Retomar trocam a track enviada", async () => {
    const t = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(t));
    await clickCreate();
    await flush();
    byText("button", "Pausar vídeo")!.onclick?.();
    expect(h.setVideo).toHaveBeenLastCalledWith(null);
    expect(byText("div", "Vídeo pausado")).toBeDefined();
    byText("button", "Retomar vídeo")!.onclick?.();
    expect(h.setVideo).toHaveBeenLastCalledWith(t);
    expect(t.stop).not.toHaveBeenCalled();
  });

  it("Trocar tela abre o seletor no clique e para a track antiga", async () => {
    const a = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(a));
    await clickCreate();
    await flush();
    const b = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(b));
    byText("button", "Trocar tela")!.onclick?.();
    expect(getDisplayMedia).toHaveBeenCalledTimes(2);
    await flush();
    expect(a.stop).toHaveBeenCalledTimes(1);
    expect(h.setVideo).toHaveBeenLastCalledWith(b);
  });

  it("Parar de compartilhar no Chromium volta para Compartilhar tela", async () => {
    const t = fakeTrack();
    getDisplayMedia.mockResolvedValue(streamOf(t));
    await clickCreate();
    await flush();
    t.onended?.();
    expect(h.setVideo).toHaveBeenLastCalledWith(null);
    expect(byText("div", "Compartilhamento encerrado")).toBeDefined();
    expect(byText("button", "Compartilhar tela")).toBeDefined();
  });

  it("captura de uma janela mostra o aviso de desalinhamento", async () => {
    const t = fakeTrack();
    t.getSettings = () => ({ displaySurface: "window", width: 800, height: 600 });
    getDisplayMedia.mockResolvedValue(streamOf(t));
    await clickCreate();
    await flush();
    expect(
      byText(
        "div",
        "Você está compartilhando uma janela: os bichinhos vão ficar desalinhados. Compartilhe a tela inteira.",
      ),
    ).toBeDefined();
  });

  it("overlay dentro do vídeo avisa o Host", async () => {
    started = { aspect: 16 / 9, overlay_in_video: true };
    await clickCreate();
    await flush();
    expect(
      byText("div", "Seu Windows não esconde os bichinhos do vídeo: eles vão aparecer duplicados"),
    ).toBeDefined();
  });

  it("overlay fora do vídeo não avisa", async () => {
    await clickCreate();
    await flush();
    expect(
      byText("div", "Seu Windows não esconde os bichinhos do vídeo: eles vão aparecer duplicados"),
    ).toBeUndefined();
  });
});

describe("vídeo no viewer do convidado", () => {
  /** Entra guardando o `onVideo`; com `t`, a track chega antes de `startGuestNet` resolver. */
  let deliver: (t: unknown) => void = () => {};
  async function enterWithTrack(t?: FakeTrack): Promise<void> {
    h.startGuestNet.mockImplementation(
      async (_cfg: unknown, _code: string, onVideo: (t: unknown) => void) => {
        deliver = onVideo;
        if (t) onVideo(t);
        return { stop: h.stop };
      },
    );
    await launch();
    const code = codeInput();
    code.value = "PATO-4821";
    code.oninput?.();
    byText("button", "Entrar")!.onclick?.();
    await flush();
  }
  const videoEl = () => walk(app).find((e) => e.tag === "video");
  const srcTracks = () => (videoEl()!.srcObject as { getTracks(): unknown[] } | null)?.getTracks();

  it("track entregue antes do welcome termina no srcObject do <video>", async () => {
    const t = fakeTrack();
    await enterWithTrack(t);
    emit("session://welcome", { animal: "duck", aspect: 2 });
    const v = videoEl()!;
    expect(srcTracks()).toEqual([t]);
    expect([v.muted, v.autoplay, v.playsInline]).toEqual([true, true, true]);
  });

  it("track que chega com o viewer aberto também é ligada", async () => {
    await enterWithTrack();
    emit("session://welcome", { animal: "duck", aspect: 2 });
    expect(srcTracks()).toBeUndefined();
    const t = fakeTrack();
    deliver(t);
    expect(srcTracks()).toEqual([t]);
  });

  it("aguardando até tocar, pausado no mute, some no unmute", async () => {
    const t = fakeTrack();
    await enterWithTrack(t);
    emit("session://welcome", { animal: "duck", aspect: 2 });
    expect(videoStatus()!.hidden).toBe(false);
    expect(videoStatus()!.textContent).toBe(GUEST_WAITING_TEXT);
    // Mute antes de tocar (a track nasce muda) não troca o texto.
    t.fire("mute");
    expect(videoStatus()!.textContent).toBe(GUEST_WAITING_TEXT);
    videoEl()!.fire("playing");
    expect(videoStatus()!.hidden).toBe(true);
    t.fire("mute");
    expect(videoStatus()!.hidden).toBe(false);
    expect(videoStatus()!.textContent).toBe(GUEST_PAUSED_TEXT);
    t.fire("unmute");
    expect(videoStatus()!.hidden).toBe(true);
    t.fire("mute");
    videoEl()!.fire("playing");
    expect(videoStatus()!.hidden).toBe(true);
  });

  it("novo welcome reusa a mesma track e solta a anterior", async () => {
    const t = fakeTrack();
    await enterWithTrack(t);
    emit("session://welcome", { animal: "duck", aspect: 2 });
    const first = videoEl()!;
    emit("session://welcome", { animal: "duck", aspect: 2 });
    expect(first.srcObject).toBeNull();
    expect(videoEl()).not.toBe(first);
    expect(srcTracks()).toEqual([t]);
    expect(t.listenerCount()).toBe(2);
  });

  it("host-lost solta o srcObject e os listeners da track", async () => {
    const t = fakeTrack();
    await enterWithTrack(t);
    emit("session://welcome", { animal: "duck", aspect: 2 });
    const v = videoEl()!;
    emit("session://host-lost");
    expect(v.srcObject).toBeNull();
    expect(t.listenerCount()).toBe(0);
  });
});
