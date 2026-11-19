import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isChannelName, type ChannelName } from "./channels";
import { ChannelLimiter, once, tooBig } from "./guards";
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

export interface HostNetSession extends NetSession {
  /**
   * Troca a track enviada (`null` pausa) a quem o Rust liberou por `net://video` e vale para
   * quem for liberado depois. No-op depois de `stop()`. A ponte nunca para tracks: o dono é
   * quem capturou.
   */
  setVideo(track: MediaStreamTrack | null): void;
}

const HOST_PEER = 0;
const HOST_CONNECT_FAILED = "Não foi possível conectar ao Host";

interface SendPayload {
  peer: number;
  channel: string;
  bytes: number[];
}

/** O Rust liga (`on`) ou corta o vídeo de um peer; quem decide a admissão é ele. */
interface VideoPayload {
  peer: number;
  on: boolean;
}

/** Liga mensagens de um PeerLink ao Rust, descartando as grandes demais e as que `allow` recusar. */
function forwardRecv(
  link: PeerLink,
  peer: number,
  alive: () => boolean,
  allow: (channel: ChannelName) => boolean = () => true,
): void {
  link.onMessage((channel, bytes) => {
    if (!alive() || tooBig(bytes) || !allow(channel)) return;
    invoke("net_recv", { peer, channel, bytes: Array.from(bytes) }).catch(() => {});
  });
}

function notify(cmd: string, peer: number): void {
  invoke(cmd, { peer }).catch(() => {});
}

/** Espera uma mensagem de sinalização que `pick` aceite; erro do servidor rejeita. */
function waitFor<T>(sig: SignalingClient, pick: (m: ServerMsg) => T | undefined): Promise<T> {
  const p = new Promise<T>((resolve, reject) => {
    sig.onMessage((m) => {
      if (m.type === "error") return reject(new Error(signalErrorMessage(m.reason)));
      const v = pick(m);
      if (v !== undefined) resolve(v);
    });
    sig.onClose(() => reject(new Error("Conexão com o servidor perdida")));
  });
  return withTimeout(p, CONNECT_TIMEOUT_MS, SERVER_TIMEOUT);
}

const SERVER_TIMEOUT = "Não foi possível conectar ao servidor";

// O timeout (e o fechamento do socket tardio) vive em `SignalingClient.connect`.
async function connectSignaling(url: string, cancelled: () => boolean): Promise<SignalingClient> {
  const s = await SignalingClient.connect(url);
  if (cancelled()) {
    s.close(); // `stop()` chegou durante a conexão
    throw new Error("Cancelado");
  }
  return s;
}

export async function startHostNet(
  cfg: NetConfig,
  onCode: (code: string) => void,
  onSignalingLost?: () => void,
  video: MediaStreamTrack | null = null,
): Promise<HostNetSession> {
  let stopped = false;
  let currentVideo = video;
  const alive = () => !stopped;
  interface Peer {
    link: PeerLink;
    opened: boolean;
    /** Liberado pelo Rust (`net://video`): só então recebe a tela. */
    video: boolean;
  }
  const peers = new Map<number, Peer>();
  const limiter = new ChannelLimiter(() => performance.now());
  let sig: SignalingClient | null = null;
  const unlisten: (() => void)[] = [];

  // Limpa o peer; `net_peer_closed` só se `net_peer_opened` foi invocado (e só uma vez).
  const drop = (id: number, p: Peer) => {
    if (peers.get(id) !== p) return;
    peers.delete(id);
    limiter.forget(id);
    p.link.close();
    if (p.opened && !stopped) notify("net_peer_closed", id);
  };

  const stop = async () => {
    if (stopped) return;
    stopped = true;
    unlisten.forEach((u) => u());
    for (const p of peers.values()) p.link.close();
    peers.clear();
    sig?.close();
  };

  const addPeer = (s: SignalingClient, id: number) => {
    const old = peers.get(id);
    if (old) drop(id, old);
    const link = PeerLink.create(cfg.ice_servers, (data) => s.send({ type: "signal", to: id, data }), "host");
    // O transceiver nasce sem track: a tela só vai depois do `net://video` do Rust.
    const p: Peer = { link, opened: false, video: false };
    peers.set(id, p);
    forwardRecv(
      link,
      id,
      () => alive() && p.opened,
      (channel) => limiter.allow(id, channel),
    );
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
    // Os listeners vêm antes da conexão: nenhum evento se perde nem corre contra o `stop`.
    unlisten.push(
      await listen<SendPayload>("net://send", (e) => {
        const { peer, channel, bytes } = e.payload;
        if (!isChannelName(channel)) return;
        peers.get(peer)?.link.send(channel, Uint8Array.from(bytes));
      }),
      await listen<VideoPayload>("net://video", (e) => {
        if (stopped) return;
        const { peer, on } = e.payload;
        const p = peers.get(peer);
        if (!p) return;
        p.video = on === true;
        void p.link.setVideo(p.video ? currentVideo : null);
      }),
    );
    const s = await connectSignaling(cfg.signaling_url, () => stopped);
    sig = s;
    s.send({ type: "create_room" });
    const code = await waitFor(s, (m) => (m.type === "room_created" ? m.code : undefined));
    s.onMessage((m) => {
      if (stopped) return;
      if (m.type === "peer_joined") {
        try {
          addPeer(s, m.peer_id);
        } catch (e) {
          console.warn("Não foi possível iniciar a conexão com o convidado", e);
        }
      } else if (m.type === "peer_left") {
        const p = peers.get(m.peer_id);
        if (p) drop(m.peer_id, p);
      } else if (m.type === "signal") void peers.get(m.from)?.link.handleSignal(m.data);
    });
    s.onClose(() => {
      if (!stopped) onSignalingLost?.();
    });
    onCode(code);
  } catch (e) {
    await stop();
    throw e;
  }
  const setVideo = (track: MediaStreamTrack | null) => {
    if (stopped) return;
    currentVideo = track;
    for (const p of peers.values()) if (p.video) void p.link.setVideo(track);
  };
  return { stop, setVideo };
}

