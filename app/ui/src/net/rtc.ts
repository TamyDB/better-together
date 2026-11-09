import { CHANNELS, isChannelName, type ChannelName } from "./channels";
import { IceQueue, once } from "./guards";

type Sig =
  | { kind: "sdp"; sdp: RTCSessionDescriptionInit }
  | { kind: "ice"; candidate: RTCIceCandidateInit };

/** `data` vem do outro peer: valida o formato, nunca confia. */
function parseSig(data: unknown): Sig | null {
  if (typeof data !== "object" || data === null) return null;
  const o = data as Record<string, unknown>;
  if (o.kind === "sdp" && typeof o.sdp === "object" && o.sdp !== null) {
    const s = o.sdp as Record<string, unknown>;
    if ((s.type === "offer" || s.type === "answer") && typeof s.sdp === "string") {
      return { kind: "sdp", sdp: { type: s.type, sdp: s.sdp } };
    }
  }
  if (o.kind === "ice" && typeof o.candidate === "object" && o.candidate !== null) {
    const c = o.candidate as Record<string, unknown>;
    if (typeof c.candidate === "string") {
      return {
        kind: "ice",
        candidate: {
          candidate: c.candidate,
          sdpMid: typeof c.sdpMid === "string" ? c.sdpMid : null,
          sdpMLineIndex: typeof c.sdpMLineIndex === "number" ? c.sdpMLineIndex : null,
        },
      };
    }
  }
  return null;
}

/** Uma conexão WebRTC com um peer, com os 3 DataChannels negotiated. */
export class PeerLink {
  readonly opened: Promise<void>;
  private readonly channels = new Map<ChannelName, RTCDataChannel>();
  private readonly ice: IceQueue<RTCIceCandidateInit>;
  private msgCb: (channel: ChannelName, bytes: Uint8Array) => void = () => {};
  private closeCbs: (() => void)[] = [];
  private closed = false;
  private readonly fireClose = once(() => {
    if (this.closed) return;
    for (const cb of this.closeCbs) cb();
  });

  private constructor(
    private readonly pc: RTCPeerConnection,
    private readonly sendSignal: (data: unknown) => void,
  ) {
    this.ice = new IceQueue((c) => pc.addIceCandidate(c));
    let openCount = 0;
    let resolveOpen!: () => void;
    let rejectOpen!: (e: Error) => void;
    this.opened = new Promise<void>((res, rej) => {
      resolveOpen = res;
      rejectOpen = rej;
    });
    this.opened.catch(() => {}); // evita rejeição não tratada se ninguém espera

    for (const name of Object.keys(CHANNELS) as ChannelName[]) {
      const ch = pc.createDataChannel(name, CHANNELS[name]);
      ch.binaryType = "arraybuffer";
      ch.onopen = () => {
        if (++openCount === 3) resolveOpen();
      };
      ch.onclose = () => {
        rejectOpen(new Error("Canal fechado"));
        this.fireClose();
      };
      ch.onmessage = (e) => {
        if (e.data instanceof ArrayBuffer) this.msgCb(name, new Uint8Array(e.data));
      };
      this.channels.set(name, ch);
    }

    pc.onicecandidate = (e) => {
      if (e.candidate && !this.closed) this.sendSignal({ kind: "ice", candidate: e.candidate.toJSON() });
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === "failed" || pc.connectionState === "closed") {
        rejectOpen(new Error("Conexão falhou"));
        this.fireClose();
      }
    };
  }

  static create(iceServers: RTCIceServer[], sendSignal: (data: unknown) => void): PeerLink {
    return new PeerLink(new RTCPeerConnection({ iceServers }), sendSignal);
  }

  async makeOffer(): Promise<void> {
    const offer = await this.pc.createOffer();
    await this.pc.setLocalDescription(offer);
    this.sendSignal({ kind: "sdp", sdp: { type: offer.type, sdp: offer.sdp } });
  }

  /** SDP ou candidato ICE vindo do outro peer. Dado malformado é ignorado. */
  async handleSignal(data: unknown): Promise<void> {
    const sig = parseSig(data);
    if (!sig || this.closed) return;
    try {
      if (sig.kind === "ice") {
        await this.ice.add(sig.candidate);
        return;
      }
      await this.pc.setRemoteDescription(sig.sdp);
      if (sig.sdp.type === "offer") {
        const answer = await this.pc.createAnswer();
        await this.pc.setLocalDescription(answer);
        this.sendSignal({ kind: "sdp", sdp: { type: answer.type, sdp: answer.sdp } });
      }
      await this.ice.remoteReady();
    } catch {
      // SDP inválido ou fora de estado: ignora, o timeout de conexão cuida do resto
    }
  }

  send(channel: ChannelName, bytes: Uint8Array): void {
    const ch = this.channels.get(channel);
    if (ch && ch.readyState === "open") ch.send(bytes as Uint8Array<ArrayBuffer>);
  }

  onMessage(cb: (channel: ChannelName, bytes: Uint8Array) => void): void {
    this.msgCb = (c, b) => {
      if (isChannelName(c) && !this.closed) cb(c, b);
    };
  }

  onClose(cb: () => void): void {
    this.closeCbs.push(cb);
  }

  /** Fecha sem disparar `onClose`: quem fecha já sabe. */
  close(): void {
    if (this.closed) return;
    this.closed = true;
    for (const ch of this.channels.values()) ch.close();
    this.pc.close();
  }
}
