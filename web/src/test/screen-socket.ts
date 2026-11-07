import { vi } from "vitest";

// The Screen panel's tests' stand-ins (WEB-U-242/244/259), shared by screen.test.tsx and screen-sign-out.test.tsx: the
// viewer socket the test drives, and the screen client (noVNC) each panel attaches. Each test file mocks
// "@novnc/novnc" with FakeScreenClient and "../api/stream" with FakeSocket.

/** A socket the test drives: opens, sends the gateway's frames, and closes with a code and reason. */
export class FakeSocket extends EventTarget {
  static opened: FakeSocket[] = [];
  binaryType: BinaryType = "blob";
  protocol = "";
  readyState: number = WebSocket.CONNECTING;
  sent: unknown[] = [];
  constructor(readonly path: string) {
    super();
    FakeSocket.opened.push(this);
  }
  send(data: unknown) {
    this.sent.push(data);
  }
  close() {
    this.readyState = WebSocket.CLOSED;
  }
  /** Opens, then says who holds the screen, as the gateway does once the desktop's side is paired. */
  open() {
    this.upgrade();
    this.frame(control("none"));
  }
  /** Opens only: the gateway still waits for the desktop's side. */
  upgrade() {
    this.readyState = WebSocket.OPEN;
    this.dispatchEvent(new Event("open"));
  }
  frame(data: string | ArrayBuffer) {
    this.dispatchEvent(new MessageEvent("message", { data }));
  }
  shut(code: number, reason = "") {
    this.readyState = WebSocket.CLOSED;
    this.dispatchEvent(new CloseEvent("close", { code, reason }));
  }
}
export const socket = () => FakeSocket.opened.at(-1) as FakeSocket;
export const deps = {
  openSocket: (path: string) => new FakeSocket(path) as unknown as WebSocket,
};
export const control = (holder: string) =>
  JSON.stringify({ type: "control", holder });

/** The screen clients made, newest last: what the panel set on each, its focus, and its connection's events. */
export const clients: {
  viewOnly: boolean;
  blur: ReturnType<typeof vi.fn>;
  focus: ReturnType<typeof vi.fn>;
  emit: (type: "connect" | "disconnect") => void;
}[] = [];

/** The screen client: records itself, and what the panel sets. */
export class FakeScreenClient {
  /** Whether a new client connects by itself, as noVNC does once its handshake is done; false holds it back. */
  static autoConnect = true;
  viewOnly = true;
  scaleViewport = false;
  resizeSession = true;
  clipViewport = true;
  blur = vi.fn();
  focus = vi.fn();
  disconnect = vi.fn();
  private listeners: Record<string, (() => void)[]> = {};
  constructor() {
    clients.push(this);
    if (FakeScreenClient.autoConnect)
      queueMicrotask(() => this.emit("connect"));
  }
  addEventListener(type: string, listener: () => void) {
    (this.listeners[type] ??= []).push(listener);
  }
  /** Fires the client's `connect` or `disconnect`, as noVNC does. */
  emit(type: "connect" | "disconnect") {
    for (const listener of this.listeners[type] ?? []) listener();
  }
}
