import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  invoke: vi.fn(async (..._a: unknown[]) => undefined),
  unlisten: vi.fn(),
  sig: null as any,
  links: [] as any[],
  createFails: false,
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: h.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => h.unlisten) }));
vi.mock("./signaling", async (orig) => {
  const real = await orig<typeof import("./signaling")>();
  class FakeSig {
    sent: any[] = [];
    msgCbs: ((m: any) => void)[] = [];
    closeCbs: (() => void)[] = [];
    closed = false;
    static async connect() {
      h.sig = new FakeSig();
      return h.sig;
    }
    send(m: any) {
      this.sent.push(m);
    }
    onMessage(cb: any) {
      this.msgCbs.push(cb);
    }
    onClose(cb: any) {
      this.closeCbs.push(cb);
    }
    close() {
      this.closed = true;
    }
    emit(m: any) {
      for (const cb of this.msgCbs) cb(m);
    }
    drop() {
      for (const cb of this.closeCbs) cb();
    }
  }
  return { ...real, SignalingClient: FakeSig };
});
vi.mock("./rtc", () => {
  class FakeLink {
    closeCbs: (() => void)[] = [];
    msgCb: any = () => {};
    signals: unknown[] = [];
    resolve!: () => void;
    reject!: (e: Error) => void;
    opened = new Promise<void>((res, rej) => {
      this.resolve = res;
      this.reject = rej;
    });
    closed = false;
    videoCalls: unknown[] = [];
    videoCbs: ((t: unknown) => void)[] = [];
    constructor() {
      this.opened.catch(() => {});
    }
    static create(_ice?: unknown, _send?: unknown, _role?: unknown) {
      if (h.createFails) throw new Error("RTCPeerConnection indisponível");
      const l = new FakeLink();
      h.links.push(l);
      return l;
    }
    async makeOffer() {}
    async handleSignal(d: unknown) {
      this.signals.push(d);
    }
    send() {}
    onMessage(cb: any) {
      this.msgCb = cb;
    }
    onClose(cb: any) {
      this.closeCbs.push(cb);
    }
    close() {
      this.closed = true;
    }
    fireClose() {
      for (const cb of this.closeCbs) cb();
    }
    async setVideo(t: unknown) {
      this.videoCalls.push(t);
    }
    onVideo(cb: (t: unknown) => void) {
      this.videoCbs.push(cb);
    }
    emitVideo(t: unknown) {
      for (const cb of this.videoCbs) cb(t);
    }
  }
  return { PeerLink: FakeLink };
});

import { startGuestNet, startHostNet } from "./bridge";

const cfg = { signaling_url: "ws://x", ice_servers: [] };
const flush = () => new Promise((r) => setTimeout(r, 0));
const calls = (name: string) => h.invoke.mock.calls.filter((c) => c[0] === name);

beforeEach(() => {
  h.invoke.mockClear();
  h.unlisten.mockClear();
  h.links.length = 0;
  h.createFails = false;
});

afterEach(() => {
  vi.restoreAllMocks();
});

async function host() {
  const p = startHostNet(cfg, () => {});
  await flush();
  h.sig.emit({ type: "room_created", code: "PATO-4821", peer_id: 1 });
  return { session: await p, sig: h.sig };
}

async function guest(onVideo?: (t: MediaStreamTrack) => void) {
  const p = startGuestNet(cfg, "pato4821", onVideo);
  await flush();
  h.sig.emit({ type: "joined", code: "PATO-4821", peer_id: 2, host_id: 1 });
  await flush();
  return { p, sig: h.sig, link: h.links[0] };
}

