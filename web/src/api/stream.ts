/** The browser's event stream; with the screen below, the only places outside the transport that open a connection (GW-U-027). */
export function openEventStream(url: string): EventSource {
  return new EventSource(url);
}

/** A task's live screen (ADR-0039 §5): a WebSocket to the same origin, its path from the operations table. */
export function openScreenSocket(path: string): WebSocket {
  const url = new URL(path, window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return new WebSocket(url);
}
