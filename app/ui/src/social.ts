// Tipos e constantes do chat/emote (espelho de `dm-protocol`). O TS só desenha e lista.

export type Emote = "heart" | "laugh" | "wow" | "angry" | "music" | "question";

/** Ordem das teclas 1–6. */
export const EMOTES: readonly Emote[] = ["heart", "laugh", "wow", "angry", "music", "question"];

export const EMOTE_ICON: Record<Emote, string> = {
  heart: "❤️",
  laugh: "😂",
  wow: "😮",
  angry: "😠",
  music: "🎵",
  question: "❓",
};

export const EMOTE_MS = 2500;
export const CHAT_MS = 5000;

export interface ChatMsg {
  from: number;
  text: string;
}

export function isEmote(x: unknown): x is Emote {
  return typeof x === "string" && (EMOTES as readonly string[]).includes(x);
}
