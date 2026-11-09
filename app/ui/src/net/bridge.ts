import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isChannelName } from "./channels";
import { once, tooBig } from "./guards";
import { PeerLink } from "./rtc";
import { normalizeCode, signalErrorMessage, SignalingClient, type ServerMsg } from "./signaling";
import { CONNECT_TIMEOUT_MS, withTimeout } from "./timeout";

export interface NetConfig {
  signaling_url: string;
  ice_servers: RTCIceServer[];
}

export interface NetSession {
  /** Remove listeners, fecha os PeerLinks e a sinalização. Depois disso nada é invocado. */
  stop(): Promise<void>;
}

const HOST_PEER = 0;

interface SendPayload {
  peer: number;
  channel: string;
  bytes: number[];
}

/** Liga mensagens de um PeerLink ao Rust, descartando as grandes demais. */
function forwardRecv(link: PeerLink, peer: number, alive: () => boolean): void {
  link.onMessage((channel, bytes) => {
    if (!alive() || tooBig(bytes)) return;
    invoke("net_recv", { peer, channel, bytes: Array.from(bytes) }).catch(() => {});
  });
}

function notify(cmd: string, peer: number): void {
  invoke(cmd, { peer }).catch(() => {});
}

/** Espera uma mensagem de sinalização que `pick` aceite; erro do servidor rejeita. */
function waitFor<T>(sig: SignalingClient, pick: (m: ServerMsg) => T | undefined): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    sig.onMessage((m) => {
      if (m.type === "error") return reject(new Error(signalErrorMessage(m.reason)));
      const v = pick(m);
      if (v !== undefined) resolve(v);
    });
    sig.onClose(() => reject(new Error("Conexão com o servidor perdida")));
  });
}

export async function startHostNet(cfg: NetConfig, onCode: (code: string) => void): Promise<NetSession> {
  const sig = await SignalingClient.connect(cfg.signaling_url);
  let stopped = false;
  const alive = () => !stopped;
  interface Peer {
    link: PeerLink;
    opened: boolean;
  }
  const peers = new Map<number, Peer>();

  // Limpa o peer; `net_peer_closed` só se `net_peer_opened` foi invocado (e só uma vez).
  const drop = (id: number, p: Peer) => {
    if (peers.get(id) !== p) return;
    peers.delete(id);
    p.link.close();
    if (p.opened && !stopped) notify("net_peer_closed", id);
  };

  const unlisten = await listen<SendPayload>("net://send", (e) => {
    const { peer, channel, bytes } = e.payload;
    if (!isChannelName(channel)) return;
    peers.get(peer)?.link.send(channel, Uint8Array.from(bytes));
  });

  const stop = async () => {
    if (stopped) return;
    stopped = true;
    unlisten();
    for (const p of peers.values()) p.link.close();
    peers.clear();
    sig.close();
  };

  const addPeer = (id: number) => {
    const old = peers.get(id);
    if (old) drop(id, old);
    const link = PeerLink.create(cfg.ice_servers, (data) => sig.send({ type: "signal", to: id, data }));
    const p: Peer = { link, opened: false };
    peers.set(id, p);
    forwardRecv(link, id, () => alive() && p.opened);
    link.onClose(once(() => drop(id, p)));
    withTimeout(link.opened, CONNECT_TIMEOUT_MS, "Não foi possível conectar ao convidado")
      .then(() => {
        if (stopped || peers.get(id) !== p) return;
        p.opened = true;
        notify("net_peer_opened", id);
      })
      .catch(() => drop(id, p));
    link.makeOffer().catch(() => drop(id, p));
  };

  try {
    sig.send({ type: "create_room" });
    const code = await waitFor(sig, (m) => (m.type === "room_created" ? m.code : undefined));
    // waitFor deixou handlers de rejeição; os de rotina vêm agora.
    sig.onMessage((m) => {
      if (stopped) return;
      if (m.type === "peer_joined") addPeer(m.peer_id);
      else if (m.type === "peer_left") {
        const p = peers.get(m.peer_id);
        if (p) drop(m.peer_id, p);
      } else if (m.type === "signal") void peers.get(m.from)?.link.handleSignal(m.data);
    });
    onCode(code);
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}

export async function startGuestNet(cfg: NetConfig, code: string): Promise<NetSession> {
  const norm = normalizeCode(code);
  if (!norm) throw new Error("Código inválido");
  const sig = await SignalingClient.connect(cfg.signaling_url);
  let stopped = false;
  let opened = false;
  let link: PeerLink | null = null;

  // Exatamente uma vez, venha de `host_left` ou do fechamento do canal.
  const closeHost = once(() => {
    link?.close();
    if (opened && !stopped) notify("net_peer_closed", HOST_PEER);
  });

  const unlisten = await listen<SendPayload>("net://send", (e) => {
    const { peer, channel, bytes } = e.payload;
    if (peer !== HOST_PEER || !isChannelName(channel)) return;
    link?.send(channel, Uint8Array.from(bytes));
  });

  const stop = async () => {
    if (stopped) return;
    stopped = true;
    unlisten();
    link?.close();
    sig.close();
  };

  try {
    sig.send({ type: "join_room", code: norm });
    const hostId = await waitFor(sig, (m) => (m.type === "joined" ? m.host_id : undefined));
    const l = PeerLink.create(cfg.ice_servers, (data) => sig.send({ type: "signal", to: hostId, data }));
    link = l;
    forwardRecv(l, HOST_PEER, () => !stopped && opened);
    l.onClose(closeHost);

    // Falha antes de abrir (host_left, sinalização caída) também aborta a espera.
    let abort!: (e: Error) => void;
    const aborted = new Promise<never>((_, rej) => (abort = rej));
    sig.onMessage((m) => {
      if (stopped) return;
      if (m.type === "signal" && m.from === hostId) void l.handleSignal(m.data);
      else if (m.type === "host_left") {
        if (!opened) abort(new Error("O Host saiu"));
        closeHost();
      }
    });
    sig.onClose(() => {
      if (!opened) abort(new Error("Conexão com o servidor perdida"));
    });

    await withTimeout(Promise.race([l.opened, aborted]), CONNECT_TIMEOUT_MS, "Não foi possível conectar ao Host");
    if (stopped) throw new Error("Cancelado");
    opened = true;
    notify("net_peer_opened", HOST_PEER);
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}
