import { randomUUID } from "node:crypto";
import { appendFileSync } from "node:fs";

import { admissionStatus, advanceQueue, queueMessage } from "./admission.mjs";
import { withContext } from "./context.mjs";
import { driftLog } from "./contract.mjs";
import { agentWork, snapshot } from "./files.mjs";
import {
  frame,
  messagesFrames,
  ready,
  templates,
  transcriptFrames,
  validFrame,
} from "./fixtures.mjs";
import { longTranscriptFrames } from "./long-transcript.mjs";
import { markdownFrames, tableAnswer } from "./markdown.mjs";
import {
  DESKTOP_GAPS_RUN,
  DESKTOP_RUN,
  scriptedFrames,
  scriptedRun,
  withDesktop,
  desktopFrames,
  desktopGapsFrames,
} from "./screen.mjs";
import { SLOW_RUN, SLOW_TICK_MS, slowRunEvents } from "./slow-run.mjs";

/** Marks a frame a test sends outside the schema on purpose (a newer gateway's code); never serialized. */
export const FORWARD = Symbol("forward-compatible frame");

/**
 * Checks a frame against the real schema. An invalid frame is drift, recorded for the e2e teardown to fail on, never a
 * crash of the whole fake front: one test's bad frame used to take every later spec down with it.
 */
function checked(value) {
  if (value.event?.[FORWARD]) return;
  try {
    validFrame(value);
  } catch (error) {
    console.error(`Fake front drifted from the protocol: ${error.message}`);
    appendFileSync(driftLog(3100), `${error.message}\n`);
  }
}

function write(response, value) {
  checked(value);
  response.write(
    `${"seq" in value && value.seq != null ? `id: ${value.seq}\n` : ""}data: ${JSON.stringify(value)}\n\n`,
  );
}

function sessionFrame(conversation) {
  const admission = admissionStatus(conversation);
  return {
    ...ready,
    conversation_id: conversation.id,
    sandbox: {
      ...ready.sandbox,
      state:
        conversation.recovery?.state ??
        (conversation.failure
          ? "failed"
          : (admission?.state ??
            (conversation.withdrawn
              ? "destroyed"
              : conversation.running
                ? "busy"
                : "idle"))),
      ...(conversation.recovery
        ? { recovery: conversation.recovery.recovery }
        : {}),
    },
    ...(admission?.queue ? { queue: admission.queue } : {}),
  };
}

function publishSession(conversation) {
  for (const response of conversation.streams)
    write(response, sessionFrame(conversation));
}

export function subscribe(request, response, conversation, url) {
  response.writeHead(200, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    "x-accel-buffering": "no",
  });
  write(response, sessionFrame(conversation));
  if (conversation.failure) write(response, conversation.failure);
  const after = Number(
    request.headers["last-event-id"] ?? url.searchParams.get("after") ?? 0,
  );
  for (const value of conversation.history)
    if (value.seq > after) write(response, value);
  write(response, sessionFrame(conversation));
  conversation.streams.add(response);
  const heartbeat = setInterval(
    () => response.write(": keepalive\n\n"),
    15_000,
  );
  const deadline = setTimeout(() => response.end(), 120_000);
  response.on("close", () => {
    clearInterval(heartbeat);
    clearTimeout(deadline);
    conversation.streams.delete(response);
  });
}

/**
 * The gateway restarting, as its shutdown does: an `error` frame with code `restarting`, no words for the page, to each
 * open stream, then the stream ends; the page reconnects (WEB-U-234).
 */
export function restartStreams(conversation) {
  for (const response of [...conversation.streams]) {
    write(response, {
      type: "error",
      code: "restarting",
      message: "",
      retryable: true,
    });
    response.end();
    // Gone at once, as a restarted gateway holds no old stream: a frame stored right after (askAgain's retry) is for
    // the page's reconnect, and writing it to an ended stream would throw and take the fake front down.
    conversation.streams.delete(response);
  }
}

