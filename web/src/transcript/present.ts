import { english, type Translate } from "../i18n/translate";
import type { Event } from "../protocol/generated/protocol";

import { unwrapAnswer } from "./answer-wrapper";
import { failureReason } from "./failure-copy";
import { noticeText } from "./notice-copy";
import { retryShort } from "./retry-copy";
import { NOTHING_READ, TEAM_READS } from "./team-read";
import { toolSummary } from "./tool-summary";
import { turnSummary } from "./turn-summary";
import type { FollowUp } from "./turn-summary";
import type {
  Item,
  KeyUseView,
  RetryView,
  RunStatus,
  StepView,
  ToolView,
  Transcript,
} from "./types";

export type Treatment =
  | "Answer"
  | "Prompt"
  | "Activity"
  | "Thinking"
  | "Status"
  | "Summary"
  | "Hidden";

/** Every generated event has exactly one treatment; additions require an explicit decision. */
export function eventTreatment(type: Event["type"]): Treatment {
  switch (type) {
    case "text.delta":
      return "Answer"; // copy-lint: not user copy
    case "message.user":
      return "Prompt"; // copy-lint: not user copy
    case "tool.call":
    case "tool.start":
    case "tool.output":
    case "tool.end":
    case "tool.result":
    case "provider.retry":
      return "Activity"; // copy-lint: not user copy
    case "reasoning.delta":
      return "Thinking"; // copy-lint: not user copy
    case "usage":
      return "Summary"; // copy-lint: not user copy
    case "run.started":
    case "run.completed":
    case "run.cancelled":
    case "run.failed":
    case "run.resumed":
    case "run.parked":
    case "workspace.clone":
    case "sandbox.state":
    case "environment.changed":
    case "permission.expired":
    case "permission.resolved":
    case "budget.warning":
    case "workspace.full":
    case "compaction":
    case "route.fallback":
    // WEB-U-093: said once per run as a notice.
    case "decision.fallback":
      return "Status"; // copy-lint: not user copy
    case "step.started":
    case "step.completed":
    case "model.completed":
    // Opaque reasoning state: clients get only its size, and it has no visible form (GW-I-051, ADR-0027).
    case "reasoning.state":
    // What the run loaded into the model context: hidden until the context panel (FR-11).
    case "context.loaded":
    // An MCP server's state: hidden until there is a server panel (ADR-0011).
    case "mcp.server":
    // A decision engine's record: hidden until there is a decisions view (ADR-0013).
    case "decision":
    // A desktop action's screenshot record: hidden until the action replay (ADR-0039 slice 5).
    case "screen.capture":
      return "Hidden"; // copy-lint: not user copy
    default: {
      const unhandled: never = type;
      return unhandled;
    }
  }
}

/** A tool's line. */
type Activity = {
  id: string;
  tool: ToolView;
  label: string;
};
export type WorkItem =
  | { kind: "narration"; id: string; text: string; streaming: boolean }
  /** A step's thinking once the model has stopped thinking, folded in its place: how long it took, null if unknown. */
  | { kind: "thought"; id: string; text: string; ms: number | null }
  | { kind: "tool"; id: string; activity: Activity }
  /** A keyed call the gateway saw during the step (ADR-0037), after the step's own commands. */
  | { kind: "key"; id: string; use: KeyUseView }
  /** Team reads in a row that found nothing, as one quiet line (WEB-U-238). */
  | { kind: "reads"; id: string; tool: string; count: number };
