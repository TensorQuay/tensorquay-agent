import { describe, expect, it } from "vitest";

import type { Event, ServerFrame } from "../protocol/generated/protocol";
import { parseFrame } from "../protocol/validate";
import { contextLoaded, laterContext } from "../test/context";
import { frame, ready, runId, transcriptFrames } from "../test/frames";

import { noticeText } from "./notice-copy";
import { apply, applyAt, emptyTranscript, SequenceGap } from "./reducer";
import type { Transcript } from "./types";

// Notices store translatable copy; these tests read them in English.
const said = (item: Transcript["items"][number] | undefined) =>
  item?.kind === "notice" ? noticeText(item.copy) : undefined;

const replay = (frames: ServerFrame[]) =>
  frames.reduce(apply, emptyTranscript());

describe("WEB-U-001 sequence and replay", () => {
  it("rejects a gap without changing the last applied sequence", () => {
    const before = apply(
      emptyTranscript(),
      frame(1, { type: "text.delta", text: "first" }),
    );
    expect(() =>
      apply(before, frame(3, { type: "text.delta", text: "missing" })),
    ).toThrow(SequenceGap);
    expect(before.lastSeq).toBe(1);
  });

  it("ignores duplicates and renders the same transcript on full replay", () => {
    const first = replay(transcriptFrames);
    expect(replay(transcriptFrames.flatMap((item) => [item, item]))).toEqual(
      first,
    );
    expect(
      transcriptFrames.filter((item) => "seq" in item).reduce(apply, first),
    ).toEqual(first);
    expect(replay(transcriptFrames)).toEqual(first);
    expect(first.items.filter((item) => item.kind === "user")).toHaveLength(1);
    expect(first.run.status).toBe("completed");
    expect(first.usage.completion).toBe(144);
  });

  it("updates ephemeral session metadata without replaying durable content", () => {
    const finished = replay(transcriptFrames);
    const reconnected = apply(finished, ready);
    expect(reconnected.sandbox).toBe("ready");
    expect(reconnected.items).toBe(finished.items);
    expect(reconnected.run).toBe(finished.run);
    expect(reconnected.lastSeq).toBe(finished.lastSeq);
  });

  it("leaves the transcript unchanged for a runtime-link workspace reply", () => {
    const finished = replay(transcriptFrames);
    const reply: ServerFrame = {
      type: "workspace.reply",
      request_id: 7,
      part: 0,
      last: true,
      result: { kind: "error", code: "busy", message: "m", retryable: true },
    };
    expect(apply(finished, reply)).toBe(finished);
  });

  // GW-I-129 / WEB-U-079: a change to the held messages is counted for the composer; the transcript itself is unchanged.
  it("counts a message.queue frame without moving the cursor or the items", () => {
    const finished = replay(transcriptFrames);
    const next = apply(finished, {
      type: "message.queue",
      held: ["7"],
      dropped: [],
    });
    expect(next.heldVersion).toBe(finished.heldVersion + 1);
    expect(next.lastSeq).toBe(finished.lastSeq);
    expect(next.items).toBe(finished.items);
  });

  // WEB-U-229 (ADR-0034 S6): team.changed is unsequenced and never stored; the page keeps the latest numbers so it reads
  // only what moved, and nothing in the transcript changes.
  it("keeps the latest team.changed without moving the cursor or the items", () => {
    const finished = replay(transcriptFrames);
    const team = "0195d8c0-0000-7000-8000-00000000aaaa";
    const first = apply(finished, {
      type: "team.changed",
      team,
      version: 9,
      last_seq: 15,
    });
    expect(first.team).toEqual({ id: team, version: 9, lastSeq: 15 });
    expect(first.lastSeq).toBe(finished.lastSeq);
    expect(first.items).toBe(finished.items);
    const moved = apply(first, {
      type: "team.changed",
      team,
      version: 10,
      last_seq: 15,
    });
    expect(moved.team).toEqual({ id: team, version: 10, lastSeq: 15 });
    // A frame older than one already seen moves neither number back.
    const stale = apply(moved, {
      type: "team.changed",
      team,
      version: 8,
      last_seq: 16,
    });
    expect(stale.team).toEqual({ id: team, version: 10, lastSeq: 16 });
  });

  // Storage quotas: each notice says what happened to the run and what to do, never "something went wrong".
  it.each([
    [
      "workspace_quota",
      true,
      "This workspace is over its storage quota. The run continues; remove files to get back under it.",
    ],
    [
      "workspace_quota",
      false,
      "The run stopped: this workspace went too far over its storage quota. Remove files, then send your message again.",
    ],
    [
      "state_quota",
      false,
      "The run stopped: this task has saved more than it may. Ask an administrator to make room.",
    ],
  ] as const)(
    "reads %s (retryable %s) as its own sentence",
    (code, retryable, sentence) => {
      const state = apply(apply(emptyTranscript(), ready), {
        type: "error",
        code,
        message: "Private diagnostic",
        retryable,
      });
      expect(state.banner && noticeText(state.banner)).toBe(sentence);
    },
  );

  it("reads decision_engine_unavailable as what the owner can do, without offering a resend", () => {
    const state = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "decision_engine_unavailable",
      message: "Private diagnostic",
      retryable: true,
    });
    expect(state.banner && noticeText(state.banner)).toBe(
      "The task's decision engine is no longer available. An administrator must restore or re-enable it; then send a message to continue.",
    );
    expect(state.actionFailure ?? null).toBeNull();
  });

  it("reads decision_engine_budget_spent in words", () => {
    const state = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "decision_engine_budget_spent",
      message: "Private diagnostic",
      retryable: false,
    });
    expect(state.banner && noticeText(state.banner)).toBe(
      "The decision engine's budget is spent. The agent decides on its own until the budget allows it again; an administrator can raise it.",
    );
  });

  // WEB-U-234: the gateway's shutdown frame is the reconnect it announces (UI-GUIDELINES §14; a pass over the pages
  // before 17.0).
  it("reads a restarting gateway as a reconnect, not a banner", () => {
    const state = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "restarting",
      message: "",
      retryable: true,
    });
    expect(state.banner).toBeNull();
    expect(state.actionFailure ?? null).toBeNull();
  });

  it("keeps a stream error's banner across a sandbox change, which is no reconnect", () => {
    // The gateway sends session.ready on every sandbox change, not only on a reconnect (the store clears on those).
    const over = apply(apply(emptyTranscript(), ready), {
      type: "error",
      code: "workspace_quota",
      message: "",
      retryable: true,
    });
    expect(over.banner).not.toBeNull();
    const busy = apply(over, {
      ...ready,
      sandbox: { backend: "docker", state: "busy" },
    });
    const back = apply(busy, ready);
    expect(back.banner).toEqual(over.banner);
  });

  it("keeps sequenced state immutable and surfaces ephemeral errors and permissions", () => {
    const first = apply(emptyTranscript(), ready);
    const error = apply(first, {
      type: "error",
      code: "sandbox_failed",
      message: "The sandbox could not start.",
      retryable: true,
    });
    expect(error.banner && noticeText(error.banner)).toBe(
      "The task's environment couldn't be started. Send the message again; if it fails again, choose another environment or ask an administrator.",
    );
    expect(first.banner).toBeNull();
    // ADR-0035: an approval request from an older gateway only moves the cursor; nothing is shown and nothing waits.
    const ignored = apply(
      error,
      parseFrame(
        JSON.stringify({
          type: "permission.request",
          seq: 1,
          request_id: "request-example",
          tool: "shell",
          summary: "Run a command",
          risk: "medium",
          expires_at: "2026-01-01T00:00:00Z",
        }),
      ),
    );
    expect(ignored.items).toBe(error.items);
    expect(ignored.lastSeq).toBe(1);
  });
});

