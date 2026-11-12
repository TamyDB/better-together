import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PeerLink } from "./rtc";
import { MAX_SDP_BYTES } from "./sdp";

/** SDP mínimo válido: um `m=video` e um `m=application`. */
export const VALID_SDP = "v=0\r\nm=video 9 X\r\nm=application 9 X\r\n";

/** RTCPeerConnection falso reutilizável: instala via `vi.stubGlobal` e devolve os espiões. */
export function installFakePeerConnection() {
  const pc = {
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
