import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  startGuestNet,
  startHostNet,
  type HostNetSession,
  type NetConfig,
  type NetSession,
} from "../net/bridge";
import { normalizeCode } from "../net/signaling";
import { CONNECT_TIMEOUT_MS, deadline } from "../net/timeout";
import {
  CAPTURE_OPTIONS,
  captureWarning,
  GUEST_PAUSED_TEXT,
  GUEST_WAITING_TEXT,
  hostVideoText,
  type VideoState,
} from "../net/video";
import { startRenderer } from "../render/canvas";
import { fitRect } from "../render/fit";
import { LOOK } from "../render/placeholder";
import type { Animal, LastJoin, RosterEntry, Snapshot } from "../types";
import { HostVideo } from "./host-video";
import { ANIMAL_LABEL, lastCode, rejectMessage, rejoinChoice, type RejectReason } from "./labels";

const ANIMALS = Object.keys(ANIMAL_LABEL) as Animal[];
const VIEWER_W = 1280;
const VIEWER_H = 720;
const OVERLAY_IN_VIDEO =
  "Seu Windows não esconde os bichinhos do vídeo: eles vão aparecer duplicados";

/** Resposta do `host_start`. */
interface HostStarted {
  aspect: number;
  overlay_in_video: boolean;
}

/** Abre o seletor de tela; sem `navigator.mediaDevices` (WebKitGTK) vira recusa, nunca exceção. */
function captureScreen(): Promise<MediaStream> {
  const media = globalThis.navigator?.mediaDevices;
  if (!media?.getDisplayMedia) return Promise.reject(new Error("Captura de tela indisponível"));
  return media.getDisplayMedia(CAPTURE_OPTIONS);
}

/** Lembrança da última entrada, guardada pelo Rust; qualquer falha vira "sem lembrança". */
async function readLast(): Promise<LastJoin | null> {
  try {
    return (await invoke<LastJoin | null>("last_join")) ?? null;
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

/** Encerra a sessão (captura, depois stop, depois reset) e recarrega para o launcher. */
async function goBack(session: NetSession | null, video: HostVideo | null = null): Promise<void> {
  video?.dispose();
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
  void readLast().then((last) => {
    const saved = lastCode(last);
    if (saved && !code.value) {
      code.value = saved;
      join.disabled = false;
    }
  });

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
  let session: HostNetSession | null = null;
  // Última track entregue por `HostVideo`: vai no `startHostNet` e, depois dele, por `setVideo`.
  let sent: MediaStreamTrack | null = null;
  let state: VideoState = "none";
  let aspect = NaN;
  const status = el("div", { cls: "muted" });
  const controls = el("div", { cls: "row" });
  const captureWarn = el("div", { cls: "warn" });
  const video: HostVideo = new HostVideo(
    captureScreen,
    (t) => {
      sent = t;
      session?.setVideo(t);
    },
    (s) => {
      state = s;
      renderVideo();
    },
  );
  const share = el("button", { cls: "secondary" });
  // Síncrono no clique: o seletor exige o gesto do usuário.
  share.onclick = () => void video.share();
  const toggle = el("button", { cls: "secondary" });
  toggle.onclick = () => (state === "live" ? video.pause() : video.resume());
  const renderVideo = () => {
    status.textContent = hostVideoText(state);
    const has = state === "live" || state === "paused";
    toggle.textContent = state === "live" ? "Pausar vídeo" : "Retomar vídeo";
    share.textContent = has ? "Trocar tela" : "Compartilhar tela";
    controls.replaceChildren(...(has ? [toggle, share] : [share]));
    const settings = video.settings();
    captureWarn.textContent = (settings && captureWarning(settings, aspect)) ?? "";
  };

  // Antes de qualquer `await`, ainda dentro do clique em "Criar sala".
  void video.share();
  // F5 ou fechar a janela também apaga o indicador de compartilhamento.
  window.addEventListener("pagehide", () => video.dispose());

  show("center", el("h1", { text: "Criando sala..." }));
  const unlisten: UnlistenFn[] = [];
  try {
    const started = await invoke<HostStarted>("host_start", { name, animal });
    aspect = started.aspect;
    renderVideo();
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
          void goBack(session, video);
        };
        renderRoster();
        show(
          "",
          el("div", { cls: "code", text: c }),
          el("div", { cls: "row" }, copy, end),
          status,
          controls,
          captureWarn,
          started.overlay_in_video ? el("div", { cls: "warn", text: OVERLAY_IN_VIDEO }) : null,
          warn,
          list,
        );
      },
      () => {
        warn.textContent = "Conexão com o servidor perdida: novos amigos não conseguem entrar";
      },
      sent,
    );
    // O seletor pode ter respondido durante o `startHostNet`.
    session.setVideo(sent);
  } catch (e) {
    unlisten.forEach((u) => u());
    video.dispose();
    backScreen("Não foi possível criar a sala", errorText(e), session);
  }
}

