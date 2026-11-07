/**
 * The viewer socket, split in two (ADR-0039 §5 and its contract): the screen's own protocol travels as binary frames,
 * which go to the screen client (noVNC) through this raw channel; the gateway's control messages travel as text frames
 * (`{"type": "control", "holder": "none" | "you" | "other"}`, with `"ready": false` when control came before the stopped run
 * had ended, and `"reset_ok": false` when that run's reset couldn't empty the browser; an absent field means true),
 * which go to `onControl`, and the page's own
 * (`take_over`, `hand_back`) go out the same way. The screen client never sees a text frame.
 */
export type Holder = "none" | "you" | "other";

export class ScreenChannel {
  onopen: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(
    private readonly socket: WebSocket,
    private readonly onControl: (
      holder: Holder,
      grant: { ready: boolean; resetOk: boolean },
    ) => void,
  ) {
    socket.binaryType = "arraybuffer";
    socket.addEventListener("open", (event) => this.onopen?.(event));
    socket.addEventListener("close", (event) => this.onclose?.(event));
    socket.addEventListener("error", (event) => this.onerror?.(event));
    socket.addEventListener("message", (event: MessageEvent) => {
      if (typeof event.data === "string") this.control(event.data);
      // While held frames wait for their hand-over, a new one queues behind them, so the order stays the server's.
      else if (this.receiver && this.held.length === 0) this.receiver(event);
      else this.held.push(event);
    });
  }

  /**
   * The screen client's receiver. The server speaks first, as soon as the socket opens, and the client (loaded when the
   * panel first shows) may attach later: what came before it is held and handed over, in order, once the attaching call
   * has returned. Never inside it: noVNC sets its receiver first and only then, in the same call, starts its handshake on
   * an open socket, so a frame it got in between failed the connection ("Unknown init state") and the panel kept
   * reconnecting (WEB-U-377).
   */
  private receiver: ((event: MessageEvent) => void) | null = null;
  private held: MessageEvent[] = [];
  get onmessage(): ((event: MessageEvent) => void) | null {
    return this.receiver;
  }
  set onmessage(receiver: ((event: MessageEvent) => void) | null) {
    this.receiver = receiver;
    if (!receiver || this.held.length === 0) return;
    queueMicrotask(() => {
      // A receiver set since then takes the held frames with its own hand-over.
      if (this.receiver !== receiver) return;
      const held = this.held;
      this.held = [];
      for (const event of held) receiver(event);
    });
  }

  /** A control frame the page knows; anything else is ignored (the gateway owns the contract). */
  private control(text: string) {
    let value: unknown;
    try {
      value = JSON.parse(text) as unknown;
    } catch {
      return;
    }
    if (
      typeof value === "object" &&
      value !== null &&
      Reflect.get(value, "type") === "control"
    ) {
      const holder: unknown = Reflect.get(value, "holder");
      if (holder === "none" || holder === "you" || holder === "other")
        this.onControl(holder, {
          ready: Reflect.get(value, "ready") !== false,
          resetOk: Reflect.get(value, "reset_ok") !== false,
        });
    }
  }

  /** The page's own messages: Take over and Hand back. */
  ask(type: "take_over" | "hand_back") {
    if (this.socket.readyState === WebSocket.OPEN)
      this.socket.send(JSON.stringify({ type }));
  }

  // The rest is what the screen client needs of a raw channel.
  get binaryType(): BinaryType {
    return this.socket.binaryType;
  }
  set binaryType(value: BinaryType) {
    this.socket.binaryType = value;
  }
  get protocol(): string {
    return this.socket.protocol;
  }
  get readyState(): number {
    return this.socket.readyState;
  }
  send(data: ArrayBufferLike | ArrayBufferView | Blob | string) {
    this.socket.send(data);
  }
  close(code?: number, reason?: string) {
    this.socket.close(code, reason);
  }
}