export type TurnView = {
  /** When its run started; null before it has one (the agent team's lines are placed by it, WEB-U-222). */
  startedAt: string | null;
  id: string;
  prompt: Extract<Item, { kind: "user" }> | null;
  status: RunStatus;
  activities: Activity[];
  work: WorkItem[];
  answer: { id: string; text: string; streaming: boolean }[];
  notices: Extract<Item, { kind: "notice" }>[];
  activityLabel: string;
  /** The turn's model retries, in order, from all its steps (WEB-U-016). */
  retries: RetryView[];
  /** Stored key uses of its run that couldn't be put on a step with confidence (ADR-0037), said once for the run. */
  runKeys: KeyUseView[];
  /** While the turn works: when the current model call started, if the model has sent nothing yet (WEB-U-099). */
  waitingSince: string | null;
  /** While the turn works: when its current step started, and what the model is thinking in it (WEB-U-123). */
  liveSince: string | null;
  liveThinking: string;
  /** While the turn works: the model is writing its words, so no live line speaks over them (17.3). */
  writing: boolean;
  /** While the turn works: the call running now, the latest when several run, whose step the live line names (17.3). */
  liveTool: ToolView | null;
  /** The reader's clock minus the frames' stamps (WEB-U-122), for live counts. */
  clockOffset: number | null;
  summary: string | null;
  /** What the footer leaves out (time, steps, commands, tokens), for its tooltip. */
  summaryDetails: string | null;
  /** How many files the turn changed. */
  changed: number;
  /** Whether the footer offers Open changes: files changed, and no named Open covers them (L8). */
  openChanges: boolean;
  /** Follow-ups that clearly follow from what happened (at most two), as copy keys. */
  followUps: readonly FollowUp[];
  /** The one file this turn wrote, for the footer's "Created …" or "Updated …" and Open. */
  wrote: string | null;
  /** Whether that file is new or was there before (the step line's own words). */
  wroteKind: "created" | "updated";
  outcome: {
    reason: string;
    next: string | null;
    provider: string | null;
    /** A decision engine's own reason, after the plain sentence (WEB-U-328). */
    engine: string | null;
    code: string | null;
  } | null;
};
type Turn = {
  id: string;
  runId: string | null;
  prompt: TurnView["prompt"];
  steps: StepView[];
  notices: TurnView["notices"];
};

/** Projects the authoritative reduced transcript into prompt, work, answer and summary turns. */
export function present(
  transcript: Transcript,
  t: Translate = english,
  changedFiles: Readonly<Record<string, readonly string[]>> = {},
): TurnView[] {
  const turns: Turn[] = [];
  let hasEnvironment = false;
  for (const item of transcript.items) {
    // Earlier work may have used the implicit environment without a change event.
    if (item.kind === "user" || item.kind === "step") hasEnvironment = true;
    if (item.kind === "notice" && item.template && !hasEnvironment) {
      hasEnvironment = true;
      continue;
    }
    let turn = turns.at(-1);
    if (
      !turn ||
      item.kind === "user" ||
      (item.kind === "notice" && item.template) ||
      (item.kind === "step" && turn.runId && turn.runId !== item.runId)
    ) {
      turn = {
        id: item.id,
        runId: null,
        prompt: null,
        steps: [],
        notices: [],
      };
      turns.push(turn);
    }
    if (item.kind === "user" && eventTreatment("message.user") === "Prompt") {
      turn.prompt = item;
      turn.runId = item.runId ?? null;
    } else if (item.kind === "step") {
      turn.steps.push(item);
      turn.runId = item.runId;
    } else if (item.kind === "notice") turn.notices.push(item);
  }
  return turns.map((turn) => presentTurn(turn, transcript, t, changedFiles));
}