/**
 * After a restart under a step, as the runtime does once its link is back (CORE-U-062): the model call was lost with the
 * gateway, so the step's attempt is given back as a `provider.retry` with reason `gateway_link` (first-hour F7). Stored
 * in the history, so the page's reconnect replays it.
 */
export function askAgain(conversation, delayMs) {
  const step = conversation.history.findLast(
    (value) => value.event?.type === "step.started",
  );
  if (!step) return;
  emit(conversation, {
    ...step,
    seq: conversation.history.at(-1).seq + 1,
    event: {
      type: "provider.retry",
      attempt: step.event.attempt,
      delay_ms: delayMs,
      reason: "gateway_link",
    },
  });
}

/** An unsequenced frame the gateway never stores (team.changed, ADR-0034 S6): to the open streams only, no replay. */
export function notify(conversation, value) {
  for (const response of conversation.streams) write(response, value);
}

export function emit(conversation, value) {
  checked(value);
  conversation.history.push(value);
  const ended =
    value.type === "event" &&
    ["run.completed", "run.cancelled", "run.failed"].includes(value.event.type);
  if (ended) {
    conversation.running = false;
    conversation.last_activity_at = new Date().toISOString();
  }
  for (const response of conversation.streams) write(response, value);
  if (ended) releaseHeld(conversation, value.event.type);
}

/**
 * As the gateway (GW-I-129): a message sent while a run is active is held, and the stream says so; returns whether it
 * was held.
 */
export function holdMessage(conversation, id, text, attachments, author) {
  if (!conversation.running) return false;
  (conversation.held ??= []).push({
    id,
    text,
    attachments,
    author,
    created_at: new Date().toISOString(),
    state: "held",
  });
  publishHeld(conversation);
  return true;
}

/** The unsequenced frame that says the held messages changed; never recorded in history. */
export function publishHeld(conversation, sent) {
  const messages = conversation.held ?? [];
  const value = {
    type: "message.queue",
    held: messages.filter((m) => m.state === "held").map((m) => m.id),
    dropped: messages.filter((m) => m.state === "dropped").map((m) => m.id),
    ...(sent ? { sent } : {}),
  };
  for (const response of conversation.streams) write(response, value);
}

/** A run that completed sends every held message as one turn; a failed or stopped one drops them. */
function releaseHeld(conversation, ended) {
  const waiting = (conversation.held ?? []).filter((m) => m.state === "held");
  if (!waiting.length) return;
  if (ended !== "run.completed") {
    for (const m of waiting)
      Object.assign(m, {
        state: "dropped",
        reason: ended === "run.cancelled" ? "run_cancelled" : "run_failed",
      });
    publishHeld(conversation);
    return;
  }
  const at = new Date().toISOString();
  for (const m of waiting) Object.assign(m, { state: "sent", sent_at: at });
  publishHeld(
    conversation,
    waiting.map((m) => m.id),
  );
  // As the gateway starts the joined turn as a new run, once the finished run's player has stopped: it may still have
  // frames to play after run.completed, and its last tick would stop the new run's player.
  const start = () =>
    conversation.timer
      ? setTimeout(start, 20)
      : begin(
          conversation,
          waiting.map((m) => m.text).join("\n\n"),
          waiting.flatMap((m) => m.attachments),
        );
  setTimeout(start);
}

/** Fake-only: a sandbox loss as the gateway's worker reports it (WEB-U-068), on every open stream. */
export function loseSandbox(conversation, state, losses, retry) {
  conversation.recovery = {
    state,
    recovery: { losses, retry_in_secs: retry },
  };
  publishSession(conversation);
}

