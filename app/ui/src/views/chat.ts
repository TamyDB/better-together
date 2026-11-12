import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { EMOTE_ICON, EMOTES, isEmote, type ChatMsg, type Emote } from "../social";

export const LOG_SIZE = 50;
export const VISIBLE_LINES = 6;
export const CHAT_MAX_CHARS = 120;
const UNKNOWN_AUTHOR = "Jogador";

/** Histórico das últimas mensagens (as mais antigas caem). */
export class ChatLog {
  private msgs: ChatMsg[] = [];
  constructor(private readonly size = LOG_SIZE) {}
  push(m: ChatMsg): void {
    this.msgs.push(m);
    if (this.msgs.length > this.size) this.msgs.splice(0, this.msgs.length - this.size);
  }
  /** As `n` mais recentes, da mais antiga para a mais nova. */
  recent(n: number): ChatMsg[] {
    return n <= 0 ? [] : this.msgs.slice(-n);
  }
}

/** Payload de `session://chat`: `from` inteiro finito e `text` string; senão `null`. */
export function parseChat(p: unknown): ChatMsg | null {
  if (typeof p !== "object" || p === null) return null;
  const { from, text } = p as { from?: unknown; text?: unknown };
  if (typeof from !== "number" || !Number.isInteger(from) || typeof text !== "string") return null;
  return { from, text };
}

/** Payload de `session://emote`; inválido vira `null`. */
export function parseEmote(p: unknown): { from: number; emote: Emote } | null {
  if (typeof p !== "object" || p === null) return null;
  const { from, emote } = p as { from?: unknown; emote?: unknown };
  if (typeof from !== "number" || !Number.isInteger(from) || !isEmote(emote)) return null;
  return { from, emote };
}

/** Escuta chat e emote já validados; devolve o cancelamento dos dois listeners. */
export async function listenSocial(on: {
  chat: (m: ChatMsg) => void;
  emote: (m: { from: number; emote: Emote }) => void;
}): Promise<UnlistenFn> {
  const a = await listen<unknown>("session://chat", (e) => {
    const m = parseChat(e.payload);
    if (m) on.chat(m);
  });
  const b = await listen<unknown>("session://emote", (e) => {
    const m = parseEmote(e.payload);
    if (m) on.emote(m);
  });
  return () => {
    a();
    b();
  };
}

export interface ChatBox {
  el: HTMLElement;
  input: HTMLInputElement;
  push(m: ChatMsg): void;
  dispose(): void;
}

/**
 * Caixa de chat: histórico (só `textContent`) e campo. `Enter` envia o texto aparado (vazio só
 * fecha) e `Esc` limpa; os dois chamam `onDone` para quem hospeda devolver o foco.
 */
export function chatBox(opts: {
  names: () => Map<number, string>;
  onSend: (text: string) => void;
  visible: number;
  onDone?: () => void;
}): ChatBox {
  const log = new ChatLog();
  const list = document.createElement("div");
  list.className = "chat-log";
  const input = document.createElement("input");
  input.className = "chat-input";
  input.maxLength = CHAT_MAX_CHARS;
  input.placeholder = "Enter para conversar";
  const el = document.createElement("div");
  el.className = "chat-box";
  el.appendChild(list);
  el.appendChild(input);

  const render = () => {
    const names = opts.names();
    list.replaceChildren(
      ...log.recent(opts.visible).map((m) => {
        const line = document.createElement("div");
        line.textContent = `${names.get(m.from) ?? UNKNOWN_AUTHOR}: ${m.text}`;
        return line;
      }),
    );
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      const text = input.value.trim();
      input.value = "";
      if (text) opts.onSend(text);
      opts.onDone?.();
    } else if (e.key === "Escape") {
      input.value = "";
      opts.onDone?.();
    }
  };
  input.addEventListener("keydown", onKey);
  return {
    el,
    input,
    push: (m) => {
      log.push(m);
      render();
    },
    dispose: () => input.removeEventListener("keydown", onKey),
  };
}

/** Seis botões de emote, na ordem das teclas 1–6. */
export function emoteBar(onEmote: (e: Emote) => void): HTMLElement {
  const bar = document.createElement("div");
  bar.className = "emote-bar";
  EMOTES.forEach((emote, i) => {
    const b = document.createElement("button");
    b.className = "secondary emote";
    b.textContent = EMOTE_ICON[emote];
    b.title = `Tecla ${i + 1}`;
    b.onclick = () => onEmote(emote);
    bar.appendChild(b);
  });
  return bar;
}
