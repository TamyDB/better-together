import { CHAT_MS, EMOTE_MS, type Emote } from "../social";

interface Timed<T> {
  value: T;
  until: number;
}

/** Balões ativos por jogador. O relógio (ms) é injetado para testar sem esperar. */
export class BubbleBoard {
  private chats = new Map<number, Timed<string>>();
  private emotes = new Map<number, Timed<Emote>>();

  constructor(private now: () => number) {}

  chat(from: number, text: string): void {
    this.chats.set(from, { value: text, until: this.now() + CHAT_MS });
  }

  emote(from: number, emote: Emote): void {
    this.emotes.set(from, { value: emote, until: this.now() + EMOTE_MS });
  }

  active(from: number): { chat?: string; emote?: Emote } {
    const t = this.now();
    const out: { chat?: string; emote?: Emote } = {};
    const c = this.chats.get(from);
    if (c && t < c.until) out.chat = c.value;
    const e = this.emotes.get(from);
    if (e && t < e.until) out.emote = e.value;
    return out;
  }

  /** Descarta os vencidos. */
  prune(): void {
    const t = this.now();
    for (const m of [this.chats, this.emotes] as Map<number, Timed<unknown>>[]) {
      for (const [k, v] of m) if (t >= v.until) m.delete(k);
    }
  }
}

/** Quebra `text` em até `maxLines` linhas de `width` chars: por palavra, cortando palavras
 *  maiores que a linha; se sobrar texto, a última linha termina em "…". */
export function wrapText(text: string, width: number, maxLines: number): string[] {
  const lines: string[] = [];
  let cur = "";
  for (const word of text.split(/\s+/).filter(Boolean)) {
    let w = word;
    while (w.length > width) {
      if (cur) {
        lines.push(cur);
        cur = "";
      }
      lines.push(w.slice(0, width));
      w = w.slice(width);
    }
    if (!cur) cur = w;
    else if (cur.length + 1 + w.length <= width) cur += " " + w;
    else {
      lines.push(cur);
      cur = w;
    }
  }
  if (cur) lines.push(cur);
  if (lines.length <= maxLines) return lines;
  const kept = lines.slice(0, maxLines);
  kept[maxLines - 1] = kept[maxLines - 1].slice(0, width - 1) + "…";
  return kept;
}
