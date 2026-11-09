import { describe, expect, it } from "vitest";
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