export function begin(conversation, text, attachments = [], message = null) {
  conversation.attachments = attachments;
  // A message brings the task back; the replaced sandbox's session.ready carries no recovery.
  conversation.recovery = null;
  conversation.withdrawn = false;
  if (queueMessage(conversation, text)) {
    // As the gateway (GW-I-173): a message waiting for a slot is listed, so any viewer can cancel it.
    if (message)
      (conversation.held ??= []).push({
        ...message,
        text,
        attachments,
        created_at: new Date().toISOString(),
        state: "waiting_for_slot",
      });
    publishSession(conversation);
    return;
  }
  beginRun(conversation, text);
}

/** Fake-only admission control publishes transient frames without touching recorded history. */
export function advanceAdmission(conversation) {
  if (!conversation.admission) return false;
  const text = advanceQueue(conversation);
  publishSession(conversation);
  if (text !== null) {
    claimWaiting(conversation);
    beginRun(conversation, text);
  }
  return true;
}

/** The waiting message goes out (GW-I-173): it is listed as sent, and a Cancel now answers 409. */
export function claimWaiting(conversation) {
  const at = new Date().toISOString();
  for (const m of conversation.held ?? [])
    if (m.state === "waiting_for_slot")
      Object.assign(m, { state: "sent", sent_at: at });
}

/**
 * As the gateway (GW-I-173): a cancelled waiting message withdraws the task from the slot queue. Open streams hear
 * message.queue, then, when no slot was granted, session.ready destroyed with no queue; a sandbox already starting
 * goes on to idle.
 */
export function withdrawWaiting(conversation) {
  const granted = admissionStatus(conversation)?.state === "creating";
  conversation.admission = null;
  conversation.running = false;
  conversation.withdrawn = !granted;
  publishHeld(conversation);
  publishSession(conversation);
}

