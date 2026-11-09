import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { startGuestNet, startHostNet, type NetConfig, type NetSession } from "../net/bridge";
import { normalizeCode } from "../net/signaling";
import { startRenderer } from "../render/canvas";
import { fitRect } from "../render/fit";
import { LOOK } from "../render/placeholder";
import type { Animal, RosterEntry, Snapshot } from "../types";
import { ANIMAL_LABEL, rejectMessage, rejoinChoice, type RejectReason } from "./labels";

const ANIMALS = Object.keys(ANIMAL_LABEL) as Animal[];
const VIEWER_W = 1280;
const VIEWER_H = 720;
/** Última entrada bem-sucedida; sobrevive ao reload do "Voltar" e morre com o app, como o token. */
const LAST_KEY = "bt.last";

function readLast(): unknown {
  try {
    return JSON.parse(sessionStorage.getItem(LAST_KEY) ?? "null");
  } catch {
    return null;
  }
}

/** Cria um elemento; todo texto entra por `textContent` (nomes vêm de pessoas não confiáveis). */
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  opts: { cls?: string; text?: string } = {},
  ...children: (Node | null)[]
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (opts.cls) e.className = opts.cls;
  if (opts.text !== undefined) e.textContent = opts.text;
  for (const c of children) if (c) e.appendChild(c);
  return e;
}

function petDot(animal: Animal): HTMLElement {
  const d = el("div", { cls: "pet", text: LOOK[animal].letter });
  d.style.background = LOOK[animal].color;
  return d;
}

function animalCard(animal: Animal, onPick: () => void, disabled = false): HTMLButtonElement {
  const b = el("button", { cls: "card" }, petDot(animal), el("span", { text: ANIMAL_LABEL[animal] }));
  b.disabled = disabled;
  b.onclick = onPick;
  return b;
}

const root = (): HTMLElement => document.getElementById("app")!;

