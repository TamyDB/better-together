import { describe, expect, it } from "vitest";
import { MAX_SDP_BYTES, acceptSdp } from "./sdp";

const sdp = (type: "offer" | "answer", ms: string[], eol = "\r\n"): RTCSessionDescriptionInit => ({
  type,
  sdp: ["v=0", ...ms.map((m) => `m=${m} 9 UDP/TLS/RTP/SAVPF 96`), ""].join(eol),
});

describe("acceptSdp", () => {
  it("host_accepts_answer_only", () => {
    expect(acceptSdp("host", sdp("answer", ["video", "application"]))).toBe(true);
    expect(acceptSdp("host", sdp("offer", ["video", "application"]))).toBe(false);
  });

  it("guest_accepts_offer_only", () => {
    expect(acceptSdp("guest", sdp("offer", ["video", "application"]))).toBe(true);
    expect(acceptSdp("guest", sdp("answer", ["video", "application"]))).toBe(false);
  });

  it("rejects_audio_and_duplicates", () => {
    expect(acceptSdp("host", sdp("answer", ["audio", "application"]))).toBe(false);
    expect(acceptSdp("host", sdp("answer", ["video", "video"]))).toBe(false);
    expect(acceptSdp("host", sdp("answer", ["application", "application"]))).toBe(false);
  });

  it("rejects_oversized", () => {
    expect(acceptSdp("host", { type: "answer", sdp: "a".repeat(MAX_SDP_BYTES + 1) })).toBe(false);
    expect(acceptSdp("host", { type: "answer", sdp: "a".repeat(MAX_SDP_BYTES) })).toBe(true);
  });

  it("accepts_crlf_and_lf", () => {
    for (const eol of ["\r\n", "\n"]) {
      expect(acceptSdp("host", sdp("answer", ["video", "application"], eol))).toBe(true);
      expect(acceptSdp("host", sdp("answer", ["audio"], eol))).toBe(false);
      expect(acceptSdp("host", sdp("answer", ["video", "video"], eol))).toBe(false);
    }
  });

  it("nao lanca em entrada estranha", () => {
    expect(acceptSdp("host", { type: "answer" })).toBe(false);
    expect(acceptSdp("host", {} as RTCSessionDescriptionInit)).toBe(false);
  });
});