function presentTurn(
  turn: Turn,
  transcript: Transcript,
  t: Translate,
  changedFiles: Readonly<Record<string, readonly string[]>>,
): TurnView {
  const run = turn.runId ? transcript.runs[turn.runId] : undefined;
  const status =
    run?.status ??
    (turn.runId === transcript.run.id ? transcript.run.status : "queued");
  const tools = turn.steps.flatMap((step) =>
    step.toolOrder.flatMap((id) => {
      const tool = step.tools[id];
      if (!tool) return [];
      return [
        {
          id: `${step.id}-${step.attempt}-${id}`,
          tool: terminalTool(tool, status),
        },
      ];
    }),
  );
  const activities =
    eventTreatment("tool.call") === "Activity"
      ? tools.map((entry) => ({ ...entry, label: toolSummary(entry.tool, t) }))
      : [];
  // Keep concurrent running work at the end without changing recorded call order within each state.
  activities.sort(
    (a, b) =>
      Number(a.tool.status === "running") - Number(b.tool.status === "running"),
  );
  const terminal =
    status === "completed" || status === "failed" || status === "cancelled";
  const last = turn.steps.at(-1);
  const answer =
    last &&
    last.toolOrder.length === 0 &&
    (last.durationMs !== null || terminal)
      ? last
      : null;
  const retries = turn.steps.flatMap((step) => step.retries);
  // The current step, while its model is still thinking (nothing written or called yet) or writing its words (17.3).
  const open = !terminal && last && last.durationMs === null ? last : null;
  const thinkingNow =
    open && !open.text && open.toolOrder.length === 0 ? open : null;
  const work = workItems(turn.steps, activities, answer, terminal, thinkingNow);
  // Calls start in their order, so the last running one is the one started last.
  const liveTool = terminal
    ? null
    : (activities.findLast(({ tool }) => tool.status === "running")?.tool ??
      null);
  const summary = turnSummary(
    run,
    turn.steps,
    tools.map(({ tool }) => tool),
    t,
    turn.runId ? changedFiles[turn.runId] : undefined,
  );
  return {
    id: turn.id,
    prompt: turn.prompt,
    status,
    startedAt: run?.startedAt ?? null,
    activities,
    work,
    runKeys: run?.keys ?? [],
    // While the model thinks, the current step's thinking is the live line's (WEB-U-123, 17.3); once it writes or calls,
    // the thinking folds in the step's place (its work item): never twice, never hidden.
    liveThinking:
      thinkingNow && eventTreatment("reasoning.delta") === "Thinking"
        ? thinkingNow.reasoning
        : "",
    writing: !!open?.text && open.toolOrder.length === 0,
    liveSince: terminal ? null : (last?.startedAt ?? null),
    liveTool,
    clockOffset: transcript.clockOffset,
    answer:
      eventTreatment("text.delta") === "Answer" && answer?.text
        ? [
            {
              id: `${answer.id}-${answer.attempt}`,
              // A model's <result>/<note> wrapper around its answer is not the reader's (C2, WEB-U-135).
              text: unwrapAnswer(answer.text),
              streaming: false,
            },
          ]
        : [],
    // A reason already shown, or a refusal a tool line already explains, is said once.
    notices: turn.notices.filter(
      (notice) =>
        noticeText(notice.copy, t) !== failureReason(run, t) &&
        !activities.some(
          (activity) => activity.label === noticeText(notice.copy, t),
        ),
    ),
    // Recovered retries are said on the step line ("… · the model was slow, retried twice").
    activityLabel: !terminal
      ? t("run.working")
      : status === "completed" && retries.length > 0
        ? `${summary.activityLabel} · ${t("activity.retriedShort", {
            reason: t(retryShort(retries.at(-1)?.reason ?? "")),
            count: retries.length,
          })}`
        : summary.activityLabel,
    retries,
    waitingSince: terminal ? null : (turn.steps.at(-1)?.waitingSince ?? null),
    summary: terminal ? summary.summary : null,
    summaryDetails: terminal ? summary.details : null,
    changed: terminal ? summary.files : 0,
    openChanges:
      terminal &&
      summary.files > 0 &&
      !summary.wrote &&
      !(status === "completed" && summary.covered),
    followUps: status === "completed" ? summary.followUps.slice(0, 2) : [],
    wrote: terminal ? summary.wrote : null,
    wroteKind: summary.wroteKind,
    outcome: terminal ? summary.outcome : null,
  };
}

// A terminal run also ends unfinished work, even when no final tool result was journalled.
function terminalTool(tool: ToolView, status: RunStatus): ToolView {
  return (status === "failed" || status === "cancelled") &&
    (tool.status === "running" || tool.status === "prepared")
    ? { ...tool, status: status === "cancelled" ? "cancelled" : "interrupted" }
    : tool;
}

