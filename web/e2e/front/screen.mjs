// A desktop task's screen in the fake front (ADR-0039 §5 and §6, its close codes): a WebSocket on the task's
// /screen path that speaks a minimal RFB 3.8 (no security, one small raw picture that changes), relaying control as
// the gateway does: view-only until this socket holds Take over, a control frame on every change, input kept for the
// tests to read only while it holds the screen, and the refusals as close codes. No dependency: the WebSocket framing
// is written out below. Only the "desktop" scenario offers a Desktop template.
import { createHash } from "node:crypto";

import { onReset } from "./state.mjs";

let on = false;
let several = false;
/** Whether the "desktop" scenario is on: a Desktop template, its screen and its limits ("desktops": two templates). */
export const desktopOn = () => on;
/** Per conversation: its open viewers, who holds the screen, the input that got through, a refusal to give. */
const screens = new Map();
onReset((scenarios) => {
  several = scenarios.includes("desktops");
  on = several || scenarios.includes("desktop");
  for (const screen of screens.values())
    for (const viewer of screen.viewers) viewer.end();
  screens.clear();
});

/** The person's environments, with Desktop added in the "desktop" scenario. */
export function withDesktop(templates) {
  if (!on) return templates;
  return {
    ...templates,
    items: [
      ...templates.items,
      {
        name: "desktop",
        display_name: "Desktop",
        description: "A desktop the agent sees and operates",
        capabilities: [
          "shell",
          "python",
          "node",
          "build",
          "browser",
          "desktop",
        ],
        is_default: false,
        cpus: 2,
        memory_mib: 4096,
      },
      ...(several
        ? [
            {
              name: "desktop-large",
              display_name: "Desktop large",
              description: "A larger desktop for heavy pages",
              capabilities: ["shell", "browser", "desktop"],
              is_default: false,
              cpus: 4,
              memory_mib: 8192,
            },
          ]
        : []),
    ],
  };
}

function screenOf(id) {
  let screen = screens.get(id);
  if (!screen) {
    screen = {
      viewers: new Set(),
      holder: null,
      inputs: [],
      refuse: null,
      // The gateway's own notices of this screen (ADR-0039 §6), served by …/screen/notices; takenAt times a hand-back.
      notices: [],
      takenAt: 0,
      // How the next Take over is answered (`/__fake/screen`): null as granted, "late" (`ready: false`, then the
      // grant again once the reset lands), "unclean" (`reset_ok: false`) or "silent" (no answer at all).
      grant: null,
      // How the next sign-out ends: "ok", or the failure's reason ("not_cleared", "unconfirmed").
      signOut: "ok",
    };
    screens.set(id, screen);
  }
  return screen;
}

const WIDTH = 320;
const HEIGHT = 200;
/** The fastest the fake answers an incremental frame request, and the unsent bytes past which it waits. */
const UPDATE_MS = 100;
const BACKLOG = 1 << 20;
/** How long a late grant waits before the stopped run's reset lands: long enough to capture the waiting line, at 390 px
 * under load too. */
const LATE_GRANT_MS = 8_000;
const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/**
 * The upgrade of GET /api/v1/conversations/{id}/screen. `conversationOf` finds the session's own task (null: not
 * signed in, or not theirs). Answers like the gateway: plain HTTP before the upgrade only for no session; everything
 * else upgrades, then closes at once with 4403/4404/4409 and the error as the reason.
 */