function beginRun(conversation, text) {
  if (text === "Fail before starting" && !conversation.failedOnce) {
    conversation.failedOnce = true;
    conversation.failure = {
      type: "error",
      code: "runtime_start_failed",
      retryable: true,
      message: "Internal sandbox startup detail.",
    };
    for (const response of conversation.streams)
      write(response, conversation.failure);
    return;
  }
  conversation.failure = null;
  conversation.running = true;
  conversation.last_activity_at = new Date().toISOString();
  conversation.run = `run-${++conversation.runs}`;
  // As the gateway does at delivery: record the workspace before the run can write, then let the fake agent work.
  snapshot(conversation, conversation.run);
  agentWork(conversation, text, conversation.attachments ?? []);
  if (conversation.runtimeTemplate !== conversation.template) {
    emit(conversation, {
      ...frame(conversation.history.length + 1, {
        type: "environment.changed",
        template: conversation.template,
        // The person's environments as the page reads them: the Desktop is one in the "desktop" scenario.
        capabilities: withDesktop(templates).items.find(
          (value) => value.name === conversation.template,
        ).capabilities,
      }),
      run_id: null,
      step: null,
      ts: new Date().toISOString(),
    });
    conversation.runtimeTemplate = conversation.template;
  }
  // PERF-040 / WEB-E-009: record a 10,000-event history at once, so the next page load replays it cold.
  if (text === "Load the long transcript") {
    const offset = conversation.history.length;
    for (const value of longTranscriptFrames())
      emit(conversation, { ...value, seq: offset + value.seq });
    return;
  }
  // ADR-0039: a scripted desktop run, its actions and their pictures (WEB-E2E-037's replay), or a run of scripted calls
  // (the suggestion card, the per-run caps, a run left open).
  if (text === DESKTOP_RUN || text === DESKTOP_GAPS_RUN || scriptedRun(text)) {
    const offset = conversation.history.length;
    const frames =
      text === DESKTOP_RUN
        ? desktopFrames
        : text === DESKTOP_GAPS_RUN
          ? desktopGapsFrames
          : scriptedFrames;
    for (const value of frames(conversation.run, text, frame))
      emit(conversation, { ...value, seq: offset + value.seq });
    return;
  }
  if (text === SLOW_RUN) {
    playSlowRun(conversation, text);
    return;
  }
  const start = conversation.history.length;
  const startedAt = Date.now();
  const markdown = text.startsWith("Stream Markdown");
  const values = withContext(
    text,
    (markdown
      ? markdownFrames
      : text === "Create a hello project and show it"
        ? messagesFrames
        : transcriptFrames
    )
      .filter((value) => value.type === "event")
      .map((value, index, frames) => {
        const event = { ...value.event };
        if (event.type === "message.user") {
          event.text = text;
          event.attachments = conversation.attachments ?? [];
        }
        if ("run_id" in event) event.run_id = conversation.run;
        if (
          text === "Fail the command" &&
          ["tool.end", "tool.result"].includes(event.type)
        ) {
          event.status = "error";
          if (event.type === "tool.end") event.exit_code = 2;
          else event.content = "Command could not complete.";
        }
        // WEB-U-016: a command the shell couldn't find (exit 127).
        if (
          text === "Run a missing command" &&
          ["tool.end", "tool.result"].includes(event.type)
        ) {
          event.status = "error";
          if (event.type === "tool.end") event.exit_code = 127;
          else event.content = "sh: 1: ls: not found";
        }
        // WEB-U-094: the run's first tool call is a push that fails with the named code ("Fail the save with
        // key_use_unrecorded"), for the step's plain words.
        const failedSave = /^Fail the save with ([a-z_]+)$/.exec(text);
        if (failedSave && event.call_id === firstCall(frames)) {
          if (event.type === "tool.call")
            Object.assign(event, {
              name: "git_push",
              arguments: {
                remote: "https://git.example.invalid/team/demo.git",
                branch: "feature/readme",
              },
            });
          if (event.type === "tool.end") event.status = "error";
          // As the runtime does (WEB-U-186): the failure's text streams as the call's output, then is its result.
          if (event.type === "tool.output")
            event.data = `${failedSave[1]}: the push tool's own detail`;
          if (event.type === "tool.result")
            Object.assign(event, {
              status: "error",
              content: `${failedSave[1]}: the push tool's own detail`,
            });
        }
        // WEB-U-327: the run's first call is the lead's read of its team, answered with the runtime's marked block
        // for the model (a person's step-in), as tqa-tools writes it; the page says how many messages, never the block.
        if (text === TEAM_READ && event.call_id === firstCall(frames)) {
          if (event.type === "tool.call")
            Object.assign(event, { name: "read_reports", arguments: {} });
          if (event.type === "tool.output") event.data = TEAM_READ_BLOCK;
          if (event.type === "tool.result")
            Object.assign(event, { status: "ok", content: TEAM_READ_BLOCK });
        }
        if (text === HOLD_START && event.call_id === firstCall(frames)) {
          if (event.type === "tool.call")
            Object.assign(event, {
              name: "start_member",
              arguments: {
                role: "researcher",
                workspace: "own_copy",
                brief: { objective: "Collect the Q3 figures for each region." },
              },
            });
          if (event.type === "tool.output") event.data = "";
          if (event.type === "tool.result")
            Object.assign(event, {
              status: "ok",
              content: "Started researcher.",
            });
        }
        if (text === "Fetch a blocked page") blockedFetch(event);
        if (text === ADD_TEST) testWrite(event);
        if (text === EDIT_TEST) testEdit(event);
        if (
          text === "Show the files table" &&
          event.type === "text.delta" &&
          event.text.startsWith("The workspace contains")
        )
          event.text = tableAnswer;
        // WEB-U-135: a model (qwen) wrapping its answer's closing paragraph in <result>, and quoting one in code.
        if (
          text === WRAPPED &&
          event.type === "text.delta" &&
          event.text.startsWith("The workspace contains")
        )
          event.text =
            "Here is the tag the report uses:\n\n```xml\n<note>draft</note>\n```\n\n<result>The workspace holds a Rust project.</result>";
        // A provider refusing the request (WEB-U-055): the run fails with its sanitised words.
        if (text === "Fail at the provider" && event.type === "run.completed")
          Object.assign(event, {
            type: "run.failed",
            code: "provider_error",
            reason: "provider_error 400",
            provider_message:
              "Invalid request: the model 'gpt-x' does not exist or you do not have access to it.",
          });
        // WEB-U-016: any failure the runtime can send, by its code and recorded reason ("Fail with budget
        // budget.output_tokens"), so every one has an end-to-end test of its plain words.
        const failWith = /^Fail with ([a-z_]+)(?: (.+))?$/.exec(text);
        if (failWith && event.type === "run.completed") {
          Object.assign(event, {
            type: "run.failed",
            code: failWith[1],
            reason: failWith[2] ?? failWith[1],
          });
          // A code the schema doesn't know yet is the point of that test (a newer gateway's failure): not drift.
          Object.defineProperty(event, FORWARD, { value: true });
        }
        // WEB-U-016: a model server refusing too long an input, in llama.cpp's words.
        if (text === "Send too long an input" && event.type === "run.completed")
          Object.assign(event, {
            type: "run.failed",
            code: "provider_error",
            reason: "provider_error 400",
            provider_message:
              "the request exceeds the available context size, try increasing it",
          });
        if (text === "Fail the command" && event.type === "run.completed")
          Object.assign(event, {
            type: "run.failed",
            code: "io",
            reason: "Command could not complete",
          });
        return {
          ...value,
          ts: new Date(
            startedAt + Date.parse(value.ts) - Date.parse(frames[0].ts),
          ).toISOString(),
          seq: start + value.seq,
          run_id: conversation.run,
          event,
        };
      }),
  );
  // WEB-U-093: the task's decision engine stops answering twice in one run; the agent decides by itself, and the page
  // says so once.
  if (text === "Decide without the engine") {
    const at = values.findIndex((value) => value.event.type === "step.started");
    const first = values[at];
    const fallbacks = ["unreachable", "timeout"].map((reason) => ({
      ...first,
      event: {
        type: "decision.fallback",
        engine: "house-rules",
        point: "before_step",
        reason,
      },
    }));
    values.splice(at + 1, 0, ...fallbacks);
    for (const [index, value] of values.entries())
      value.seq = start + index + 1;
  }
  // WEB-U-016: two model retries in the first step that the run recovers from, for the step's retry line.
  if (text === "Retry, then answer") {
    const at = values.findIndex((value) => value.event.type === "step.started");
    const first = values[at];
    const retries = [
      { reason: "timeout", delay_ms: 1203 },
      { reason: "fallback_parse: schema", delay_ms: 800 },
    ].map((retry, attempt) => ({
      ...first,
      event: { type: "provider.retry", attempt: attempt + 1, ...retry },
    }));
    values.splice(at + 1, 0, ...retries);
    for (const [index, value] of values.entries())
      value.seq = start + index + 1;
  }
  // WEB-U-123: a model that thinks for long, out loud; the run stays in its first step until the test ends.
  if (text === THINK) {
    const at = values.findIndex((value) => value.event.type === "step.started");
    values.splice(at + 1, values.length, {
      ...values[at],
      event: {
        type: "reasoning.delta",
        text: "The totals come from three sheets; I'll compare them first.",
      },
    });
    for (const [index, value] of values.entries())
      value.seq = start + index + 1;
  }
  // A refused call never ran, and a file write has no output: their calls keep only the call and its result.
  if (text === ADD_TEST || text === EDIT_TEST) {
    for (let at = values.length - 1; at >= 0; at--)
      if (
        ["tool.start", "tool.output", "tool.end"].includes(
          values[at].event.type,
        )
      )
        values.splice(at, 1);
    for (const [index, value] of values.entries())
      value.seq = start + index + 1;
  }
  // A bounded real interval exercises browser rendering while the response is still streaming.
  let index = 0;
  const play = () => {
    conversation.timer = setInterval(
      () => {
        const value = values[index++];
        if (value) emit(conversation, value);
        // HOLD: pauses right after the narration, so a test can check the live run before it ends; resume() goes on.
        if (
          (text === HOLD ||
            text === HOLD_START ||
            conversation.holdNarration) &&
          value?.event.type === "text.delta" &&
          value.event.text.startsWith("I’ll check")
        ) {
          clearInterval(conversation.timer);
          conversation.timer = null;
          conversation.holdNarration = false;
          conversation.resume = play;
          return;
        }
        if (text === "Fetch a blocked page" && value?.event.type === "tool.end")
          denyEgress(conversation);
        if (text === "Call keyed services" && value?.event.type === "tool.end")
          keyedCalls(conversation);
        if (
          index === values.length ||
          (text === "Wait for cancellation" && index === 5) ||
          (text === "Hold activity" && index === 8)
        ) {
          clearInterval(conversation.timer);
          conversation.timer = null;
        }
      },
      markdown ? 25 : 80,
    );
  };
  play();
}

