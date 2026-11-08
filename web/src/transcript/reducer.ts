import type { BudgetKind, Event } from "../protocol/generated/protocol";
import type { ParsedFrame } from "../protocol/validate";

import { actionFailure, quotaKey } from "./action-failure";
import { blockedAccess, destination, EGRESS_REPEAT_MS } from "./egress";
import { failureCopy, gatewayCode } from "./failure-copy";
import { message, type Copy } from "./notice-copy";
import { recordRun, recordUsage } from "./runs";
import {
  blankStep,
  duration,
  updateStep,
  updateTool,
  type EventFrame,
} from "./steps";
import type { KeyUseView, Transcript } from "./types";

export class SequenceGap extends Error {
  constructor(readonly after: number) {
    super("The transcript needs to reconnect."); // copy-lint: not user copy
  }
}

export function emptyTranscript(): Transcript {
  return {
    lastSeq: 0,
    heldVersion: 0,
    items: [],
    sandbox: "destroyed",
    queue: null,
    recovery: null,
    run: { id: null, status: "idle" },
    runs: {},
    context: {},
    usage: { prompt: 0, completion: 0, cached: 0, reasoning: 0 },
    banner: null,
    bannerEndsOnReconnect: false,
    title: null,
    team: null,
    screenNotices: [],
    actionFailure: null,
    egress: {},
    fellBack: [],
    clockOffset: null,
  };
}

function never(value: never): never {
  // copy-lint: not user copy (internal diagnostic)
  throw new Error(`Unhandled protocol variant: ${JSON.stringify(value)}`);
}

/** Applies one validated frame without mutating any previously published snapshot. */
export function apply(state: Transcript, frame: ParsedFrame): Transcript {
  const seq = "seq" in frame ? frame.seq : null;
  if (seq != null) {
    if (!Number.isSafeInteger(seq) || seq < 0)
      // copy-lint: not user copy (internal diagnostic)
      throw new Error("The event sequence cannot be represented safely.");
    if (seq <= state.lastSeq) return state;
    if (seq !== state.lastSeq + 1) throw new SequenceGap(state.lastSeq);
    state = { ...state, lastSeq: seq };
  }
  switch (frame.type) {
    case "heartbeat":
    // A runtime-link reply the gateway never forwards to clients (revision 7, ADR-0030).
    case "workspace.reply":
    // Runtime-link frames of the Desktop the gateway never forwards to clients (revision 14, ADR-0039).
    case "screen.upload":
    case "screen.sign_out.done":
    case "screen.reset":
    // An unknown frame or event kind only moves the cursor.
    case "ignored":
      return state;
    // The task's held messages changed (GW-I-129): the composer reads them again; the transcript shows nothing.
    case "message.queue":
      return { ...state, heldVersion: state.heldVersion + 1 };
    // The gateway named the task (WEB-U-081): the header, the sidebar and the tab take it; the transcript shows nothing.
    case "task.titled":
      return { ...state, title: frame.title };
    // The task's agent team changed (ADR-0034 S6): the page reads what moved; the transcript shows nothing.
    case "team.changed":
      return {
        ...state,
        team: {
          id: frame.team,
          version: Math.max(frame.version, state.team?.version ?? 0),
          lastSeq: Math.max(frame.last_seq, state.team?.lastSeq ?? 0),
        },
      };
    case "session.ready":
      return {
        ...state,
        // ADR-0035: nothing parks any more; an old conversation's parked environment is left as it was.
        sandbox:
          frame.sandbox.state === "parked"
            ? state.sandbox
            : frame.sandbox.state,
        queue: frame.sandbox.state === "queued" ? (frame.queue ?? null) : null,
        recovery: frame.sandbox.recovery
          ? {
              losses: frame.sandbox.recovery.losses,
              seconds: frame.sandbox.recovery.retry_in_secs ?? null,
              from: null,
              at: null,
            }
          : null,
      };
    case "error":
      // A blocked network attempt is live-only and time-limited; applyAt shows it.
      if (frame.code === "egress_denied") return state;
      // The gateway is restarting: the stream's reconnect line says so, and nothing failed (WEB-U-234). A sequence
      // gap is mended by the store itself, which replays from the last frame it has (WEB-U-236).
      if (frame.code === "restarting" || frame.code === "seq_gap") return state;
      const quota = quotaKey(frame.code, frame.retryable);
      return {
        ...state,
        banner: quota ? message(quota) : { failure: actionFailure(frame.code) },
        bannerEndsOnReconnect: quota === null,
        actionFailure: [
          "sandbox_failed",
          "runtime_start_failed",
          "action_failed",
        ].includes(frame.code)
          ? {
              generation: (state.actionFailure?.generation ?? 0) + 1,
              code: frame.code,
            }
          : state.actionFailure,
      };
    // A keyed call the gateway let through or refused (ADR-0037): said on the step running at the time.
    case "host_key.used":
    case "host_key.refused":
      return keyUse(state, frame.run_id, frame.step ?? null, {
        sender: frame.sender_id,
        name: frame.name ?? null,
        host: frame.host,
        port: frame.port,
        outcome: frame.type === "host_key.used" ? "ok" : frame.outcome,
      });
    // ADR-0035: no approvals. A gateway from before it could still ask; nothing is shown, and nothing waits.
    case "permission.request":
      return state;
    // A screen notice (ADR-0039): the Screen panel places it in the transcript from its own state; nothing here yet.
    // The gateway's screen notice (ADR-0039 §6, PROTO-U-038): never sequenced; kept for the transcript's quiet lines.
    case "screen.notice":
      return {
        ...state,
        screenNotices: [
          ...state.screenNotices,
          {
            kind: frame.kind,
            at: frame.at,
            ...(frame.seconds != null ? { seconds: frame.seconds } : {}),
            ...(frame.ended != null ? { ended: frame.ended } : {}),
            ...(frame.reason != null ? { reason: frame.reason } : {}),
          },
        ],
      };
    // The runtime's answer to a sign-out goes to the gateway only, which turns it into a screen notice (ADR-0039 §6).
    case "screen.sign_out.done":
      return state;
    case "event":
      return applyEvent(state, frame);
    default:
      return never(frame);
  }
}

