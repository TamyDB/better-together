// Espelho de `crates/signaling/src/proto.rs` e `code.rs`.

export type SignalError =
  | "room_not_found"
  | "room_full"
  | "rate_limited"
  | "bad_message"
  | "already_in_room"
  | "bad_target"
  | "server_full";

export type ServerMsg =
  | { type: "room_created"; code: string; peer_id: number }
  | { type: "joined"; code: string; peer_id: number; host_id: number }
  | { type: "peer_joined"; peer_id: number }
  | { type: "peer_left"; peer_id: number }
  | { type: "signal"; from: number; data: unknown }
  | { type: "host_left" }
  | { type: "error"; reason: SignalError };

export type ClientMsg =
  | { type: "create_room" }
  | { type: "join_room"; code: string }
  | { type: "signal"; to: number; data: unknown };

const ERRORS: readonly string[] = [
  "room_not_found",
  "room_full",
  "rate_limited",
  "bad_message",
  "already_in_room",
  "bad_target",
  "server_full",
];

const MESSAGES: Record<SignalError, string> = {
  room_not_found: "Sala não encontrada",
  room_full: "Sala cheia",
  rate_limited: "Muitas tentativas, aguarde um pouco",
  bad_message: "Mensagem inválida",
  already_in_room: "Você já está em uma sala",
  bad_target: "Destinatário inválido",
  server_full: "Servidor cheio, tente mais tarde",
};

export function signalErrorMessage(reason: SignalError): string {
  return MESSAGES[reason];
}

const isId = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
const isStr = (v: unknown): v is string => typeof v === "string";

/** Valida `type` e os campos obrigatórios; qualquer outra coisa vira `null`. */
export function parseServerMsg(raw: string): ServerMsg | null {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  switch (o.type) {
    case "room_created":
      return isStr(o.code) && isId(o.peer_id) ? { type: "room_created", code: o.code, peer_id: o.peer_id } : null;
    case "joined":
      return isStr(o.code) && isId(o.peer_id) && isId(o.host_id)
        ? { type: "joined", code: o.code, peer_id: o.peer_id, host_id: o.host_id }
        : null;
    case "peer_joined":
      return isId(o.peer_id) ? { type: "peer_joined", peer_id: o.peer_id } : null;
    case "peer_left":
      return isId(o.peer_id) ? { type: "peer_left", peer_id: o.peer_id } : null;
    case "signal":
      return isId(o.from) && "data" in o ? { type: "signal", from: o.from, data: o.data } : null;
    case "host_left":
      return { type: "host_left" };
    case "error":
      return isStr(o.reason) && ERRORS.includes(o.reason) ? { type: "error", reason: o.reason as SignalError } : null;
    default:
      return null;
  }
}

/** Mesmas regras de `code::normalize`: só ASCII, entrada > 32 bytes é rejeitada. */
export function normalizeCode(input: string): string | null {
  if (new TextEncoder().encode(input).length > 32) return null;
  const s = input.replace(/\s/gu, "").replace(/[a-z]/g, (c) => c.toUpperCase());
  const dash = s.indexOf("-");
  const word = dash >= 0 ? s.slice(0, dash) : s.slice(0, 4);
  const digits = dash >= 0 ? s.slice(dash + 1) : s.slice(4);
  return /^[A-Z]{4}$/.test(word) && /^[0-9]{4}$/.test(digits) ? `${word}-${digits}` : null;
}

export class SignalingClient {
  private msgCbs: ((m: ServerMsg) => void)[] = [];
  private closeCbs: (() => void)[] = [];

  private constructor(private readonly ws: WebSocket) {
    ws.onmessage = (e) => {
      if (typeof e.data !== "string") return;
      const m = parseServerMsg(e.data);
      if (m) for (const cb of this.msgCbs) cb(m);
    };
    ws.onclose = () => {
      for (const cb of this.closeCbs) cb();
    };
  }

  static connect(url: string): Promise<SignalingClient> {
    return new Promise((resolve, reject) => {
      let ws: WebSocket;
      try {
        ws = new WebSocket(url);
      } catch {
        reject(new Error("Não foi possível conectar ao servidor"));
        return;
      }
      ws.onopen = () => resolve(new SignalingClient(ws));
      ws.onerror = () => reject(new Error("Não foi possível conectar ao servidor"));
    });
  }

  send(msg: ClientMsg): void {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg));
  }

  onMessage(cb: (m: ServerMsg) => void): void {
    this.msgCbs.push(cb);
  }

  onClose(cb: () => void): void {
    this.closeCbs.push(cb);
  }

  close(): void {
    this.msgCbs = [];
    this.closeCbs = [];
    this.ws.close();
  }
}
