// Os três DataChannels são negotiated: os dois lados os criam com o mesmo id, sem handshake.
export const CHANNELS: Record<"state" | "input" | "events", RTCDataChannelInit> = {
  state: { negotiated: true, id: 0, ordered: false, maxRetransmits: 0 },
  input: { negotiated: true, id: 1, ordered: false, maxRetransmits: 0 },
  events: { negotiated: true, id: 2, ordered: true },
};

export type ChannelName = keyof typeof CHANNELS;

export function isChannelName(v: unknown): v is ChannelName {
  return v === "state" || v === "input" || v === "events";
}