/** Applies a live frame received at `nowMs`; blocked network attempts become one quiet line per destination per minute. */
export function applyAt(
  state: Transcript,
  frame: ParsedFrame,
  nowMs: number,
): Transcript {
  const next = applyLive(state, frame, nowMs);
  // The soonest arrival is closest to the clocks' own difference; a replayed frame arrives late and says less.
  const stamp = frame.type === "event" ? Date.parse(frame.ts) : Number.NaN;
  if (!Number.isFinite(stamp)) return next;
  const offset = nowMs - stamp;
  return next.clockOffset === null || offset < next.clockOffset
    ? { ...next, clockOffset: offset }
    : next;
}

function applyLive(
  state: Transcript,
  frame: ParsedFrame,
  nowMs: number,
): Transcript {
  if (frame.type === "session.ready") {
    // The replacement's time on the reader's clock, so the status line counts down (WEB-U-068).
    const next = apply(state, frame);
    const seconds = next.recovery?.seconds;
    return next.recovery && seconds != null
      ? {
          ...next,
          recovery: {
            ...next.recovery,
            from: nowMs,
            at: nowMs + seconds * 1000,
          },
        }
      : next;
  }
  if (frame.type !== "error" || frame.code !== "egress_denied")
    return apply(state, frame);
  const key = frame.target ? destination(frame.target) : "";
  const last = state.egress[key];
  if (last !== undefined && nowMs - last < EGRESS_REPEAT_MS) return state;
  const recent = Object.fromEntries(
    Object.entries(state.egress).filter(
      ([, shown]) => nowMs - shown < EGRESS_REPEAT_MS,
    ),
  );
  return {
    ...state,
    egress: { ...recent, [key]: nowMs },
    items: [
      ...state.items,
      {
        kind: "notice",
        id: `egress-${state.lastSeq}-${state.items.length}`,
        copy: blockedAccess(frame.target),
      },
    ],
  };
}

/**
 * Onto the step it names, even when that step has ended, once per key, address and outcome (the gateway already limits
 * repeats to one a minute). A step not on this page shows nothing.
 */
function keyUse(
  state: Transcript,
  runId: string,
  step: number | null,
  use: KeyUseView,
): Transcript {
  // No step was running when the proxy saw the call (the gateway's null): the run's latest step says it.
  const index =
    step === null
      ? state.items.findLastIndex(
          (item) => item.kind === "step" && item.runId === runId,
        )
      : state.items.findIndex(
          (item) =>
            item.kind === "step" && item.id === JSON.stringify([runId, step]),
        );
  const item = state.items[index];
  if (item?.kind !== "step") return state;
  const keys = item.keys ?? [];
  if (
    keys.some(
      (seen) =>
        seen.name === use.name &&
        seen.host === use.host &&
        seen.port === use.port &&
        seen.outcome === use.outcome,
    )
  )
    return state;
  const items = [...state.items];
  items[index] = { ...item, keys: [...keys, use] };
  return { ...state, items };
}

