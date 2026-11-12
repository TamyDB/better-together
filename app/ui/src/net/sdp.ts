export type Role = "host" | "guest";

/** Teto do SDP vindo do outro peer (em unidades UTF-16, suficiente como limite). */
export const MAX_SDP_BYTES = 32 * 1024;

/**
 * O Host é sempre o offerer: só aceita `answer`; o convidado só aceita `offer`.
 * Só `video` (0 ou 1) e `application` (exatamente 1) são mídias válidas. Pura, nunca lança.
 */
export function acceptSdp(role: Role, sdp: RTCSessionDescriptionInit): boolean {
  if (typeof sdp !== "object" || sdp === null) return false;
  if (sdp.type !== (role === "host" ? "answer" : "offer")) return false;
  const text = sdp.sdp;
  if (typeof text !== "string" || text.length > MAX_SDP_BYTES) return false;
  let video = 0;
  let application = 0;
  for (const line of text.split(/\r?\n/)) {
    if (!line.startsWith("m=")) continue;
    const kind = line.slice(2).split(" ")[0];
    if (kind === "video") video++;
    else if (kind === "application") application++;
    else return false;
  }
  return video <= 1 && application === 1;
}
