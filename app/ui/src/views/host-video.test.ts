import { describe, expect, it, vi } from "vitest";
import type { VideoState } from "../net/video";
import { HostVideo } from "./host-video";

/** Track falsa: só o que o `HostVideo` usa. */
function fakeTrack(settings: MediaTrackSettings = { displaySurface: "monitor" }) {
  const t = {
    contentHint: "",
    onended: null as ((ev: Event) => void) | null,
    stop: vi.fn(),
    getSettings: () => settings,
    end() {
      t.onended?.(new Event("ended"));
    },
  };
  return t;
}
type FakeTrack = ReturnType<typeof fakeTrack>;

const streamOf = (t: FakeTrack) =>
  ({ getVideoTracks: () => [t], getTracks: () => [t] }) as unknown as MediaStream;

function setup(...results: (FakeTrack | "refuse")[]) {
  const capture = vi.fn(() => {
    const r = results.shift();
    return r === "refuse" || r === undefined
      ? Promise.reject(new DOMException("recusado", "NotAllowedError"))
      : Promise.resolve(streamOf(r));
  });
  const apply = vi.fn<(t: MediaStreamTrack | null) => void>();
  const states: VideoState[] = [];
  const video = new HostVideo(capture, apply, (s) => states.push(s));
  return { video, capture, apply, states };
}

describe("HostVideo", () => {
  it("share chama capture de forma síncrona", () => {
    const { video, capture } = setup(fakeTrack());
    void video.share();
    expect(capture).toHaveBeenCalledTimes(1);
  });

  it("share resolvido aplica a track, ajusta o contentHint e fica live", async () => {
    const t = fakeTrack({ displaySurface: "monitor", width: 1280, height: 720 });
    const { video, apply, states } = setup(t);
    await video.share();
    expect(apply.mock.calls).toEqual([[t]]);
    expect(t.contentHint).toBe("detail");
    expect(states).toEqual(["live"]);
    expect(video.settings()).toEqual({ displaySurface: "monitor", width: 1280, height: 720 });
  });

  it("segundo share para a primeira track e aplica a nova", async () => {
    const a = fakeTrack();
    const b = fakeTrack();
    const { video, apply } = setup(a, b);
    await video.share();
    await video.share();
    expect(a.stop).toHaveBeenCalledTimes(1);
    expect(b.stop).not.toHaveBeenCalled();
    expect(apply.mock.calls).toEqual([[a], [b]]);
    // O `ended` da antiga não derruba a nova.
    a.end();
    expect(apply).toHaveBeenCalledTimes(2);
  });

  it("capture rejeitando mantém none e não chama apply", async () => {
    const { video, apply, states } = setup("refuse");
    await video.share();
    expect(apply).not.toHaveBeenCalled();
    expect(states).toEqual([]);
    expect(video.settings()).toBeNull();
  });

  it("capture lançando de forma síncrona vira recusa", async () => {
    const apply = vi.fn();
    const video = new HostVideo(
      () => {
        throw new Error("sem mediaDevices");
      },
      apply,
      () => {},
    );
    await expect(video.share()).resolves.toBeUndefined();
    expect(apply).not.toHaveBeenCalled();
  });

  it("recusa ao trocar mantém a track atual", async () => {
    const a = fakeTrack();
    const { video, apply, states } = setup(a, "refuse");
    await video.share();
    await video.share();
    expect(a.stop).not.toHaveBeenCalled();
    expect(apply.mock.calls).toEqual([[a]]);
    expect(states).toEqual(["live"]);
  });

  it("onended aplica null e fica ended", async () => {
    const t = fakeTrack();
    const { video, apply, states } = setup(t);
    await video.share();
    t.end();
    expect(apply.mock.calls).toEqual([[t], [null]]);
    expect(states).toEqual(["live", "ended"]);
    expect(video.settings()).toBeNull();
    // Sem track não há o que pausar nem retomar.
    video.pause();
    video.resume();
    expect(apply).toHaveBeenCalledTimes(2);
  });

  it("pause aplica null e resume reaplica a track", async () => {
    const t = fakeTrack();
    const { video, apply, states } = setup(t);
    await video.share();
    video.pause();
    video.pause();
    video.resume();
    video.resume();
    expect(apply.mock.calls).toEqual([[t], [null], [t]]);
    expect(states).toEqual(["live", "paused", "live"]);
    expect(t.stop).not.toHaveBeenCalled();
  });

  it("pause e resume sem track são no-ops", () => {
    const { video, apply, states } = setup();
    video.pause();
    video.resume();
    expect(apply).not.toHaveBeenCalled();
    expect(states).toEqual([]);
  });

  it("dispose para a track uma vez e silencia pause, resume e onended", async () => {
    const t = fakeTrack();
    const { video, apply, states } = setup(t);
    await video.share();
    video.dispose();
    video.dispose();
    expect(t.stop).toHaveBeenCalledTimes(1);
    video.pause();
    video.resume();
    t.end();
    expect(apply.mock.calls).toEqual([[t]]);
    expect(states).toEqual(["live"]);
    expect(video.settings()).toBeNull();
  });

  it("captura que chega depois do dispose é parada e não aplicada", async () => {
    const t = fakeTrack();
    const { video, apply, states } = setup(t);
    const p = video.share();
    video.dispose();
    await p;
    expect(t.stop).toHaveBeenCalledTimes(1);
    expect(apply).not.toHaveBeenCalled();
    expect(states).toEqual([]);
  });

  it("share depois do dispose não abre o seletor", async () => {
    const { video, capture } = setup(fakeTrack());
    video.dispose();
    await video.share();
    expect(capture).not.toHaveBeenCalled();
  });

  it("stream sem track de vídeo é descartado", async () => {
    const audio = fakeTrack();
    const capture = vi.fn(async () => ({ getVideoTracks: () => [], getTracks: () => [audio] }) as unknown as MediaStream);
    const apply = vi.fn();
    const video = new HostVideo(capture, apply, () => {});
    await video.share();
    expect(audio.stop).toHaveBeenCalledTimes(1);
    expect(apply).not.toHaveBeenCalled();
  });
});