export async function startGuestNet(
  cfg: NetConfig,
  code: string,
  onVideo?: (track: MediaStreamTrack) => void,
): Promise<NetSession> {
  const norm = normalizeCode(code);
  if (!norm) throw new Error("Código inválido");
  let stopped = false;
  let opened = false;
  let link: PeerLink | null = null;
  let sig: SignalingClient | null = null;
  let unlisten: (() => void) | null = null;

  // Exatamente uma vez, venha de `host_left` (antes de abrir) ou do fechamento do canal.
  const closeHost = once(() => {
    link?.close();
    if (opened && !stopped) notify("net_peer_closed", HOST_PEER);
  });

  const stop = async () => {
    if (stopped) return;
    stopped = true;
    unlisten?.();
    link?.close();
    sig?.close();
  };

  try {
    unlisten = await listen<SendPayload>("net://send", (e) => {
      const { peer, channel, bytes } = e.payload;
      if (peer !== HOST_PEER || !isChannelName(channel)) return;
      link?.send(channel, Uint8Array.from(bytes));
    });
    const s = await connectSignaling(cfg.signaling_url, () => stopped);
    sig = s;
    s.send({ type: "join_room", code: norm });
    const hostId = await waitFor(s, (m) => (m.type === "joined" ? m.host_id : undefined));
    let l: PeerLink;
    try {
      l = PeerLink.create(cfg.ice_servers, (data) => s.send({ type: "signal", to: hostId, data }), "guest");
    } catch {
      throw new Error("Não foi possível iniciar a conexão com o Host");
    }
    link = l;
    forwardRecv(l, HOST_PEER, () => !stopped && opened);
    l.onClose(closeHost);
    if (onVideo) {
      // Pode chegar antes dos canais abrirem; nada sai depois do `stop()`.
      l.onVideo((t) => {
        if (!stopped) onVideo(t);
      });
    }

    // Falha antes de abrir (host_left, sinalização caída) também aborta a espera.
    let abort!: (e: Error) => void;
    const aborted = new Promise<never>((_, rej) => (abort = rej));
    aborted.catch(() => {});
    s.onMessage((m) => {
      if (stopped) return;
      if (m.type === "signal" && m.from === hostId) void l.handleSignal(m.data);
      else if (m.type === "host_left" && !opened) {
        // Depois de aberto o P2P sobrevive à sinalização: decidem o canal e o watchdog do Rust.
        abort(new Error("O Host saiu"));
        closeHost();
      }
    });
    s.onClose(() => {
      if (!opened) abort(new Error("Conexão com o servidor perdida"));
    });

    const linkOpened = l.opened.catch(() => {
      throw new Error(HOST_CONNECT_FAILED);
    });
    await withTimeout(Promise.race([linkOpened, aborted]), CONNECT_TIMEOUT_MS, HOST_CONNECT_FAILED);
    opened = true;
    notify("net_peer_opened", HOST_PEER);
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}