async function guestFlow(name: string, code: string): Promise<void> {
  let session: NetSession | null = null;
  let cancelled = false;
  const unlisten: UnlistenFn[] = [];
  // Canal aberto mas o Host mudo: sem isso, "Conectando..." ficaria para sempre.
  const firstReply = deadline(CONNECT_TIMEOUT_MS, () => {
    cleanup();
    backScreen("Não foi possível entrar", "Não foi possível conectar ao Host", session);
  });
  const cleanup = () => {
    firstReply.settle();
    unlisten.forEach((u) => u());
  };
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
  // A track do Host pode chegar antes do `welcome`: guardada até o viewer abrir.
  let hostVideo: MediaStreamTrack | null = null;
  // Reconexão automática: uma única tentativa, sem passar pelos cartões (sala cheia também vale).
  let autoTried = false;
  let autoPending = false;
  // O nome do último `Hello`: na reconexão automática ele vem da lembrança, não do campo.
  let sentName = name;

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
              sentName = name;
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
        firstReply.settle();
        taken = e.payload.taken;
        if (inViewer) return;
        if (!autoTried) {
          // Marcado antes do await: um segundo lobby durante a leitura não tenta de novo.
          autoTried = true;
          autoPending = true;
          void readLast().then((last) => {
            // Cancelado, recusado ou já no viewer enquanto lia: nada a fazer.
            if (cancelled || !autoPending || inViewer) return;
            const again = rejoinChoice(last, code);
            if (!again) {
              autoPending = false;
              lobby();
              return;
            }
            sentName = again.name;
            invoke("guest_choose", again).catch(() => {
              autoPending = false;
              lobby();
            });
          });
          return;
        }
        if (!autoPending) lobby();
      }),
      await listen<{ animal: Animal; aspect: number }>("session://welcome", (e) => {
        firstReply.settle();
        inViewer = true;
        autoPending = false;
        invoke("remember_join", { code, name: sentName, animal: e.payload.animal }).catch(() => {});
        viewer?.dispose();
        invoke("enter_viewer").catch(() => {});
        viewer = showViewer(e.payload.aspect, leave);
        viewer.setRoster(roster);
        if (hostVideo) viewer.setVideo(hostVideo);
      }),
      await listen<{ reason: RejectReason }>("session://rejected", (e) => {
        firstReply.settle();
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
        hostVideo = null;
        cleanup();
        backScreen("O Host saiu", null, session);
      }),
    );
    session = await startGuestNet(cfg, code, (t) => {
      hostVideo = t;
      viewer?.setVideo(t);
    });
    if (cancelled) await session.stop();
    firstReply.arm();
  } catch (e) {
    hostVideo = null;
    if (cancelled) return; // a própria UI cancelou: a rejeição é esperada
    viewer?.dispose();
    cleanup();
    backScreen("Não foi possível entrar", errorText(e), session);
  }
}

interface ViewerHandle {
  setRoster(r: RosterEntry[]): void;
  /** Liga a track do Host ao `<video>`; a mesma track pode ser religada num viewer novo. */
  setVideo(t: MediaStreamTrack): void;
  dispose(): void;
}

/** Viewer do convidado: 1280x720 de referência, letterbox no `aspect` do Host. */
function showViewer(aspect: number, leave: () => void): ViewerHandle {
  const canvas = document.createElement("canvas");
  // `muted` + `playsinline` como atributo e propriedade: autoplay sem gesto do usuário.
  const video = el("video", { cls: "video" });
  for (const a of ["autoplay", "muted", "playsinline"]) video.setAttribute(a, "");
  video.autoplay = true;
  video.muted = true;
  video.playsInline = true;
  const status = el("div", { cls: "video-status", text: GUEST_WAITING_TEXT });
  const exit = el("button", { cls: "secondary", text: "Sair" });
  exit.style.cssText = "position:absolute;top:8px;right:8px;z-index:1";
  exit.onclick = leave;
  // Ordem de empilhamento: vídeo, aviso, bichinhos, botão.
  const stage = el("div", { cls: "stage" }, video, status, canvas, exit);
  show("viewer", stage);

  const box = () => {
    const w = stage.clientWidth || VIEWER_W;
    const h = stage.clientHeight || VIEWER_H;
    return fitRect(w, h, aspect > 0 && isFinite(aspect) ? aspect : 16 / 9);
  };
  // O vídeo (`object-fit: fill`) ocupa o mesmo retângulo dos bichinhos.
  const layout = () => {
    const r = box();
    for (const e of [video, status])
      Object.assign(e.style, {
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

  // Aguardando até o primeiro `playing`; depois disso, `mute` da track é o Host pausando.
  let track: MediaStreamTrack | null = null;
  let played = false;
  const showStatus = (text: string | null) => {
    status.hidden = text === null;
    if (text !== null) status.textContent = text;
  };
  const onPlaying = () => {
    played = true;
    showStatus(null);
  };
  const onMute = () => {
    if (played) showStatus(GUEST_PAUSED_TEXT);
  };
  const onUnmute = () => {
    if (played) showStatus(null);
  };
  video.addEventListener("playing", onPlaying);
  const detach = () => {
    track?.removeEventListener("mute", onMute);
    track?.removeEventListener("unmute", onUnmute);
    track = null;
  };

  return {
    setRoster: (r) => renderer.setRoster(r),
    setVideo: (t) => {
      if (disposed || t === track) return;
      detach();
      track = t;
      played = false;
      showStatus(GUEST_WAITING_TEXT);
      t.addEventListener("mute", onMute);
      t.addEventListener("unmute", onUnmute);
      video.srcObject = new MediaStream([t]);
    },
    dispose: () => {
      disposed = true;
      detach();
      video.removeEventListener("playing", onPlaying);
      video.srcObject = null;
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
