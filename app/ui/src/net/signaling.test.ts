import { describe, expect, it, vi } from "vitest";
import { normalizeCode, parseServerMsg } from "./signaling";

describe("parseServerMsg", () => {
  it("aceita os 7 tipos", () => {
    const ok = [
      '{"type":"room_created","code":"PATO-4821","peer_id":1}',
      '{"type":"joined","code":"PATO-4821","peer_id":2,"host_id":1}',
      '{"type":"peer_joined","peer_id":2}',
      '{"type":"peer_left","peer_id":2}',
      '{"type":"signal","from":2,"data":{"sdp":"x"}}',
      '{"type":"host_left"}',
      '{"type":"error","reason":"room_full"}',
    ];
    for (const raw of ok) expect(parseServerMsg(raw), raw).not.toBeNull();
    expect(parseServerMsg(ok[4])).toEqual({ type: "signal", from: 2, data: { sdp: "x" } });
  });

  it("rejeita lixo", () => {
    for (const raw of ['"oi"', "oi", "{}", '{"type":"joined"}', '{"type":"nope"}', "null"]) {
      expect(parseServerMsg(raw), raw).toBeNull();
    }
    expect(parseServerMsg('{"type":"error","reason":"xyz"}')).toBeNull();
    expect(parseServerMsg('{"type":"peer_joined","peer_id":"a"}')).toBeNull();
  });
});

describe("normalizeCode", () => {
  it("normaliza variantes", () => {
    for (const ok of ["pato 4821", " Pato-4821 ", "PATO4821"]) {
      expect(normalizeCode(ok), ok).toBe("PATO-4821");
    }
  });

  it("rejeita inválidos", () => {
    for (const bad of ["PAT-4821", "PATO-48A1", "PATO-48210", "", "ßß-4821", "ÁTO-4821", "A".repeat(100)]) {
      expect(normalizeCode(bad), bad).toBeNull();
    }
  });
});

describe("SignalingClient.onClose", () => {
  it("chama já se o socket fechou antes do registro", async () => {
    const { SignalingClient } = await import("./signaling");
    class FakeWS {
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;
      readyState = 1;
      onopen: (() => void) | null = null;
      onclose: (() => void) | null = null;
      onerror: (() => void) | null = null;
      onmessage: unknown = null;
      constructor() {
        queueMicrotask(() => this.onopen?.());
      }
      close() {}
    }
    vi.stubGlobal("WebSocket", FakeWS);
    try {
      const c = await SignalingClient.connect("ws://x");
      const ws = (c as unknown as { ws: FakeWS }).ws;
      ws.readyState = 3;
      ws.onclose?.();
      const cb = vi.fn();
      c.onClose(cb);
      expect(cb).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("SignalingClient.connect timeout", () => {
  it("fecha o socket que abriria tarde e rejeita", async () => {
    const { SignalingClient } = await import("./signaling");
    let inst!: { closed: boolean; onopen: (() => void) | null };
    class FakeWS {
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;
      readyState = 0;
      closed = false;
      onopen: (() => void) | null = null;
      onerror: (() => void) | null = null;
      onclose: (() => void) | null = null;
      constructor() {
        inst = this;
      }
      close() {
        this.closed = true;
      }
    }
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", FakeWS);
    try {
      const p = SignalingClient.connect("ws://x", 1000);
      const assertion = expect(p).rejects.toThrow("Não foi possível conectar ao servidor");
      await vi.advanceTimersByTimeAsync(1000);
      await assertion;
      expect(inst.closed).toBe(true);
      expect(inst.onopen).toBeNull();
    } finally {
      vi.useRealTimers();
      vi.unstubAllGlobals();
    }
  });
});