describe("Host", () => {
  it("net_peer_closed uma vez só (peer_left + fechamento do canal)", async () => {
    const { sig } = await host();
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    link.resolve();
    await flush();
    expect(calls("net_peer_opened")).toHaveLength(1);
    sig.emit({ type: "peer_left", peer_id: 5 });
    link.fireClose();
    await flush();
    expect(calls("net_peer_closed")).toHaveLength(1);
  });

  it("peer que falha antes de opened não gera net_peer_closed", async () => {
    const { sig } = await host();
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    link.reject(new Error("falhou"));
    link.fireClose();
    await flush();
    expect(link.closed).toBe(true);
    expect(calls("net_peer_opened")).toHaveLength(0);
    expect(calls("net_peer_closed")).toHaveLength(0);
  });

  it("descarta mensagem > 16384 bytes e repassa a menor", async () => {
    const { sig } = await host();
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    link.resolve();
    await flush();
    link.msgCb("events", new Uint8Array(16385));
    link.msgCb("events", new Uint8Array(3));
    expect(calls("net_recv")).toHaveLength(1);
  });

  it("nada é invocado depois de stop()", async () => {
    const { session, sig } = await host();
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    link.resolve();
    await flush();
    h.invoke.mockClear();
    await session.stop();
    expect(h.unlisten).toHaveBeenCalled();
    expect(sig.closed).toBe(true);
    link.msgCb("events", new Uint8Array(3));
    link.fireClose();
    sig.emit({ type: "peer_left", peer_id: 5 });
    await flush();
    expect(h.invoke).not.toHaveBeenCalled();
  });

  it("falha ao criar o PeerLink registra console.warn", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { sig } = await host();
    h.createFails = true;
    sig.emit({ type: "peer_joined", peer_id: 5 });
    expect(warn).toHaveBeenCalledTimes(1);
    expect(h.links).toHaveLength(0);
    expect(calls("net_peer_opened")).toHaveLength(0);
    // O próximo convidado segue normal.
    h.createFails = false;
    sig.emit({ type: "peer_joined", peer_id: 6 });
    expect(h.links).toHaveLength(1);
  });

  describe("limite por canal", () => {
    async function opened(...ids: number[]) {
      const { sig } = await host();
      for (const id of ids) sig.emit({ type: "peer_joined", peer_id: id });
      const links = [...h.links];
      for (const l of links) l.resolve();
      await flush();
      return { sig, links };
    }
    const send = (link: any, channel: string, n: number) => {
      for (let i = 0; i < n; i++) link.msgCb(channel, new Uint8Array(1));
    };
    const recv = (channel?: string) =>
      calls("net_recv").filter((c) => channel === undefined || (c[1] as any).channel === channel);

    it("descarta o excesso de input e events antes do invoke", async () => {
      const now = vi.spyOn(performance, "now").mockReturnValue(1000);
      const { links } = await opened(5);
      send(links[0], "events", 40);
      send(links[0], "input", 300);
      expect(recv("events")).toHaveLength(32);
      expect(recv("input")).toHaveLength(240);
      now.mockReturnValue(2000);
      send(links[0], "events", 40);
      expect(recv("events")).toHaveLength(64);
    });

    it("state vindo de convidado nunca chega ao Rust", async () => {
      vi.spyOn(performance, "now").mockReturnValue(1000);
      const { links } = await opened(5);
      send(links[0], "state", 5);
      expect(recv()).toHaveLength(0);
    });

    it("peer que sai e volta com o mesmo id recomeça o limite", async () => {
      vi.spyOn(performance, "now").mockReturnValue(1000);
      const { sig, links } = await opened(5);
      send(links[0], "events", 40);
      expect(recv("events")).toHaveLength(32);
      sig.emit({ type: "peer_left", peer_id: 5 });
      sig.emit({ type: "peer_joined", peer_id: 5 });
      h.links[1].resolve();
      await flush();
      send(h.links[1], "events", 40);
      expect(recv("events")).toHaveLength(64);
    });

    it("o limite de um peer não afeta o outro", async () => {
      vi.spyOn(performance, "now").mockReturnValue(1000);
      const { links } = await opened(5, 6);
      send(links[0], "events", 40);
      send(links[1], "events", 2);
      expect(recv("events")).toHaveLength(34);
    });
  });

  it("onSignalingLost só sem stop()", async () => {
    const lost = vi.fn();
    const p = startHostNet(cfg, () => {}, lost);
    await flush();
    h.sig.emit({ type: "room_created", code: "PATO-4821", peer_id: 1 });
    const session = await p;
    h.sig.drop();
    expect(lost).toHaveBeenCalledTimes(1);
    await session.stop();
    h.sig.drop();
    expect(lost).toHaveBeenCalledTimes(1);
  });
});

