import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PeerLink } from "./rtc";
import { MAX_SDP_BYTES } from "./sdp";
import { VIDEO_ENCODING } from "./video";

/** SDP mínimo válido: um `m=video` e um `m=application`. */
export const VALID_SDP = "v=0\r\nm=video 9 X\r\nm=application 9 X\r\n";

/** RTCPeerConnection falso reutilizável: instala via `vi.stubGlobal` e devolve os espiões. */
export function installFakePeerConnection() {
  const sender = { replaceTrack: vi.fn(async (_t: MediaStreamTrack | null) => {}) };
  const pc = {
    sender,
    addTransceiver: vi.fn((_kind: string, _init?: RTCRtpTransceiverInit) => ({ sender })),
    ontrack: null as ((e: { track: MediaStreamTrack }) => void) | null,
    createDataChannel: vi.fn(() => ({
      binaryType: "",
      readyState: "connecting",
      send: vi.fn(),
      close: vi.fn(),
    })),
    createOffer: vi.fn(async () => ({ type: "offer", sdp: VALID_SDP })),
    createAnswer: vi.fn(async () => ({ type: "answer", sdp: VALID_SDP })),
    setLocalDescription: vi.fn(async () => {}),
    setRemoteDescription: vi.fn(async () => {}),
    addIceCandidate: vi.fn(async () => {}),
    close: vi.fn(),
    connectionState: "new",
  };
  vi.stubGlobal("RTCPeerConnection", vi.fn(() => pc));
  return pc;
}

const sig = (type: string, sdp: string) => ({ kind: "sdp", sdp: { type, sdp } });

describe("PeerLink.handleSignal (filtro de SDP)", () => {
  let pc: ReturnType<typeof installFakePeerConnection>;
  let sent: unknown[];
  const make = (role: "host" | "guest") =>
    PeerLink.create([], (d) => sent.push(d), role);

  beforeEach(() => {
    pc = installFakePeerConnection();
    sent = [];
  });
  afterEach(() => vi.unstubAllGlobals());

  it("host ignora offer", async () => {
    await make("host").handleSignal(sig("offer", VALID_SDP));
    expect(pc.setRemoteDescription).not.toHaveBeenCalled();
  });

  it("host ignora answer com audio", async () => {
    await make("host").handleSignal(sig("answer", VALID_SDP + "m=audio 9 X\r\n"));
    expect(pc.setRemoteDescription).not.toHaveBeenCalled();
  });

  it("host ignora answer grande demais", async () => {
    const big = VALID_SDP + `a=${"x".repeat(MAX_SDP_BYTES)}\r\n`;
    await make("host").handleSignal(sig("answer", big));
    expect(pc.setRemoteDescription).not.toHaveBeenCalled();
  });

  it("host aceita answer valida", async () => {
    await make("host").handleSignal(sig("answer", VALID_SDP));
    expect(pc.setRemoteDescription).toHaveBeenCalledTimes(1);
  });

  it("convidado ignora answer", async () => {
    await make("guest").handleSignal(sig("answer", VALID_SDP));
    expect(pc.setRemoteDescription).not.toHaveBeenCalled();
  });

  it("convidado aceita offer e responde com answer", async () => {
    await make("guest").handleSignal(sig("offer", VALID_SDP));
    expect(pc.setRemoteDescription).toHaveBeenCalledTimes(1);
    expect(sent).toEqual([{ kind: "sdp", sdp: { type: "answer", sdp: VALID_SDP } }]);
  });
});

const track = (kind = "video") => ({ kind }) as MediaStreamTrack;

