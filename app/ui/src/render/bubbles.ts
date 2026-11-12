import { CHAT_MS, EMOTE_MS, type Emote } from "../social";

interface Timed<T> {
  value: T;
  until: number;
}

/** Balões ativos por jogador. O relógio (ms) é injetado para testar sem esperar. */
export class BubbleBoard {
  private chats = new Map<number, Timed<{ text: string; lines: string[] }>>();
  private emotes = new Map<number, Timed<Emote>>();

  constructor(private now: () => number) {}

  chat(from: number, text: string): void {
    this.chats.set(from, { value: { text, lines: wrapText(text, 28, 3) }, until: this.now() + CHAT_MS });
  }

  emote(from: number, emote: Emote): void {
    this.emotes.set(from, { value: emote, until: this.now() + EMOTE_MS });
  }

  active(from: number): { chat?: string; chatLines?: string[]; emote?: Emote } {
    const t = this.now();
    const out: { chat?: string; chatLines?: string[]; emote?: Emote } = {};
    const c = this.chats.get(from);
    if (c && t < c.until) {
      out.chat = c.value.text;
      out.chatLines = c.value.lines;
    }
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

const segmenter =
  typeof Intl !== "undefined" && "Segmenter" in Intl
    ? new Intl.Segmenter("pt", { granularity: "grapheme" })
    : null;

/** Grafemas simples (emoji com VS16/ZWJ ficam inteiros); sem `Intl.Segmenter`, code points. */
function graphemes(s: string): string[] {
  return segmenter ? Array.from(segmenter.segment(s), (x) => x.segment) : Array.from(s);
}

/** Quebra `text` em até `maxLines` linhas de `width` grafemas: por palavra, cortando palavras
 *  maiores que a linha; se sobrar texto, a última linha termina em "…". */
export function wrapText(text: string, width: number, maxLines: number): string[] {
  width = Math.max(1, Math.floor(width));
  const lines: string[] = [];
  let cur: string[] = [];
  for (const word of text.split(/\s+/).filter(Boolean)) {
    let w = graphemes(word);
    while (w.length > width) {
      if (cur.length) {
        lines.push(cur.join(""));
        cur = [];
      }
      lines.push(w.slice(0, width).join(""));
      w = w.slice(width);
    }
    if (!cur.length) cur = w;
    else if (cur.length + 1 + w.length <= width) cur = [...cur, " ", ...w];
    else {
      lines.push(cur.join(""));
      cur = w;
    }
  }
  if (cur.length) lines.push(cur.join(""));
  if (lines.length <= maxLines) return lines;
  const kept = lines.slice(0, maxLines);
  kept[maxLines - 1] = graphemes(kept[maxLines - 1]).slice(0, width - 1).join("") + "…";
  return kept;
}