export function screenUpgrade(request, socket, conversationOf) {
  const match = /^\/api\/v1\/conversations\/([^/]+)\/screen$/.exec(
    new URL(request.url, "http://localhost").pathname,
  );
  if (!match) return false;
  const id = decodeURIComponent(match[1]);
  const found = conversationOf(request, id);
  if (found === "unauthenticated") {
    socket.end("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
    return true;
  }
  const accept = createHash("sha1")
    .update(`${request.headers["sec-websocket-key"]}${GUID}`)
    .digest("base64");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
      `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
  );
  const viewer = new Viewer(socket);
  const screen = screenOf(id);
  const refusal = !found
    ? [4404, "not_found"]
    : !on || found.template !== "desktop"
      ? [4409, "no_desktop"]
      : screen.refuse
        ? [4409, screen.refuse]
        : screen.viewers.size >= 4
          ? [4409, "screen_viewers"]
          : null;
  if (refusal) {
    viewer.close(...refusal);
    return true;
  }
  screen.viewers.add(viewer);
  viewer.onClose = () => {
    screen.viewers.delete(viewer);
    if (screen.holder === viewer) {
      handedBack(screen, "socket_closed");
      release(screen);
    }
  };
  viewer.onText = (text) => {
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (message.type === "take_over") {
      // Another tab holds it: refused on the same socket, which is told again who holds it (the gateway's rule).
      if (screen.holder && screen.holder !== viewer)
        return viewer.sendText(
          JSON.stringify({
            type: "control",
            holder: holderFor(screen, viewer),
          }),
        );
      // A gateway that never answers: the page offers Take over again after its own wait (15 s).
      if (screen.grant === "silent") return;
      screen.holder = viewer;
      screen.takenAt = Date.now();
      notice(screen, "screen.taken_over");
      if (screen.grant === "late") {
        // Granted before the stopped run had ended, then once more when its reset lands (the gateway's late grant).
        tellAll(screen, { ready: false });
        setTimeout(() => {
          if (screen.holder === viewer) tellAll(screen);
        }, LATE_GRANT_MS);
      } else
        tellAll(screen, screen.grant === "unclean" ? { reset_ok: false } : {});
    }
    if (message.type === "hand_back" && screen.holder === viewer) {
      handedBack(screen, "hand_back");
      release(screen);
    }
  };
  viewer.onInput = (kind) => {
    // View-only is the gateway's rule: input counts only from the socket that holds the screen.
    if (screen.holder === viewer) screen.inputs.push(kind);
  };
  viewer.sendText(
    JSON.stringify({ type: "control", holder: holderFor(screen, viewer) }),
  );
  viewer.startRfb();
  return true;
}

function holderFor(screen, viewer) {
  return screen.holder === null
    ? "none"
    : screen.holder === viewer
      ? "you"
      : "other";
}
/** A control frame to every viewer; `grant`'s fields (`ready`, `reset_ok`) go to the holder only, as the gateway's. */
function tellAll(screen, grant = {}) {
  for (const each of screen.viewers) {
    const holder = holderFor(screen, each);
    each.sendText(
      JSON.stringify({
        type: "control",
        holder,
        ...(holder === "you" ? grant : {}),
      }),
    );
  }
}
function release(screen) {
  screen.holder = null;
  tellAll(screen);
}

/** The person every notice of the fake front names: the signed-in administrator, by id and sign-in address. */
const BY = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  name: "admin@example.invalid",
};
/** Stores a notice as the gateway does: never sequenced, placed after the task's last event, read with …/notices. */
function notice(screen, kind, extra = {}) {
  screen.notices.push({
    kind,
    by: BY,
    at: new Date().toISOString(),
    after_seq: 0,
    ...extra,
  });
}
/** The hand-back's notice: how long it was held, and how it ended. */
function handedBack(screen, ended) {
  notice(screen, "screen.handed_back", {
    seconds: Math.max(0, Math.round((Date.now() - screen.takenAt) / 1000)),
    ended,
  });
}

/**
 * The gateway restarting under a Take over: every viewer's socket drops with the old process (a
 * close the page retries), and the new process records the hand-back the restart cut short before it listens: ended
 * `restart`, with no seconds (how long it was held isn't known). The page reads it once it is back.
 */
export function restartScreen(id) {
  const screen = screenOf(id);
  if (screen.holder) {
    screen.holder = null;
    notice(screen, "screen.handed_back", { ended: "restart" });
  }
  for (const viewer of [...screen.viewers]) viewer.close(1012, "");
}

/**
 * POST …/screen/sign-out (ADR-0039 §6): accepted with no answer (202); its outcome comes a moment later as a notice,
 * stored and sent live, as the gateway turns the runtime's answer into one. `send` writes a frame to the open streams.
 */
export function screenSignOutRoute(suffix, method, id, answer, send) {
  if (suffix !== "/screen/sign-out" || method !== "POST") return false;
  const screen = screenOf(id);
  answer.json(202, {});
  setTimeout(() => {
    if (screen.signOut === "ok") notice(screen, "screen.signed_out");
    else notice(screen, "screen.sign_out_failed", { reason: screen.signOut });
    send({ type: "screen.notice", ...screen.notices.at(-1) });
  }, 300);
  return true;
}

/** GET …/screen/notices: the task's notices, oldest first. */
export function screenNoticesRoute(suffix, method, id, answer) {
  if (suffix !== "/screen/notices" || method !== "GET") return false;
  answer.json(200, { items: screenOf(id).notices });
  return true;
}

/**
 * Test control (`/__fake/screen?conversation=…`): GET reads who holds it, how many viewers, and the input that got
 * through; POST `{refuse}` refuses the next viewers with that reason and closes the open ones with it (as a sandbox
 * stopping closes them with sandbox_not_running), and `{refuse: null}` lets them in again. POST `{grant}` sets how the
 * next Take over is answered and `{sign_out}` how the next sign-out ends (see `screenOf`); each field only if given.
 */
export async function screenFixture(url, request, response, body) {
  if (url.pathname !== "/__fake/screen") return false;
  const screen = screenOf(url.searchParams.get("conversation") ?? "");
  if (request.method === "POST") {
    const input = await body(request);
    if ("grant" in input) screen.grant = input.grant;
    if ("sign_out" in input) screen.signOut = input.sign_out;
    if ("refuse" in input || !("grant" in input || "sign_out" in input)) {
      const { refuse = null } = input;
      screen.refuse = refuse;
      if (refuse)
        for (const viewer of [...screen.viewers]) viewer.close(4409, refuse);
    }
    response.statusCode = 204;
    response.end();
    return true;
  }
  response.setHeader("content-type", "application/json");
  response.end(
    JSON.stringify({
      holder: screen.holder ? "someone" : "none",
      viewers: screen.viewers.size,
      inputs: screen.inputs,
    }),
  );
  return true;
}

/** One viewer: WebSocket frames in and out, and the RFB conversation on its binary frames. */
class Viewer {
  onClose = () => {};
  onText = () => {};
  onInput = () => {};
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.rfb = Buffer.alloc(0);
    this.stage = "version";
    this.timer = null;
    this.tick = 0;
    this.closed = false;
    socket.on("data", (chunk) => this.read(chunk));
    socket.on("close", () => this.gone());
    socket.on("error", () => this.gone());
  }
  gone() {
    if (this.closed) return;
    this.closed = true;
    clearInterval(this.timer);
    clearTimeout(this.pending);
    this.onClose();
  }
  end() {
    this.socket.destroy();
    this.gone();
  }
  frame(opcode, payload) {
    const length = payload.length;
    const head =
      length < 126
        ? Buffer.from([0x80 | opcode, length])
        : length < 65536
          ? Buffer.from([0x80 | opcode, 126, length >> 8, length & 255])
          : (() => {
              const big = Buffer.alloc(10);
              big[0] = 0x80 | opcode;
              big[1] = 127;
              big.writeBigUInt64BE(BigInt(length), 2);
              return big;
            })();
    if (!this.closed) this.socket.write(Buffer.concat([head, payload]));
  }
  sendText(text) {
    this.frame(1, Buffer.from(text));
  }
  sendBinary(bytes) {
    this.frame(2, bytes);
  }
  close(code, reason) {
    const payload = Buffer.alloc(2 + Buffer.byteLength(reason));
    payload.writeUInt16BE(code, 0);
    payload.write(reason, 2);
    this.frame(8, payload);
    this.socket.end();
    this.gone();
  }
  /** Client frames are masked; a close or a broken frame ends the viewer. */
  read(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      if (this.buffer.length < 2) return;
      const opcode = this.buffer[0] & 15;
      let length = this.buffer[1] & 127;
      let at = 2;
      if (length === 126) {
        if (this.buffer.length < 4) return;
        length = this.buffer.readUInt16BE(2);
        at = 4;
      } else if (length === 127) {
        if (this.buffer.length < 10) return;
        length = Number(this.buffer.readBigUInt64BE(2));
        at = 10;
      }
      if (this.buffer.length < at + 4 + length) return;
      const mask = this.buffer.subarray(at, at + 4);
      const payload = Buffer.from(
        this.buffer.subarray(at + 4, at + 4 + length),
      );
      for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
      this.buffer = this.buffer.subarray(at + 4 + length);
      // A close from the page is answered with a close, as a server does, so the page sees its own code.
      if (opcode === 8) {
        this.frame(8, payload.subarray(0, 2));
        this.socket.end();
        return this.gone();
      }
      if (opcode === 1) this.onText(payload.toString());
      if (opcode === 2) this.readRfb(payload);
    }
  }
  startRfb() {
    this.sendBinary(Buffer.from("RFB 003.008\n"));
  }
  /** RFB 3.8: version, the one security type None, its result, ClientInit, then the client's messages. */
  readRfb(bytes) {
    this.rfb = Buffer.concat([this.rfb, bytes]);
    for (;;) {
      if (this.stage === "version") {
        if (this.rfb.length < 12) return;
        this.rfb = this.rfb.subarray(12);
        this.sendBinary(Buffer.from([1, 1]));
        this.stage = "security";
      } else if (this.stage === "security") {
        if (this.rfb.length < 1) return;
        this.rfb = this.rfb.subarray(1);
        this.sendBinary(Buffer.from([0, 0, 0, 0]));
        this.stage = "init";
      } else if (this.stage === "init") {
        if (this.rfb.length < 1) return;
        this.rfb = this.rfb.subarray(1);
        this.sendBinary(serverInit());
        this.stage = "normal";
      } else {
        const size = messageSize(this.rfb);
        if (size === null || this.rfb.length < size) return;
        const type = this.rfb[0];
        // FramebufferUpdateRequest: byte 1 is 1 for an incremental request (only what changed).
        const incremental = type === 3 && this.rfb[1] === 1;
        this.rfb = this.rfb.subarray(size);
        if (type === 3) this.requested(incremental);
        else if (type === 4) this.onInput("key");
        else if (type === 5) this.onInput("pointer");
        else if (type === 6) this.onInput("paste");
      }
    }
  }
  /**
   * A frame request answered as a VNC server does, without flooding: a full request at once; an incremental one (the
   * client asks again after every frame) at most every UPDATE_MS, and never while the socket is still sending, since
   * a slow page would otherwise buffer frames without bound (the 7 Oct heap OOM).
   */
  requested(incremental) {
    if (!incremental) return this.update();
    if (this.pending) return;
    this.pending = setTimeout(() => {
      this.pending = null;
      if (this.closed) return;
      if (this.socket.writableLength > BACKLOG) return this.requested(true);
      this.update();
    }, UPDATE_MS);
  }
  /** One full raw frame, its colour moving with each update, so a test sees the picture change. */
  update() {
    this.tick += 1;
    const header = Buffer.alloc(16);
    header[0] = 0;
    header.writeUInt16BE(1, 2);
    header.writeUInt16BE(WIDTH, 8);
    header.writeUInt16BE(HEIGHT, 10);
    header.writeInt32BE(0, 12);
    const pixels = Buffer.alloc(WIDTH * HEIGHT * 4);
    const shade = (this.tick * 40) % 256;
    for (let i = 0; i < pixels.length; i += 4) {
      pixels[i] = shade;
      pixels[i + 1] = 120;
      pixels[i + 2] = 200;
    }
    this.sendBinary(Buffer.concat([header, pixels]));
  }
}

/** ServerInit: the size, 32-bit true colour (little-endian, red at bit 0), and the desktop's name. */
function serverInit() {
  const name = Buffer.from("fake desktop");
  const init = Buffer.alloc(24);
  init.writeUInt16BE(WIDTH, 0);
  init.writeUInt16BE(HEIGHT, 2);
  init.set([32, 24, 0, 1], 4);
  init.writeUInt16BE(255, 8);
  init.writeUInt16BE(255, 10);
  init.writeUInt16BE(255, 12);
  init.set([0, 8, 16], 14);
  init.writeUInt32BE(name.length, 20);
  return Buffer.concat([init, name]);
}

/** A client message's whole length, or null until enough of it has come to know. */
function messageSize(bytes) {
  if (bytes.length < 1) return null;
  switch (bytes[0]) {
    case 0:
      return 20;
    case 2:
      return bytes.length < 4 ? null : 4 + 4 * bytes.readUInt16BE(2);
    case 3:
      return 10;
    case 4:
      return 8;
    case 5:
      return 6;
    case 6:
      return bytes.length < 8 ? null : 8 + bytes.readUInt32BE(4);
    default:
      return bytes.length;
  }
}

/** A scripted desktop run (ADR-0039 §9, revision 14): a click by ref with its before and after, then the model's marked screenshot. */
export const DESKTOP_RUN = "Act on the desktop";
/** The pictures this fake keeps, by id; "expired" answers 410 as the gateway's purge would. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
  "base64",
);
const capture = (call_id, phase, id, sent_to_model, marked = false) => ({
  type: "screen.capture",
  call_id,
  phase,
  id,
  width: 1280,
  height: 800,
  bytes: PNG.length,
  sha256: "a".repeat(64),
  sent_to_model,
  marked,
});

/** The run's events, numbered from 1; `frame(seq, event, step)` is the fixtures' frame builder. */
export function desktopFrames(run, text, frame) {
  const events = [
    [{ type: "message.user", text, attachments: [] }, null],
    [{ type: "run.started", run_id: run }, null],
    [{ type: "step.started", step: 1, attempt: 1 }, 1],
    [
      {
        type: "tool.call",
        call_id: "desk-1",
        name: "computer",
        arguments: { action: "click", ref: "e7" },
      },
      1,
    ],
    [capture("desk-1", "before", "shot-1-before", false), 1],
    [{ type: "tool.start", call_id: "desk-1" }, 1],
    [{ type: "tool.end", call_id: "desk-1", status: "ok" }, 1],
    [capture("desk-1", "after", "shot-1-after", false), 1],
    [
      {
        type: "tool.result",
        call_id: "desk-1",
        status: "ok",
        content: "Clicked e7; a dialog opened: 'Save as'.",
        target: { role: "button", name: "Save" },
        evidence: { before: true, after: true },
      },
      1,
    ],
    [{ type: "step.started", step: 2, attempt: 1 }, 2],
    [
      {
        type: "tool.call",
        call_id: "desk-2",
        name: "computer",
        arguments: { action: "screenshot" },
      },
      2,
    ],
    [{ type: "tool.start", call_id: "desk-2" }, 2],
    [capture("desk-2", "screenshot", "shot-2-model", true, true), 2],
    [{ type: "tool.end", call_id: "desk-2", status: "ok" }, 2],
    [
      {
        type: "tool.result",
        call_id: "desk-2",
        status: "ok",
        content: "Screenshot 1280×800.",
      },
      2,
    ],
    [{ type: "run.completed", run_id: run }, null],
  ];
  return events.map(([event, step], index) => {
    const value = frame(index + 1, event, step ?? 1);
    if (step === null) {
      value.step = null;
      value.run_id = event.run_id ?? run;
    } else value.run_id = run;
    return value;
  });
}

/**
 * A desktop run whose pictures weren't all kept (WEB-U-282, -241): a screenshot the store didn't keep (its result's
 * evidence `{after: false}`, and no capture), then a screenshot refused as too detailed (`screenshot_too_large`).
 */
export const DESKTOP_GAPS_RUN = "Act on the desktop, some pictures lost";

export function desktopGapsFrames(run, text, frame) {
  const screenshot = (call_id, step) => [
    {
      type: "tool.call",
      call_id,
      name: "computer",
      arguments: { action: "screenshot" },
    },
    step,
  ];
  const events = [
    [{ type: "message.user", text, attachments: [] }, null],
    [{ type: "run.started", run_id: run }, null],
    [{ type: "step.started", step: 1, attempt: 1 }, 1],
    screenshot("gap-1", 1),
    [{ type: "tool.start", call_id: "gap-1" }, 1],
    [{ type: "tool.end", call_id: "gap-1", status: "ok" }, 1],
    [
      {
        type: "tool.result",
        call_id: "gap-1",
        status: "ok",
        content: "Screenshot 1280×800.\n(not kept for the record)",
        evidence: { after: false },
      },
      1,
    ],
    [{ type: "step.started", step: 2, attempt: 1 }, 2],
    screenshot("gap-2", 2),
    [{ type: "tool.start", call_id: "gap-2" }, 2],
    [
      {
        type: "tool.end",
        call_id: "gap-2",
        status: "error",
        failure: { code: "screenshot_too_large" },
      },
      2,
    ],
    [
      {
        type: "tool.result",
        call_id: "gap-2",
        status: "error",
        content:
          "screenshot_too_large: the screen has too much detail to send as a picture even at its smallest size; work from observe",
      },
      2,
    ],
    [{ type: "run.completed", run_id: run }, null],
  ];
  return events.map(([event, step], index) => {
    const value = frame(index + 1, event, step ?? 1);
    if (step === null) {
      value.step = null;
      value.run_id = event.run_id ?? run;
    } else value.run_id = run;
    return value;
  });
}

/** A run past each per-run cap (ADR-0039 §9-10): screenshots, then actions, and a screenshot too detailed to send. */
export const CAPS_RUN = "Act on the desktop past its caps";
/** A desktop run that stays open (no end), for states shown over an active run (a sandbox given up, WEB-U-311). */
export const OPEN_RUN = "Keep working on the desktop";

/** Scripted runs made of tool calls, by message text: each call `[name, arguments, failure code or null, content]`. */
const SCRIPTED = {
  [CAPS_RUN]: {
    open: false,
    calls: [
      [
        "computer",
        { action: "screenshot" },
        "screenshot_limit",
        "screenshot_limit: this run has sent the model as many screenshots as it may",
      ],
      [
        "computer",
        { action: "click", ref: "e3" },
        "action_limit",
        "action_limit: this run has taken as many desktop actions as it may",
      ],
      [
        "computer",
        { action: "screenshot" },
        "screenshot_too_large",
        "screenshot_too_large: the screen has too much detail to send as a picture even at its smallest size; work from observe",
      ],
    ],
  },
  [OPEN_RUN]: { open: true, calls: [] },
};
export const scriptedRun = (text) => text in SCRIPTED;

/** A scripted run's events, numbered from 1, as `desktopFrames`: one step per call, ended unless the run stays open. */
export function scriptedFrames(run, text, frame) {
  const { open, calls } = SCRIPTED[text];
  const events = [
    [{ type: "message.user", text, attachments: [] }, null],
    [{ type: "run.started", run_id: run }, null],
  ];
  calls.forEach(([name, args, failure, content], index) => {
    const step = index + 1;
    const call_id = `scripted-${step}`;
    const status = failure ? "error" : "ok";
    events.push(
      [{ type: "step.started", step, attempt: 1 }, step],
      [{ type: "tool.call", call_id, name, arguments: args }, step],
      [{ type: "tool.start", call_id }, step],
      [
        {
          type: "tool.end",
          call_id,
          status,
          ...(failure ? { failure: { code: failure } } : {}),
        },
        step,
      ],
      [{ type: "tool.result", call_id, status, content }, step],
    );
  });
  if (open)
    events.push([
      { type: "step.started", step: calls.length + 1, attempt: 1 },
      calls.length + 1,
    ]);
  else events.push([{ type: "run.completed", run_id: run }, null]);
  return events.map(([event, step], index) => {
    const value = frame(index + 1, event, step ?? 1);
    if (step === null) {
      value.step = null;
      value.run_id = event.run_id ?? run;
    } else value.run_id = run;
    return value;
  });
}

const SCREENSHOTS = new Set(["shot-1-before", "shot-1-after", "shot-2-model"]);

/** GET …/screenshots/{id} for the session's own task (ADR-0039 §9): the picture, 410 once expired, else 404. */
export function screenshotRoute(suffix, method, response, answer) {
  const id = /^\/screenshots\/([^/]+)$/.exec(suffix)?.[1];
  if (!id || method !== "GET") return false;
  if (id === "expired") {
    answer.error(410, "screenshot_expired", "This screenshot has expired.");
    return true;
  }
  if (!SCREENSHOTS.has(id)) {
    answer.error(404, "not_found", "No such screenshot.");
    return true;
  }
  response.writeHead(200, {
    "content-type": "image/png",
    "cache-control": "no-store",
    "x-content-type-options": "nosniff",
  });
  response.end(PNG);
  return true;
}