/**
 * The scripted slow model's long scenario (slow-run.mjs): one frame per chunk, each stamped as it goes out, so its
 * calls take their real time. While the test's `fixture_hold=steps` cookie was set, it holds at each of its marks until
 * fixture-resume.
 */
function playSlowRun(conversation, text) {
  const events = slowRunEvents(text, conversation.run);
  let index = 0;
  const play = () => {
    conversation.timer = setInterval(() => {
      const entry = events[index++];
      if (entry) {
        const [event, step, hold] = entry;
        emit(conversation, {
          type: "event",
          seq: conversation.history.length + 1,
          run_id: conversation.run,
          step,
          ts: new Date().toISOString(),
          event,
        });
        if (hold && conversation.holdSteps && index < events.length) {
          clearInterval(conversation.timer);
          conversation.timer = null;
          conversation.resume = play;
          return;
        }
      }
      if (index >= events.length) {
        clearInterval(conversation.timer);
        conversation.timer = null;
      }
    }, SLOW_TICK_MS);
  };
  play();
}

/** Continues a run held after its narration (HOLD); a run that is not held is left alone. */
export function resume(conversation) {
  const play = conversation.resume;
  conversation.resume = null;
  if (play && !conversation.timer) play();
}

/** The message whose answer ends in a <result> wrapper and quotes a tag in code (WEB-U-135). */
export const WRAPPED = "Answer in a result tag.";
/** WEB-U-327: a run whose first call reads the team's messages. */
export const TEAM_READ = "Check for reports";
const TEAM_READ_BLOCK =
  '<signed-in-person source="person" seq="5" type="person_message" from="your person" id="b332b4cc72cc104b">\n' +
  "Use only facts you can name a source for.\n" +
  '</signed-in-person id="b332b4cc72cc104b">';