describe("Host (vídeo)", () => {
  const trackA = { kind: "video", id: "A" } as unknown as MediaStreamTrack;
  const trackB = { kind: "video", id: "B" } as unknown as MediaStreamTrack;

  async function hostWith(video: MediaStreamTrack | null) {
    const p = startHostNet(cfg, () => {}, undefined, video);
    await flush();
    h.sig.emit({ type: "room_created", code: "PATO-4821", peer_id: 1 });
    return { session: await p, sig: h.sig };
  }

  it("a track atual vale para peers vivos e novos; quem saiu não recebe mais", async () => {
    const { session, sig } = await hostWith(trackA);
    sig.emit({ type: "peer_joined", peer_id: 5 });
    sig.emit({ type: "peer_joined", peer_id: 6 });
    const [l5, l6] = h.links;
    expect(l5.videoCalls).toEqual([trackA]);
    expect(l6.videoCalls).toEqual([trackA]);

    session.setVideo(trackB);
    expect(l5.videoCalls).toEqual([trackA, trackB]);
    expect(l6.videoCalls).toEqual([trackA, trackB]);

    sig.emit({ type: "peer_joined", peer_id: 7 });
    expect(h.links[2].videoCalls).toEqual([trackB]);

    sig.emit({ type: "peer_left", peer_id: 5 });
    session.setVideo(null);
    expect(l5.videoCalls).toEqual([trackA, trackB]);
    expect(l6.videoCalls).toEqual([trackA, trackB, null]);
    expect(h.links[2].videoCalls).toEqual([trackB, null]);
  });

  it("peer que fecha sozinho não recebe mais setVideo", async () => {
    const { session, sig } = await hostWith(trackA);
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    link.fireClose();
    session.setVideo(trackB);
    expect(link.videoCalls).toEqual([trackA]);
  });

  it("sem track inicial, o peer novo recebe null", async () => {
    const { sig } = await hostWith(null);
    sig.emit({ type: "peer_joined", peer_id: 5 });
    expect(h.links[0].videoCalls).toEqual([null]);
  });

  it("depois de stop(), setVideo não chega a ninguém", async () => {
    const { session, sig } = await hostWith(trackA);
    sig.emit({ type: "peer_joined", peer_id: 5 });
    const link = h.links[0];
    await session.stop();
    session.setVideo(trackB);
    session.setVideo(null);
    expect(link.videoCalls).toEqual([trackA]);
  });

  it("a ponte nunca para tracks", async () => {
    const stopA = vi.fn();
    const a = { kind: "video", stop: stopA } as unknown as MediaStreamTrack;
    const { session, sig } = await hostWith(a);
    sig.emit({ type: "peer_joined", peer_id: 5 });
    session.setVideo(trackB);
    sig.emit({ type: "peer_left", peer_id: 5 });
    await session.stop();
    expect(stopA).not.toHaveBeenCalled();
  });
});

describe("Convidado (vídeo)", () => {
  it("onVideo recebe a track do link, e nada depois do stop()", async () => {
    const got: MediaStreamTrack[] = [];
    const { p, link } = await guest((t) => got.push(t));
    const t1 = { kind: "video" } as MediaStreamTrack;
    link.emitVideo(t1); // pode chegar antes de abrir os canais
    link.resolve();
    const session = await p;
    const t2 = { kind: "video" } as MediaStreamTrack;
    link.emitVideo(t2);
    expect(got).toEqual([t1, t2]);
    await session.stop();
    link.emitVideo({ kind: "video" });
    expect(got).toEqual([t1, t2]);
  });
});

describe("Convidado", () => {
  it("host_left depois de opened é ignorado; o fechamento do canal decide", async () => {
    const { p, sig, link } = await guest();
    link.resolve();
    await p;
    expect(calls("net_peer_opened")).toHaveLength(1);
    sig.emit({ type: "host_left" });
    await flush();
    expect(link.closed).toBe(false);
    expect(calls("net_peer_closed")).toHaveLength(0);
    link.fireClose();
    await flush();
    expect(calls("net_peer_closed")).toHaveLength(1);
    expect(calls("net_peer_closed")[0][1]).toEqual({ peer: 0 });
  });

  it("host_left antes de opened encerra com erro", async () => {
    const { p, sig, link } = await guest();
    sig.emit({ type: "host_left" });
    await expect(p).rejects.toThrow("O Host saiu");
    expect(link.closed).toBe(true);
    expect(calls("net_peer_closed")).toHaveLength(0);
  });

  it("ICE falho ou canal fechado antes de abrir: não conectou ao Host", async () => {
    const { p, link } = await guest();
    link.reject(new Error("Conexão falhou"));
    await expect(p).rejects.toThrow("Não foi possível conectar ao Host");
    expect(calls("net_peer_opened")).toHaveLength(0);
  });

  it("sinalização caída depois de opened não encerra a sessão", async () => {
    const { p, sig, link } = await guest();
    link.resolve();
    await p;
    sig.drop();
    await flush();
    expect(link.closed).toBe(false);
    expect(calls("net_peer_closed")).toHaveLength(0);
  });

  it("não limita as mensagens do Host", async () => {
    const { p, link } = await guest();
    link.resolve();
    await p;
    for (let i = 0; i < 40; i++) link.msgCb("events", new Uint8Array(1));
    for (let i = 0; i < 300; i++) link.msgCb("state", new Uint8Array(1));
    expect(calls("net_recv")).toHaveLength(340);
  });

  it("ignora signal de outro peer", async () => {
    const { p, sig, link } = await guest();
    sig.emit({ type: "signal", from: 99, data: { a: 1 } });
    sig.emit({ type: "signal", from: 1, data: { b: 2 } });
    expect(link.signals).toEqual([{ b: 2 }]);
    link.resolve();
    await p;
  });
});
