import { describe, expect, it } from "vitest";
import {
  ASPECT_TOLERANCE,
  CAPTURE_OPTIONS,
  GUEST_PAUSED_TEXT,
  GUEST_WAITING_TEXT,
  STALL_MS,
  VIDEO_ENCODING,
  captureWarning,
  hostVideoText,
} from "./video";

const WINDOW_WARNING =
  "Você está compartilhando uma janela: os bichinhos vão ficar desalinhados. Compartilhe a tela inteira.";
const SCREEN_WARNING = "Essa não é a tela principal: os bichinhos vão ficar desalinhados.";

describe("CAPTURE_OPTIONS", () => {
  it("capture_options_shape", () => {
    expect(CAPTURE_OPTIONS.audio).toBe(false);
    const v = CAPTURE_OPTIONS.video as MediaTrackConstraints;
    expect(v.displaySurface).toBe("monitor");
    expect(v.width).toEqual({ max: 1280 });
    expect(v.height).toEqual({ max: 720 });
    expect(v.frameRate).toEqual({ max: 30 });
    for (const c of [v.width, v.height, v.frameRate]) {
      expect(c).not.toHaveProperty("min");
      expect(c).not.toHaveProperty("exact");
    }
  });

  it("encoding por convidado", () => {
    expect(VIDEO_ENCODING).toEqual({ maxBitrate: 2_500_000, maxFramerate: 30 });
    expect(ASPECT_TOLERANCE).toBe(0.02);
  });
});

describe("captureWarning", () => {
  it("capture_warning_cases", () => {
    const wide = 16 / 9;
    expect(captureWarning({ displaySurface: "monitor", width: 1280, height: 720 }, wide)).toBeNull();
    expect(captureWarning({ displaySurface: "monitor", width: 1280, height: 540 }, wide)).toBe(
      SCREEN_WARNING,
    );
    expect(captureWarning({ displaySurface: "window" }, wide)).toBe(
      WINDOW_WARNING,
    );
    expect(captureWarning({}, wide)).toBeNull();
    expect(captureWarning({ width: 1280, height: 800 }, 16 / 10)).toBeNull();
  });

  it("fronteira da tolerância de proporção", () => {
    // Desvio relativo de 1,9% passa; de 2,1% avisa.
    expect(captureWarning({ width: 1019, height: 1000 }, 1)).toBeNull();
    expect(captureWarning({ width: 1021, height: 1000 }, 1)).toBe(SCREEN_WARNING);
    expect(captureWarning({ width: 981, height: 1000 }, 1)).toBeNull();
    expect(captureWarning({ width: 979, height: 1000 }, 1)).toBe(SCREEN_WARNING);
  });

  it("janela vem antes da proporção", () => {
    const s = { displaySurface: "window", width: 1280, height: 540 };
    expect(captureWarning(s, 16 / 9)).toBe(WINDOW_WARNING);
  });

  it("entradas inválidas não avisam proporção", () => {
    for (const aspect of [NaN, Infinity, 0, -1]) {
      expect(captureWarning({ width: 1280, height: 540 }, aspect)).toBeNull();
    }
    expect(captureWarning({ width: 0, height: 540 }, 16 / 9)).toBeNull();
    expect(captureWarning({ width: 1280, height: 0 }, 16 / 9)).toBeNull();
  });
});

describe("textos", () => {
  it("host_video_text", () => {
    expect(hostVideoText("none")).toBe("Tela não compartilhada");
    expect(hostVideoText("live")).toBe("Compartilhando a tela");
    expect(hostVideoText("paused")).toBe("Vídeo pausado");
    expect(hostVideoText("ended")).toBe("Compartilhamento encerrado");
    expect(GUEST_WAITING_TEXT).toBe("Aguardando o vídeo do Host...");
    expect(GUEST_PAUSED_TEXT).toBe("Vídeo pausado pelo Host");
  });

  it("vídeo parado por 3 s vira pausado", () => {
    expect(STALL_MS).toBe(3000);
  });
});