function emptyRead(
  item: WorkItem,
): item is Extract<WorkItem, { kind: "tool" }> {
  if (item.kind !== "tool") return false;
  const { tool } = item.activity;
  return (
    TEAM_READS.has(tool.name) &&
    tool.status === "ok" &&
    NOTHING_READ.has((tool.result ?? tool.output).trim())
  );
}

/**
 * Two or more team reads in a row of the same tool that found nothing read as one line, "Checked for reports 3 times
 * · nothing new" (WEB-U-238): a lead waiting on its helpers otherwise fills its steps with the same empty line. A
 * single empty read, a read that found something, and a running one keep their own line.
 */
function foldEmptyReads(items: WorkItem[]): WorkItem[] {
  const out: WorkItem[] = [];
  for (let at = 0; at < items.length;) {
    const item = items[at];
    if (item === undefined) break;
    if (!emptyRead(item)) {
      out.push(item);
      at += 1;
      continue;
    }
    let end = at + 1;
    for (; end < items.length; end++) {
      const next = items[end];
      if (
        next === undefined ||
        !emptyRead(next) ||
        next.activity.tool.name !== item.activity.tool.name
      )
        break;
    }
    if (end - at > 1)
      out.push({
        kind: "reads",
        id: `${item.id}-reads`,
        tool: item.activity.tool.name,
        count: end - at,
      });
    else out.push(item);
    at = end;
  }
  return out;
}

/** How long a step's model thought: from its attempt's start to its first words, call or end; null if unknown. */
function thoughtMs(step: StepView): number | null {
  return step.thoughtUntil && step.startedAt
    ? Math.max(0, Date.parse(step.thoughtUntil) - Date.parse(step.startedAt))
    : null;
}

function workItems(
  steps: StepView[],
  activities: Activity[],
  answer: StepView | null,
  terminal: boolean,
  thinkingNow: StepView | null,
): WorkItem[] {
  const byId = new Map(activities.map((activity) => [activity.id, activity]));
  const items = steps.flatMap((step): WorkItem[] => {
    // Its thinking, folded where it was once the model stopped thinking (17.3); while it thinks, the live line has it.
    const thought: WorkItem[] =
      step.reasoning &&
      step !== thinkingNow &&
      eventTreatment("reasoning.delta") === "Thinking"
        ? [
            {
              kind: "thought",
              id: `${step.id}-${step.attempt}-thought`,
              text: step.reasoning,
              ms: thoughtMs(step),
            },
          ]
        : [];
    const narration: WorkItem[] =
      step.text && step !== answer
        ? [
            {
              kind: "narration",
              id: `${step.id}-${step.attempt}-text`,
              text: step.text,
              // Its words are done once the step's next call starts (17.3): the caret ends there.
              streaming:
                !terminal &&
                step.durationMs === null &&
                step.toolOrder.every(
                  (id) => (step.tools[id]?.status ?? "prepared") === "prepared",
                ),
            },
          ]
        : [];
    // A call is a line from the moment it starts (17.3): one only asked for, not started, has none yet.
    const stepTools = step.toolOrder.flatMap((id) => {
      const activity = byId.get(`${step.id}-${step.attempt}-${id}`);
      return activity && activity.tool.status !== "prepared"
        ? [
            {
              kind: "tool" as const,
              id: activity.id,
              activity,
            },
          ]
        : [];
    });
    stepTools.sort(
      (a, b) =>
        Number(a.activity.tool.status === "running") -
        Number(b.activity.tool.status === "running"),
    );
    const keys = (step.keys ?? []).map((use, index): WorkItem => ({
      kind: "key",
      id: `${step.id}-${step.attempt}-key-${index}`,
      use,
    }));
    return [...thought, ...narration, ...stepTools, ...keys];
  });
  return foldEmptyReads(items);
}
