import type { VideoState } from "../net/video";

/**
 * Dono da captura de tela do Host: única track viva, parada ao trocar e no `dispose`.
 * `apply` recebe a track a enviar (`null` pausa); depois do `dispose` nunca mais é chamado.
 */
export class HostVideo {
  private track: MediaStreamTrack | null = null;
  private state: VideoState = "none";
  private disposed = false;

  constructor(
    private readonly capture: () => Promise<MediaStream>,
    private readonly apply: (t: MediaStreamTrack | null) => void,
    private readonly onChange: (s: VideoState) => void,
  ) {}

  /**
   * Abre o seletor. `capture()` sai antes de qualquer `await`: precisa do gesto do usuário.
   * Recusa (ou captura indisponível) mantém o estado anterior.
   */
  share(): Promise<void> {
    if (this.disposed) return Promise.resolve();
    let pending: Promise<MediaStream>;
    try {
      pending = this.capture();
    } catch (e) {
      console.debug("captura de tela indisponível", e);
      return Promise.resolve();
    }
    return pending.then(
      (stream) => this.adopt(stream),
      (e) => console.debug("captura de tela recusada", e),
    );
  }

  pause(): void {
    if (this.disposed || this.state !== "live" || !this.track) return;
    this.apply(null);
    this.set("paused");
  }

  resume(): void {
    if (this.disposed || this.state !== "paused" || !this.track) return;
    this.apply(this.track);
    this.set("live");
  }

  settings(): MediaTrackSettings | null {
    return this.track?.getSettings() ?? null;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.release();
  }

  private adopt(stream: MediaStream): void {
    const track = stream.getVideoTracks()[0];
    // Só a primeira track de vídeo fica; o resto (e tudo, se já acabou) é parado.
    for (const t of stream.getTracks()) if (t !== track || this.disposed) t.stop();
    if (!track || this.disposed) return;
    this.release();
    track.contentHint = "detail"; // texto legível vale mais que fluidez
    track.onended = () => {
      // "Parar de compartilhar" do Chromium: só vale para a track atual.
      if (this.disposed || this.track !== track) return;
      this.track = null;
      this.apply(null);
      this.set("ended");
    };
    this.track = track;
    this.apply(track);
    this.set("live");
  }

  /** Para a track atual sem disparar o `onended` dela. */
  private release(): void {
    const old = this.track;
    this.track = null;
    if (!old) return;
    old.onended = null;
    old.stop();
  }

  private set(s: VideoState): void {
    this.state = s;
    this.onChange(s);
  }
}