/** The message whose run thinks, and goes on thinking, in its first step (WEB-U-123). */
export const THINK = "Think it over for a while.";

/** The message whose run pauses after its narration until the test resumes it (conversation.spec, WEB-E2E-004). */
export const HOLD = "Summarize the workspace, then wait.";
/**
 * HOLD's run whose first call is the lead's start_member, answered ok (WEB-U-416): a test starts the team while it is
 * held, as the gateway does before the call's result.
 */
export const HOLD_START = "Start a researcher, then wait.";

const blockedTarget = { host: "example.invalid", port: 443 };
// The tool's own words for the model (C1); the page reads the failure on tool.end, never these words.
const deniedResult =
  "example.invalid isn't on this task's allowed sites. An administrator can allow it in Settings > Internet access.";

/** The message whose run adds a test file (plan §3, Done: the Run all the tests follow-up). */
/** The first tool call's id in a scripted run. */
const firstCall = (frames) =>
  frames.find((value) => value.event?.type === "tool.call")?.event.call_id;

export const ADD_TEST = "Add a test for the parser";

function testWrite(event) {
  if (event.type === "tool.call")
    Object.assign(event, {
      name: "write_file",
      arguments: {
        path: "tests/test_parser.py",
        content: "def test_parse(): ...\n",
      },
    });
  if (event.type === "tool.result")
    Object.assign(event, {
      status: "ok",
      content: "created, 23 bytes\nadded: 1 lines\nremoved: 0 lines",
    });
}