function show(cls: string, ...children: (Node | null)[]): HTMLElement {
  const app = root();
  app.className = cls;
  app.replaceChildren(...(children.filter(Boolean) as Node[]));
  return app;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Encerra a sessão (stop antes do reset) e recarrega para o launcher. */
async function goBack(session: NetSession | null): Promise<void> {
  try {
    await session?.stop();
  } catch {
    /* já parada */
  }
  try {
    await invoke("session_reset");
  } catch {
    // Pode estar "ainda iniciando": uma segunda tentativa após curta espera.
    await sleep(400);
    await invoke("session_reset").catch(() => {});
  }
  location.reload();
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function backScreen(title: string, detail: string | null, session: NetSession | null, cls = "error"): void {
  const back = el("button", { text: "Voltar" });
  back.onclick = () => {
    back.disabled = true;
    void goBack(session);
  };
  show("center", el("h1", { text: title }), detail ? el("div", { cls, text: detail }) : null, back);
}

export function startMainWindow(): void {
  const app = document.createElement("div");
  app.id = "app";
  document.body.appendChild(app);
  // F5/crash do WebView pode deixar o modo antigo vivo no Rust: zera antes de começar.
  invoke("session_reset")
    .catch(() => {})
    .then(launcher);
}

function launcher(): void {
  const name = el("input");
  name.placeholder = "Seu nome";
  name.maxLength = 16;
  let animal: Animal | null = null;
  const create = el("button", { text: "Criar sala" });
  create.disabled = true;
  const cards = ANIMALS.map((a) =>
    animalCard(a, () => {
      animal = a;
      cards.forEach((c, i) => c.classList.toggle("selected", ANIMALS[i] === a));
      create.disabled = false;
    }),
  );
  const code = el("input");
  code.placeholder = "Código da sala";
  const join = el("button", { text: "Entrar" });
  join.disabled = true;
  code.oninput = () => (join.disabled = normalizeCode(code.value) === null);

  const nick = () => name.value.trim() || "Jogador";
  create.onclick = () => animal && void hostFlow(nick(), animal);
  join.onclick = () => {
    const c = normalizeCode(code.value);
    if (c) void guestFlow(nick(), c);
  };
  code.onkeydown = (e) => {
    if (e.key === "Enter" && !join.disabled) join.click();
  };

  show(
    "",
    el("h1", { text: "Better Together" }),
    name,
    el("h2", { text: "Escolha seu bichinho e crie uma sala" }),
    el("div", { cls: "cards" }, ...cards),
    create,
    el("h2", { text: "Ou entre na sala de um amigo" }),
    el("div", { cls: "row" }, code, join),
  );
}

async function hostFlow(name: string, animal: Animal): Promise<void> {
  show("center", el("h1", { text: "Criando sala..." }));
  let session: NetSession | null = null;
  const unlisten: UnlistenFn[] = [];
  try {
    await invoke("host_start", { name, animal });
    const cfg = await invoke<NetConfig>("net_config");

    // O roster só sai quando alguém entra/sai: até lá, mostra o próprio Host.
    let roster: RosterEntry[] = [{ id: 0, name, animal, asleep: false }];
    const list = el("ul");
    const warn = el("div", { cls: "warn" });
    const renderRoster = () =>
      list.replaceChildren(
        ...roster.map((r) =>
          el(
            "li",
            {},
            petDot(r.animal),
            el("span", { text: r.name }),
            r.asleep ? el("span", { cls: "muted", text: "dormindo" }) : null,
          ),
        ),
      );
    unlisten.push(
      await listen<RosterEntry[]>("session://roster", (e) => {
        roster = e.payload;
        renderRoster();
      }),
    );

    session = await startHostNet(
      cfg,
      (c) => {
        const copy = el("button", { cls: "secondary", text: "Copiar" });
        copy.onclick = () => {
          navigator.clipboard.writeText(c).then(
            () => (copy.textContent = "Copiado!"),
            () => {},
          );
          setTimeout(() => (copy.textContent = "Copiar"), 1500);
        };
        const end = el("button", { cls: "secondary", text: "Encerrar" });
        end.onclick = () => {
          end.disabled = true;
          unlisten.forEach((u) => u());
          void goBack(session);
        };
        renderRoster();
        show("", el("div", { cls: "code", text: c }), el("div", { cls: "row" }, copy, end), warn, list);
      },
      () => {
        warn.textContent = "Conexão com o servidor perdida: novos amigos não conseguem entrar";
      },
    );
  } catch (e) {
    unlisten.forEach((u) => u());
    backScreen("Não foi possível criar a sala", errorText(e), session);
  }
}

async function guestFlow(name: string, code: string): Promise<void> {
  let session: NetSession | null = null;
  let cancelled = false;
  const unlisten: UnlistenFn[] = [];
  const cleanup = () => unlisten.forEach((u) => u());
  const leave = () => {
    cancelled = true;
    cleanup();
    void goBack(session);
  };

  const connecting = el("button", { cls: "secondary", text: "Cancelar" });
  connecting.onclick = leave;
  show("center", el("h1", { text: "Conectando..." }), connecting);

  let taken: Animal[] = [];
  let message: string | null = null;
  let inViewer = false;
  let roster: RosterEntry[] = [];
  let viewer = null as ViewerHandle | null;
  // Reconexão automática: uma única tentativa, sem passar pelos cartões (sala cheia também vale).
  let autoTried = false;
  let autoPending = false;

  const lobby = () => {
    inViewer = false;
    const back = el("button", { cls: "secondary", text: "Sair" });
    back.onclick = leave;
    const shown = message ?? (taken.length >= ANIMALS.length ? "Sala cheia" : null);
    show(
      "",
      el("h1", { text: "Escolha seu bichinho" }),
      shown ? el("div", { cls: "error", text: shown }) : null,
      el(
        "div",
        { cls: "cards" },
        ...ANIMALS.map((a) =>
          animalCard(
            a,
            () => {
              message = null;
              root().querySelector(".error")?.remove();
              invoke("guest_choose", { name, animal: a }).catch((e) => {
                message = errorText(e);
                lobby();
              });
            },
            taken.includes(a),
          ),
        ),
      ),
      back,
    );
  };

  try {
    await invoke("guest_start");
    const cfg = await invoke<NetConfig>("net_config");
    unlisten.push(
      await listen<{ taken: Animal[] }>("session://lobby", (e) => {
        taken = e.payload.taken;
        if (inViewer) return;
        if (!autoTried) {
          autoTried = true;
          const again = rejoinChoice(readLast(), code);
          if (again) {
            autoPending = true;
            invoke("guest_choose", again).catch(() => {
              autoPending = false;
              lobby();
            });
            return;
          }
        }
        if (!autoPending) lobby();
      }),
      await listen<{ animal: Animal; aspect: number }>("session://welcome", (e) => {
        inViewer = true;
        autoPending = false;
        sessionStorage.setItem(LAST_KEY, JSON.stringify({ code, name, animal: e.payload.animal }));
        viewer?.dispose();
        invoke("enter_viewer").catch(() => {});
        viewer = showViewer(e.payload.aspect, leave);
        viewer.setRoster(roster);
      }),
      await listen<{ reason: RejectReason }>("session://rejected", (e) => {
        // Rejeição da tentativa automática: lobby normal, sem erro.
        message = autoPending ? null : rejectMessage(e.payload.reason);
        autoPending = false;
        lobby();
      }),
      await listen<RosterEntry[]>("session://roster", (e) => {
        roster = e.payload;
        viewer?.setRoster(roster);
      }),
      await listen("session://host-lost", () => {
        viewer?.dispose();
        viewer = null;
        cleanup();
        backScreen("O Host saiu", null, session);
      }),
    );
    session = await startGuestNet(cfg, code);
    if (cancelled) await session.stop();
  } catch (e) {
    if (cancelled) return; // a própria UI cancelou: a rejeição é esperada
    viewer?.dispose();
    cleanup();
    backScreen("Não foi possível entrar", errorText(e), session);
  }
}

interface ViewerHandle {
  setRoster(r: RosterEntry[]): void;
  dispose(): void;
}

/** Viewer do convidado: 1280x720 de referência, letterbox no `aspect` do Host. */
function showViewer(aspect: number, leave: () => void): ViewerHandle {
  const canvas = document.createElement("canvas");
  const video = el("div", { cls: "video", text: "O vídeo do Host chega no M3" });
  const exit = el("button", { cls: "secondary", text: "Sair" });
  exit.style.cssText = "position:absolute;top:8px;right:8px;z-index:1";
  exit.onclick = leave;
  const stage = el("div", { cls: "stage" }, video, canvas, exit);
  show("viewer", stage);

  const box = () => {
    const w = stage.clientWidth || VIEWER_W;
    const h = stage.clientHeight || VIEWER_H;
    return fitRect(w, h, aspect > 0 && isFinite(aspect) ? aspect : 16 / 9);
  };
  const layout = () => {
    const r = box();
    Object.assign(video.style, {
      left: `${r.x}px`,
      top: `${r.y}px`,
      width: `${r.w}px`,
      height: `${r.h}px`,
    });
  };
  window.addEventListener("resize", layout);
  layout();

  const renderer = startRenderer(canvas, { area: box });
  let unlisten: UnlistenFn | null = null;
  let disposed = false;
  void listen<Snapshot>("sim://snapshot", (e) => renderer.update(e.payload)).then((u) =>
    disposed ? u() : (unlisten = u),
  );

  // Só aqui o teclado vai para o Rust; `blur` solta o que estiver pressionado.
  const pressed = new Set<string>();
  const send = (code: string, down: boolean) => invoke("key_event", { code, down }).catch(() => {});
  const onDown = (e: KeyboardEvent) => {
    pressed.add(e.code);
    send(e.code, true);
  };
  const onUp = (e: KeyboardEvent) => {
    pressed.delete(e.code);
    send(e.code, false);
  };
  const onBlur = () => {
    for (const c of pressed) send(c, false);
    pressed.clear();
  };
  window.addEventListener("keydown", onDown);
  window.addEventListener("keyup", onUp);
  window.addEventListener("blur", onBlur);

  return {
    setRoster: (r) => renderer.setRoster(r),
    dispose: () => {
      disposed = true;
      renderer.stop();
      unlisten?.();
      window.removeEventListener("resize", layout);
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("blur", onBlur);
      onBlur();
    },
  };
}
