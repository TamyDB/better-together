/** Captura da tela do Host: sem `min`/`exact` (o Chromium recusa em `getDisplayMedia`). */
export const CAPTURE_OPTIONS: DisplayMediaStreamOptions = {
  video: {
    displaySurface: "monitor",
    width: { max: 1280 },
    height: { max: 720 },
    frameRate: { max: 30 },
  },
  audio: false,
};

/** Encoding por convidado (cerca de 7,5 Mbps de upload com 3 convidados). */
export const VIDEO_ENCODING: RTCRtpEncodingParameters = {
  maxBitrate: 2_500_000,
  maxFramerate: 30,
};

/** Diferença relativa de proporção aceita entre a captura e o monitor do Host. */
export const ASPECT_TOLERANCE = 0.02;

const WINDOW_WARNING =
  "Você está compartilhando uma janela: os bichinhos vão ficar desalinhados. Compartilhe a tela inteira.";
const SCREEN_WARNING = "Essa não é a tela principal: os bichinhos vão ficar desalinhados.";

/** Aviso para o Host quando a captura não coincide com o monitor primário; `null` se estiver ok. */
export function captureWarning(settings: MediaTrackSettings, hostAspect: number): string | null {
  const surface = settings.displaySurface;
  if (surface !== undefined && surface !== "monitor") return WINDOW_WARNING;
  const { width, height } = settings;
  if (width === undefined || height === undefined || width <= 0 || height <= 0) return null;
  if (!Number.isFinite(hostAspect) || hostAspect <= 0) return null;
  if (Math.abs(width / height - hostAspect) / hostAspect > ASPECT_TOLERANCE) return SCREEN_WARNING;
  return null;
}

export type VideoState = "none" | "live" | "paused" | "ended";

export function hostVideoText(s: VideoState): string {
  switch (s) {
    case "none":
      return "Tela não compartilhada";
    case "live":
      return "Compartilhando a tela";
    case "paused":
      return "Vídeo pausado";
    case "ended":
      return "Compartilhamento encerrado";
  }
}

export const GUEST_WAITING_TEXT = "Aguardando o vídeo do Host...";
export const GUEST_PAUSED_TEXT = "Vídeo pausado pelo Host";