/** The message whose run edits that test file: the footer says Updated, as its step line does (WEB-U-016). */
export const EDIT_TEST = "Change the parser test";

function testEdit(event) {
  if (event.type === "tool.call")
    Object.assign(event, {
      name: "edit_file",
      arguments: {
        path: "tests/test_parser.py",
        edits: [
          {
            old_text: "...",
            new_text: "assert parse('1') == 1",
            replace_all: null,
          },
        ],
      },
    });
  if (event.type === "tool.result")
    Object.assign(event, {
      status: "ok",
      content: "edited tests/test_parser.py: 1 edits, +1 -1 lines",
    });
}

/** Turns the scripted tool call into a fetch that the policy proxy refused. */
function blockedFetch(event) {
  if (event.type === "tool.call")
    Object.assign(event, {
      name: "fetch_url",
      arguments: { url: "https://example.invalid/data" },
    });
  if (event.type === "tool.output") event.data = deniedResult;
  if (event.type === "tool.end")
    Object.assign(event, {
      status: "error",
      exit_code: null,
      failure: { code: "egress_denied", target: blockedTarget },
    });
  if (event.type === "tool.result")
    Object.assign(event, { status: "error", content: deniedResult });
}

/** The gateway's live-only refusal frame, sent twice to prove the per-minute limit; never stored. */
function denyEgress(conversation) {
  const value = {
    type: "error",
    code: "egress_denied",
    retryable: false,
    message: "Internal proxy diagnostic.",
    target: blockedTarget,
  };
  checked(value);
  for (let repeat = 0; repeat < 2; repeat += 1)
    for (const response of conversation.streams) write(response, value);
}

/**
 * The gateway's live notices of keyed calls during the run's command (ADR-0037 Amendment 1): a key used, refusals, and
 * a key the task used that isn't set up. Sent while that step runs (no step named: the run's latest); the uses are kept as the gateway's audit keeps them.
 */
function keyedCalls(conversation) {
  const notice = (extra) => ({
    type: "host_key.used",
    run_id: conversation.run,
    step: null,
    sender_id: conversation.owner_id,
    name: "GITHUB_WORK",
    host: "api.github.com",
    port: 443,
    ...extra,
  });
  for (const value of [
    notice({}),
    notice({ type: "host_key.refused", outcome: "expired" }),
    notice({ type: "host_key.refused", outcome: "unavailable" }),
    notice({
      type: "host_key.refused",
      outcome: "absent",
      name: null,
      host: "git.acme.lan",
      port: 8443,
    }),
  ]) {
    checked(value);
    for (const response of conversation.streams) write(response, value);
    // As the gateway's audit, for a page opened later (GET …/key-uses): each use a row; a key that isn't set up has
    // no key, so it makes none.
    if (value.outcome === "absent") continue;
    const at = new Date().toISOString();
    (conversation.keyUses ??= []).push({
      id: randomUUID(),
      run_id: value.run_id,
      sender_id: value.sender_id,
      name: value.name,
      host: value.host,
      port: value.port,
      outcome: value.type === "host_key.used" ? "ok" : value.outcome,
      count: 1,
      first_at: at,
      last_at: at,
    });
  }
}

export function cancel(conversation) {
  clearInterval(conversation.timer);
  conversation.timer = null;
  emit(conversation, {
    ...frame(conversation.history.length + 1, {
      type: "run.cancelled",
      run_id: conversation.run,
    }),
    run_id: conversation.run,
    ts: new Date().toISOString(),
  });
}

export function close(conversation) {
  clearInterval(conversation.timer);
  conversation.timer = null;
  for (const response of conversation.streams) response.end();
}