function notice(
  state: Transcript,
  copy: Copy,
  template?: string,
  provider?: string,
): Transcript {
  return {
    ...state,
    items: [
      ...state.items,
      {
        kind: "notice",
        id: `notice-${state.lastSeq}`,
        copy,
        ...(template ? { template } : {}),
        ...(provider ? { provider } : {}),
      },
    ],
  };
}

function cloneNotice(
  state: Transcript,
  event: Extract<Event, { type: "workspace.clone" }>,
): Transcript {
  switch (event.status) {
    case "started":
      return notice(state, message("clone.fetching"));
    case "completed":
      return notice(state, message("clone.fetched"));
    case "skipped":
      return state;
    case "failed":
      // The reason is chosen by stable code in the catalogue; the runtime's message is diagnostic and never shown.
      return notice(
        state,
        message("clone.fetchFailed", { reason: event.code ?? "unknown" }),
      );
    default:
      return never(event.status);
  }
}

function applyEvent(state: Transcript, frame: EventFrame): Transcript {
  const event = frame.event;
  switch (event.type) {
    case "message.user":
      return {
        ...state,
        banner: null,
        run: { ...state.run, status: "queued" },
        items: [
          ...state.items,
          {
            kind: "user",
            id: `user-${frame.seq}`,
            text: event.text,
            attachments: event.attachments,
            // A helper's first turn: its brief from the lead (WEB-U-223).
            ...(event.lead_brief
              ? {
                  brief: {
                    role: event.lead_brief.role,
                    objective: event.lead_brief.brief.objective,
                    output: event.lead_brief.brief.output,
                    sources: event.lead_brief.brief.sources ?? null,
                    boundaries: event.lead_brief.brief.boundaries,
                  },
                }
              : {}),
            // The gateway's wake (revision 13): its notice, not the person's message.
            ...(event.team_wake ? { wake: true as const } : {}),
          },
        ],
      };
    case "run.started":
      return { ...recordRun(state, frame, "running"), banner: null };
    case "run.resumed":
      return notice(
        { ...recordRun(state, frame, "running"), banner: null },
        message("notice.resumed"),
      );
    // ADR-0035: a run never parks to wait for a person.
    case "run.parked":
      return state;
    case "workspace.clone":
      return cloneNotice(state, event);
    case "run.completed":
      return recordRun(state, frame, "completed");
    case "run.cancelled":
      return recordRun(state, frame, "cancelled");
    case "run.failed":
      return notice(
        recordRun(state, frame, "failed"),
        failureCopy(event.code, event.reason, event.provider_message),
      );
    case "sandbox.state":
      // ADR-0035: an old conversation's replay may still say its environment parked; that changes nothing now.
      if (event.state === "parked") return state;
      return {
        ...state,
        sandbox: event.state,
        queue: null,
        // A running environment has recovered (WEB-U-068).
        recovery: ["busy", "ready", "idle"].includes(event.state)
          ? null
          : state.recovery,
      };
    case "environment.changed":
      return notice(
        state,
        message("notice.switched", { template: event.template }),
        event.template,
      );
    // ADR-0035: no approvals, so no decisions to show.
    case "permission.expired":
    case "permission.resolved":
      return state;
    case "step.started":
      return updateStep(state, frame, (step) => {
        // A new attempt starts the step's output over; its retries stay, since they're why it started over (WEB-U-016).
        const current =
          (event.attempt ?? 1) > step.attempt
            ? {
                ...blankStep(step.id, step.runId, step.step, event.attempt),
                retries: step.retries,
              }
            : step;
        // Nothing from the model yet in this attempt: how long it takes to start is worth saying (WEB-U-099).
        const quiet =
          !current.text && !current.reasoning && !current.toolOrder.length;
        const timed = {
          ...current,
          startedAt: frame.ts,
          firstStartedAt: step.firstStartedAt ?? frame.ts,
        };
        return quiet ? { ...timed, waitingSince: frame.ts } : timed;
      });
    case "step.completed":
      return updateStep(state, frame, (step) => ({
        ...step,
        durationMs: event.duration_ms ?? 0,
        waitingSince: null,
        thoughtUntil: step.thoughtUntil ?? frame.ts,
      }));
    case "model.completed":
    // Only the size of opaque reasoning state reaches clients; nothing of it is kept or shown (GW-I-051).
    case "reasoning.state":
    // An MCP server's state is for a future server panel (ADR-0011); the transcript shows nothing of it.
    case "mcp.server":
    // A decision engine's record is for a future decisions view (ADR-0013); the transcript shows nothing of it.
    case "decision":
      return state;
    // A desktop action's picture (revision 14, ADR-0039 §9): kept on its call, for its card and the replay.
    case "screen.capture":
      return updateTool(state, frame, event.call_id, (tool) => ({
        ...tool,
        captures: [
          ...(tool.captures ?? []).filter((each) => each.id !== event.id),
          {
            phase: event.phase,
            id: event.id,
            width: event.width,
            height: event.height,
            marked: event.marked,
          },
        ],
      }));
    // The engine stopped answering and the agent decides by itself; the run goes on. Said once per run, with why
    // (WEB-U-093); later fallbacks in the same run add nothing.
    case "decision.fallback": {
      const run = frame.run_id ?? state.run.id ?? "unassigned";
      if (state.fellBack.includes(run)) return state;
      return {
        ...notice(
          state,
          message("notice.decisionFallback", { reason: event.reason }),
        ),
        fellBack: [...state.fellBack, run],
      };
    }
    // What a run loaded is kept for the context panel (FR-11); the transcript shows nothing of it.
    case "context.loaded": {
      const { instructions, skills, skipped } = event;
      const run = frame.run_id ?? state.run.id ?? "unassigned";
      return {
        ...state,
        context: { ...state.context, [run]: { instructions, skills, skipped } },
      };
    }
    case "text.delta":
      return updateStep(state, frame, (step) => ({
        ...step,
        text: step.text + event.text,
        waitingSince: null,
        // Its first words end its thinking (17.3), as its first call does.
        thoughtUntil: step.thoughtUntil ?? frame.ts,
      }));
    case "reasoning.delta":
      return updateStep(state, frame, (step) => ({
        ...step,
        reasoning: step.reasoning + event.text,
        waitingSince: null,
      }));
    case "tool.call":
      return updateStep(state, frame, (step) => ({
        ...step,
        waitingSince: null,
        thoughtUntil: step.thoughtUntil ?? frame.ts,
        toolOrder: step.tools[event.call_id]
          ? step.toolOrder
          : [...step.toolOrder, event.call_id],
        tools: {
          ...step.tools,
          [event.call_id]: {
            id: event.call_id,
            name: event.name,
            arguments: event.arguments,
            status: "prepared",
            output: "",
            result: null,
            ran: false,
            startedAt: null,
            durationMs: null,
            exitCode: null,
          },
        },
      }));
    case "tool.start":
      return updateTool(state, frame, event.call_id, (tool) => ({
        ...tool,
        status: "running",
        ran: true,
        startedAt: frame.ts,
      }));
    case "tool.output":
      return updateTool(state, frame, event.call_id, (tool) => ({
        ...tool,
        output: tool.output + event.data,
      }));
    case "tool.end":
      return updateTool(state, frame, event.call_id, (tool) => ({
        ...tool,
        status: event.status,
        exitCode: event.exit_code ?? null,
        failure: event.failure ?? undefined,
        durationMs: duration(tool.startedAt, frame.ts),
      }));
    case "tool.result":
      return updateTool(state, frame, event.call_id, (tool) => ({
        ...tool,
        status: event.status,
        result: event.content,
        // A desktop action's element and whether its pictures were kept (revision 14, ADR-0039).
        ...(event.target ? { target: event.target } : {}),
        ...(event.evidence ? { evidence: event.evidence } : {}),
      }));
    case "usage":
      return recordUsage(state, frame);
    case "provider.retry":
      // A retry belongs to its step (WEB-U-016): said there in plain words, folded once the run is done.
      return updateStep(state, frame, (step) => ({
        ...step,
        retries: [
          ...step.retries,
          {
            reason: event.reason,
            delayMs: event.delay_ms,
            // The gateway's own refusal (GW-I-147) isn't the provider's words, so none are quoted.
            ...(event.provider_message && !gatewayCode(event.reason)
              ? { provider: event.provider_message }
              : {}),
          },
        ],
      }));
    case "budget.warning":
      return notice(state, budgetNotice(event.budget, event.used, event.limit));
    case "workspace.full":
      return notice(state, message("notice.workspaceFull"));
    case "compaction":
      return notice(state, message("notice.compacted"));
    case "route.fallback":
      return notice(
        state,
        message("notice.routeFallback", { from: event.from, to: event.to }),
      );
    default:
      return never(event);
  }
}

function budgetNotice(budget: BudgetKind, used: number, limit: number): Copy {
  switch (budget) {
    case "steps":
      return message("budget.steps", { used, limit });
    case "wall_clock":
      return message("budget.wallClock", { used, limit });
    // A reply's length as a share of the task's limit, not tokens; no progress in plain words.
    case "output_tokens":
      return message("budget.outputTokens", {
        percent: limit > 0 ? Math.round((used * 100) / limit) : 100,
      });
    case "no_progress":
      return message("budget.noProgress", { used });
    default:
      return never(budget);
  }
}