describe("PeerLink (vídeo)", () => {
  let pc: ReturnType<typeof installFakePeerConnection>;
  const make = (role: "host" | "guest") => PeerLink.create([], () => {}, role);

  beforeEach(() => {
    pc = installFakePeerConnection();
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("host cria 1 transceiver sendonly com VIDEO_ENCODING antes da oferta", async () => {
    await make("host").makeOffer();
    expect(pc.addTransceiver).toHaveBeenCalledTimes(1);
    expect(pc.addTransceiver).toHaveBeenCalledWith("video", {
      direction: "sendonly",
      sendEncodings: [VIDEO_ENCODING],
    });
    expect(pc.addTransceiver.mock.invocationCallOrder[0]).toBeLessThan(
      pc.createOffer.mock.invocationCallOrder[0],
    );
  });

  it("convidado não cria transceiver", () => {
    make("guest");
    expect(pc.addTransceiver).not.toHaveBeenCalled();
  });

  it("setVideo troca a track do sender", async () => {
    const t = track();
    await make("host").setVideo(t);
    expect(pc.sender.replaceTrack).toHaveBeenCalledWith(t);
  });

  it("setVideo depois de close() não chama nada", async () => {
    const link = make("host");
    link.close();
    await link.setVideo(track());
    expect(pc.sender.replaceTrack).not.toHaveBeenCalled();
  });

  it("replaceTrack que rejeita não propaga", async () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    pc.sender.replaceTrack.mockRejectedValueOnce(new Error("falhou"));
    await expect(make("host").setVideo(track())).resolves.toBeUndefined();
    expect(debug).toHaveBeenCalled();
  });

  it("trocas rápidas: a última track vence", async () => {
    const pending: (() => void)[] = [];
    pc.sender.replaceTrack.mockImplementation(
      () => new Promise<void>((res) => pending.push(res)),
    );
    const link = make("host");
    const a = track();
    const b = track();
    const done = [link.setVideo(a), link.setVideo(b), link.setVideo(null)];
    expect(pc.sender.replaceTrack).toHaveBeenCalledTimes(1);
    while (pending.length) {
      pending.shift()!();
      await new Promise((r) => setTimeout(r, 0));
    }
    await Promise.all(done);
    const args = pc.sender.replaceTrack.mock.calls.map((c) => c[0]);
    expect(args).toEqual([a, null]);
  });

  it("falha no meio de trocas rápidas ainda aplica a última", async () => {
    vi.spyOn(console, "debug").mockImplementation(() => {});
    let rejectFirst!: (e: Error) => void;
    pc.sender.replaceTrack.mockImplementationOnce(
      () => new Promise<void>((_, rej) => (rejectFirst = rej)),
    );
    const link = make("host");
    const a = track();
    const b = track();
    const first = link.setVideo(a);
    const second = link.setVideo(b);
    rejectFirst(new Error("falhou"));
    await Promise.all([first, second]);
    expect(pc.sender.replaceTrack.mock.calls.map((c) => c[0])).toEqual([a, b]);
  });

  it("close() durante a troca não aplica a pendente", async () => {
    let resolveFirst!: () => void;
    pc.sender.replaceTrack.mockImplementationOnce(
      () => new Promise<void>((res) => (resolveFirst = res)),
    );
    const link = make("host");
    const first = link.setVideo(track());
    const second = link.setVideo(track());
    link.close();
    resolveFirst();
    await Promise.all([first, second]);
    expect(pc.sender.replaceTrack).toHaveBeenCalledTimes(1);
  });

  it("ontrack antes de onVideo entrega a track no registro", () => {
    const link = make("guest");
    const t = track();
    pc.ontrack?.({ track: t });
    const got: MediaStreamTrack[] = [];
    link.onVideo((x) => got.push(x));
    expect(got).toEqual([t]);
    const t2 = track();
    pc.ontrack?.({ track: t2 });
    expect(got).toEqual([t, t2]);
  });

  it("convidado ignora track que não é de vídeo", () => {
    const link = make("guest");
    const got: MediaStreamTrack[] = [];
    link.onVideo((x) => got.push(x));
    pc.ontrack?.({ track: track("audio") });
    expect(got).toEqual([]);
  });

  it("nada é entregue depois de close()", () => {
    const link = make("guest");
    pc.ontrack?.({ track: track() });
    link.close();
    const got: MediaStreamTrack[] = [];
    link.onVideo((x) => got.push(x)); // a guardada não sai depois do close
    pc.ontrack?.({ track: track() });
    expect(got).toEqual([]);
  });

  it("onVideo registrado antes do close() não recebe ontrack depois", () => {
    const link = make("guest");
    const cb = vi.fn();
    link.onVideo(cb);
    link.close();
    pc.ontrack?.({ track: track() });
    expect(cb).not.toHaveBeenCalled();
  });
});