describe("WEB-U-013 attempts and interleaved calls", () => {
  it.each([
    ["steps", "Step budget: 80 of 100 steps used"],
    ["wall_clock", "Time budget: 80 of 100 s used"],
    ["output_tokens", "Reply length: 80 % of this task's limit"],
    ["no_progress", "The agent made no progress in its last 80 steps"],
  ] as const)("renders %s as a quiet notice", (budget, text) => {
    const before = apply(
      emptyTranscript(),
      frame(1, { type: "run.started", run_id: runId }),
    );
    const warning = frame(2, {
      type: "budget.warning",
      budget,
      used: 80,
      limit: 100,
    });
    const state = apply(before, warning);
    expect(said(state.items.at(-1))).toBe(text);
    expect(state.run).toEqual(before.run);
    expect(state.banner).toBeNull();
    expect(apply(state, warning)).toEqual(state);
  });

  // WEB-U-252 (a pass over the pages before 17.0): "Time budget: 1530 of 1800 s used" read raw beside
  // "Worked for 1 643 s".
  it.each([
    ["wall_clock", "Time budget: 1\u202f530 of 1\u202f800 s used"],
    ["steps", "Step budget: 1\u202f200 of 1\u202f500 steps used"],
    ["no_progress", "The agent made no progress in its last 1\u202f530 steps"],
  ] as const)("groups %s's thousands as every number does", (budget, text) => {
    const warning = frame(2, {
      type: "budget.warning",
      budget,
      used: budget === "steps" ? 1200 : 1530,
      limit: budget === "steps" ? 1500 : 1800,
    });
    const state = apply(
      apply(
        emptyTranscript(),
        frame(1, { type: "run.started", run_id: runId }),
      ),
      warning,
    );
    expect(said(state.items.at(-1))).toBe(text);
  });

  it("replaces an older attempt's text, reasoning and prepared calls", () => {
    const events: Event[] = [
      { type: "step.started", step: 1, attempt: 1 },
      { type: "text.delta", text: "discarded text" },
      { type: "reasoning.delta", text: "discarded reasoning" },
      { type: "tool.call", name: "shell", call_id: "old", arguments: {} },
      { type: "step.started", step: 1, attempt: 2 },
      { type: "text.delta", text: "replacement" },
      { type: "step.started", step: 1, attempt: 1 },
    ];
    const state = replay(events.map((event, index) => frame(index + 1, event)));
    expect(state.items).toHaveLength(1);
    expect(state.items[0]).toMatchObject({
      kind: "step",
      attempt: 2,
      text: "replacement",
      reasoning: "",
      tools: {},
    });
  });

  it("keeps three interleaved calls independent, including a call that never ran", () => {
    const events: Event[] = [
      { type: "step.started", step: 1 },
      ...["a", "b", "c"].map((call_id): Event => ({
        type: "tool.call",
        name: "shell",
        call_id,
        arguments: { command: call_id },
      })),
      { type: "tool.start", call_id: "a" },
      { type: "tool.start", call_id: "b" },
      { type: "tool.output", call_id: "b", chunk: 0, last: true, data: "B" },
      {
        type: "tool.result",
        call_id: "c",
        status: "error",
        content: "Not permitted",
      },
      { type: "tool.output", call_id: "a", chunk: 0, last: true, data: "A" },
      { type: "tool.end", call_id: "b", status: "ok", exit_code: 0 },
      { type: "tool.end", call_id: "a", status: "error", exit_code: 2 },
      { type: "tool.result", call_id: "b", status: "ok", content: "B result" },
      {
        type: "tool.result",
        call_id: "a",
        status: "error",
        content: "A result",
      },
    ];
    const state = replay(events.map((event, index) => frame(index + 1, event)));
    expect(state.items[0]).toMatchObject({
      kind: "step",
      tools: {
        a: {
          output: "A",
          result: "A result",
          status: "error",
          exitCode: 2,
          ran: true,
        },
        b: {
          output: "B",
          result: "B result",
          status: "ok",
          exitCode: 0,
          ran: true,
        },
        c: {
          output: "",
          result: "Not permitted",
          status: "error",
          ran: false,
          durationMs: null,
        },
      },
    });
  });

  it("keeps retries with their step, and explains budgets and terminal failures with quiet notices", () => {
    const state = replay([
      frame(1, {
        type: "provider.retry",
        attempt: 1,
        reason: "http 503",
        delay_ms: 2000,
      }),
      frame(2, {
        type: "budget.warning",
        budget: "steps",
        used: 80,
        limit: 100,
      }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "budget",
        reason: "the run's step budget was reached",
      }),
    ]);
    // A retry belongs to its step (WEB-U-016), never a transcript notice.
    const step = state.items.find((item) => item.kind === "step");
    expect(step?.kind === "step" && step.retries).toEqual([
      { reason: "http 503", delayMs: 2000 },
    ]);
    expect(
      state.items.filter((item) => item.kind === "notice").map(said),
    ).toEqual([
      expect.stringContaining("80"),
      "The task reached its step limit",
    ]);
    expect(state.run.status).toBe("failed");
  });

  // The Product Owner's direction, then PRD §12 item 22: getting a workspace's files says so in plain words, never
  // "clone", "repository" or "project".
  it("replays getting a workspace's files as quiet notices and hides a skipped one", () => {
    const clone = (
      seq: number,
      status: "started" | "completed" | "skipped" | "failed",
      code?: "egress_denied" | "io" | "internal",
    ) =>
      frame(seq, {
        type: "workspace.clone",
        status,
        message: "Internal detail",
        ...(code ? { code } : {}),
      });
    const texts = (frames: ServerFrame[]) =>
      replay(frames).items.flatMap((item) =>
        item.kind === "notice" ? [noticeText(item.copy)] : [],
      );
    expect(texts([clone(1, "started"), clone(2, "completed")])).toEqual([
      "Getting the workspace's files…",
      "Got the workspace's files",
    ]);
    expect(texts([clone(1, "skipped")])).toEqual([]);
    expect(
      texts([
        clone(1, "failed", "egress_denied"),
        clone(2, "failed", "io"),
        clone(3, "failed", "internal"),
        clone(4, "failed"),
      ]),
    ).toEqual([
      "Couldn't get the workspace's files: its site isn't on the list of sites the agent may reach",
      "Couldn't get the workspace's files: they couldn't be downloaded",
      "Couldn't get the workspace's files",
      "Couldn't get the workspace's files",
    ]);
    const frames = [clone(1, "started"), clone(2, "completed")];
    expect(replay(frames.flatMap((item) => [item, item]))).toEqual(
      replay(frames),
    );
  });

  // ADR-0035: a run never parks for a person; an older gateway's parked marker shows nothing, and the resumption stays
  // a quiet notice.
  it("replays a parked run as nothing and its resumption as a quiet notice, without ending the run", () => {
    const frames = [
      frame(1, { type: "message.user", text: "Deploy.", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      // As an older gateway stored it; the retired marker keeps nothing but its sequence.
      parseFrame(
        JSON.stringify({
          ...frame(3, { type: "run.completed", run_id: runId }),
          event: {
            type: "run.parked",
            run_id: runId,
            request_id: "request-example",
          },
        }),
      ) as ServerFrame,
      frame(4, { type: "run.resumed", run_id: runId, from_step: 1 }),
    ];
    const parked = replay(frames.slice(0, 3));
    expect(parked.items).toEqual(replay(frames.slice(0, 2)).items);
    expect(parked.run).toEqual({ id: runId, status: "running" });
    expect(parked.lastSeq).toBe(3);

    const resumed = replay(frames);
    expect(
      resumed.items.flatMap((item) =>
        item.kind === "notice" ? [noticeText(item.copy)] : [],
      ),
    ).toEqual(["Resumed after the environment restarted"]);
    expect(resumed.run).toEqual({ id: runId, status: "running" });
    expect(replay(frames.flatMap((item) => [item, item]))).toEqual(resumed);
  });
});

// SEC-001 (web side): a blocked network attempt is shown live, quietly and at most once per destination per minute.
describe("SEC-001 blocked network attempts", () => {
  const denied = (host?: string, port = 443): ServerFrame => ({
    type: "error",
    code: "egress_denied",
    message: "Internal detail",
    retryable: false,
    ...(host ? { target: { host, port } } : {}),
  });
  const notices = (state: ReturnType<typeof emptyTranscript>) =>
    state.items.flatMap((item) =>
      item.kind === "notice" ? [noticeText(item.copy)] : [],
    );
  const start = 1_000_000;

  it("shows one quiet line with the destination and never the banner or the gateway message", () => {
    const state = applyAt(emptyTranscript(), denied("example.invalid"), start);
    expect(notices(state)).toEqual([
      "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.",
    ]);
    expect(state.banner).toBeNull();
    expect(state.lastSeq).toBe(0);
    expect(JSON.stringify(state)).not.toContain("Internal detail");
  });

  it("brackets an IPv6 destination and explains a denial without a destination", () => {
    const state = [denied("2001:db8::1", 8443), denied()].reduce(
      (current, next) => applyAt(current, next, start),
      emptyTranscript(),
    );
    expect(notices(state)).toEqual([
      "Blocked network access to [2001:db8::1]:8443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.",
      "Blocked network access: the network allow-list doesn't allow this host. An administrator can change the list under Settings › Administration › Internet access.",
    ]);
  });

  it("shows each host and port at most once a minute", () => {
    const at = (
      state: ReturnType<typeof emptyTranscript>,
      frame: ServerFrame,
      ms: number,
    ) => applyAt(state, frame, ms);
    let state = at(emptyTranscript(), denied("example.invalid"), start);
    state = at(state, denied("example.invalid"), start + 59_999);
    state = at(state, denied("example.invalid", 80), start + 59_999);
    state = at(state, denied("example.invalid"), start + 60_000);
    expect(notices(state)).toEqual([
      "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.",
      "Blocked network access to example.invalid:80: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.",
      "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.",
    ]);
    expect(new Set(state.items.map((item) => item.id)).size).toBe(3);
  });
});

// An event kind the web does not know advances the cursor, dedups and replays like any event, renders
// nothing and never raises the error banner.
describe("WEB-U-001 unknown event kinds", () => {
  const unknown = (seq: number) =>
    parseFrame(
      JSON.stringify({
        type: "event",
        seq,
        run_id: runId,
        step: 1,
        ts: new Date(Date.UTC(2026, 0, 1) + seq * 100).toISOString(),
        event: { type: "memory.pruned", summary: "Earlier steps", dropped: 3 },
      }),
    );
  const known = [
    frame(1, { type: "text.delta", text: "first " }),
    frame(2, { type: "text.delta", text: "second" }),
  ];
  const mixed = [
    frame(1, { type: "text.delta", text: "first " }),
    unknown(2),
    frame(3, { type: "text.delta", text: "second" }),
  ];

  it("keeps sequence order and lets the following known events render", () => {
    const withUnknown = mixed.reduce(apply, emptyTranscript());
    const without = known.reduce(apply, emptyTranscript());
    expect(withUnknown.lastSeq).toBe(3);
    expect(withUnknown.items).toEqual(without.items);
    expect(withUnknown.banner).toBeNull();
  });

  it("replays and dedups like any event", () => {
    const once = mixed.reduce(apply, emptyTranscript());
    expect(
      mixed.flatMap((item) => [item, item]).reduce(apply, emptyTranscript()),
    ).toEqual(once);
    expect(mixed.reduce(apply, once)).toEqual(once);
  });

  // The runtime's answer that the browser was reset (ADR-0039 §6) is a runtime-link frame: the gateway consumes it and
  // never forwards it. Should one arrive, it changes nothing and throws nothing, like the other Desktop link frames.
  it("leaves the transcript as it was for a runtime-link screen.reset", () => {
    const state = [ready, ...known].reduce(apply, emptyTranscript());
    for (const ok of [true, false])
      expect(
        apply(state, parseFrame(JSON.stringify({ type: "screen.reset", ok }))),
      ).toEqual(state);
  });

  it("still reports a gap around an unknown event", () => {
    expect(() => apply(emptyTranscript(), unknown(2))).toThrow(SequenceGap);
    const first = apply(emptyTranscript(), known[0] as ServerFrame);
    expect(() => apply(first, unknown(3))).toThrow(SequenceGap);
  });
});

// The context was compacted before a model call; the activity log says so once, quietly.
describe("WEB-U-001 compaction", () => {
  const compaction = (seq: number) =>
    frame(seq, {
      type: "compaction",
      through_seq: seq - 1,
      source: "model",
      version: 1,
      summary: {
        goal: "Ship the report",
        done: ["Read the data"],
        pending: [],
        decisions: [],
        key_paths: [],
        open_errors: [],
      },
    });
  const frames = [
    frame(1, { type: "text.delta", text: "first" }),
    compaction(2),
    frame(3, { type: "text.delta", text: " second" }),
  ] as ServerFrame[];

  it("adds one quiet line and keeps the following events", () => {
    const state = replay(frames);
    expect(state.lastSeq).toBe(3);
    expect(state.banner).toBeNull();
    expect(
      state.items.flatMap((item) =>
        item.kind === "notice" ? [noticeText(item.copy)] : [],
      ),
    ).toEqual(["Earlier steps summarised"]);
  });

  it("replays and dedups to the same transcript", () => {
    const once = replay(frames);
    expect(replay(frames.flatMap((item) => [item, item]))).toEqual(once);
  });
});

// The fallback tool-call parser gave up; the transcript says so in plain words, once.
describe("WEB-U-001 run failure copy", () => {
  it("shows the plain copy for fallback_parse in the notice and the turn outcome", () => {
    const state = replay([
      frame(1, { type: "message.user", text: "Build it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "fallback_parse",
        reason: "fallback parse failed after retry",
      }),
    ] as ServerFrame[]);
    expect(said(state.items.at(-1))).toBe(
      "The model's answer couldn't be used",
    );
    expect(state.runs[runId]).toMatchObject({ code: "fallback_parse" });
  });
});

// The model route switched to its fallback; one quiet status line says which and why.
describe("WEB-U-025 route fallback line", () => {
  const fallback = (seq: number) =>
    frame(seq, {
      type: "route.fallback",
      from: "primary",
      to: "backup",
      reason: "unavailable",
    });
  const frames = [
    frame(1, { type: "text.delta", text: "first" }),
    fallback(2),
    frame(3, { type: "text.delta", text: " second" }),
  ] as ServerFrame[];

  it("adds one quiet line naming both routes and keeps the following events", () => {
    const state = replay(frames);
    expect(state.lastSeq).toBe(3);
    expect(state.banner).toBeNull();
    expect(
      state.items.flatMap((item) =>
        item.kind === "notice" ? [noticeText(item.copy)] : [],
      ),
    ).toEqual(["Switched to backup (primary unavailable)"]);
  });

  it("replays and dedups to the same transcript", () => {
    const once = replay(frames);
    expect(replay(frames.flatMap((item) => [item, item]))).toEqual(once);
  });
});

describe("WEB-U-042 a full workspace", () => {
  it("renders workspace.full as a quiet notice that leaves the run as it was", () => {
    const before = apply(
      emptyTranscript(),
      frame(1, { type: "run.started", run_id: runId }),
    );
    const full = frame(2, { type: "workspace.full", call_id: "call-7" });
    const state = apply(before, full);
    expect(said(state.items.at(-1))).toBe(
      "The workspace is full. Free some space (delete files you no longer need) and continue.",
    );
    expect(state.run).toEqual(before.run);
    expect(state.banner).toBeNull();
    expect(apply(state, full)).toEqual(state);
  });
});

// Each run's context report is kept for the context panel, never as a transcript item (WEB-U-046, FR-11).
describe("WEB-U-046 context reports", () => {
  const started = (seq: number, run: string): ServerFrame => ({
    ...frame(seq, { type: "run.started", run_id: run }),
    run_id: run,
  });
  const loaded = (
    seq: number,
    run: string | null,
    event: Event,
  ): ServerFrame => ({
    ...frame(seq, event),
    run_id: run,
  });

  it("files each report under its run, and under the current run when the frame names none", () => {
    const state = replay([
      ready,
      started(1, "run-1"),
      loaded(2, "run-1", contextLoaded),
      started(3, "run-2"),
      loaded(4, null, laterContext),
    ]);
    expect(state.context).toEqual({
      "run-1": {
        instructions: contextLoaded.instructions,
        skills: contextLoaded.skills,
        skipped: contextLoaded.skipped,
      },
      "run-2": {
        instructions: laterContext.instructions,
        skills: [],
        skipped: [],
      },
    });
    expect(state.items).toEqual([]);
  });

  it("replaces a run's report when the resumed run reports again, and replays to the same state", () => {
    const frames = [
      ready,
      started(1, "run-1"),
      loaded(2, "run-1", contextLoaded),
      {
        ...frame(3, { type: "run.resumed", run_id: "run-1", from_step: 1 }),
        run_id: "run-1",
      },
      loaded(4, "run-1", laterContext),
    ];
    const state = replay(frames);
    expect(state.context["run-1"]?.skills).toEqual([]);
    expect(replay(frames.flatMap((item) => [item, item]))).toEqual(state);
  });
});
