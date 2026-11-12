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
    const base = "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\n";
    const pad = (n: number) => `a=${"x".repeat(n - 3)}\n`;
    const exact = base + pad(MAX_SDP_BYTES - base.length);
    expect(exact.length).toBe(MAX_SDP_BYTES);
    expect(acceptSdp("host", { type: "answer", sdp: exact })).toBe(true);
    expect(acceptSdp("host", { type: "answer", sdp: exact + "a" })).toBe(false);
  });

  it("rejects_missing_application", () => {
    expect(acceptSdp("host", sdp("answer", ["video"]))).toBe(false);
    expect(acceptSdp("host", sdp("answer", []))).toBe(false);
  });

  it("m_line_so_no_inicio_da_linha", () => {
    const base = ["v=0", "m=video 9 X", "m=application 9 X"];
    expect(acceptSdp("host", { type: "answer", sdp: [...base, "a=x m=audio", ""].join("\r\n") })).toBe(true);
    expect(acceptSdp("host", { type: "answer", sdp: [...base, " m=audio 9 X", ""].join("\r\n") })).toBe(true);
  });

  it("rejects_audio_depois_de_validas", () => {
    expect(acceptSdp("host", sdp("answer", ["video", "application", "audio"]))).toBe(false);
  });

  it("rejects_sdp_vazio_e_tipos_estranhos", () => {
    expect(acceptSdp("host", { type: "answer", sdp: "" })).toBe(false);
    for (const type of ["pranswer", "rollback"]) {
      const d = { type, sdp: sdp("answer", ["video", "application"]).sdp } as RTCSessionDescriptionInit;
      expect(acceptSdp("host", d)).toBe(false);
      expect(acceptSdp("guest", d)).toBe(false);
    }
  });

  it("rejects_dois_video_com_eol_misto", () => {
    const text = "v=0\nm=video 9 X\r\nm=video 9 X\nm=application 9 X\r\n";
    expect(acceptSdp("host", { type: "answer", sdp: text })).toBe(false);
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
