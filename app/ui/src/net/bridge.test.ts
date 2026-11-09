import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  invoke: vi.fn(async (..._a: unknown[]) => undefined),
  unlisten: vi.fn(),
  sig: null as any,
  links: [] as any[],
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
    constructor() {
      this.opened.catch(() => {});
    }
    static create() {
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
});

async function host() {
  const p = startHostNet(cfg, () => {});
  await flush();
  h.sig.emit({ type: "room_created", code: "PATO-4821", peer_id: 1 });
  return { session: await p, sig: h.sig };
}

async function guest() {
  const p = startGuestNet(cfg, "pato4821");
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

describe("Convidado", () => {
  it("host_left + fechamento do canal: net_peer_closed uma vez", async () => {
    const { p, sig, link } = await guest();
    link.resolve();
    await p;
    expect(calls("net_peer_opened")).toHaveLength(1);
    sig.emit({ type: "host_left" });
    link.fireClose();
    await flush();
    expect(calls("net_peer_closed")).toHaveLength(1);
    expect(calls("net_peer_closed")[0][1]).toEqual({ peer: 0 });
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
