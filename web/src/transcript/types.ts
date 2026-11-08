import type {
  Event,
  QueuePosition,
  SandboxState,
  CapturePhase,
  ScreenEvidence,
  ScreenTarget,
  ToolFailure,
  ToolStatus,
} from "../protocol/generated/protocol";
import type { HostKeyOutcome } from "../protocol/generated/protocol";

import type { Copy } from "./notice-copy";

export type ToolView = {
  id: string;
  name: string;
  arguments: unknown;
  status: "prepared" | "running" | ToolStatus;
  output: string;
  result: string | null;
  ran: boolean;
  startedAt: string | null;
  durationMs: number | null;
  exitCode: number | null;
  /** Why the call failed, as data from its tool.end; absent for a call that didn't fail or an older runtime. */
  failure?: ToolFailure | undefined;
  /** A desktop action's element, as the snapshot showed it (revision 14, ADR-0039): screen text, shown as text only. */
  target?: ScreenTarget | undefined;
  /** Whether a desktop action's before and after pictures were kept (revision 14); absent on every other call. */
  evidence?: ScreenEvidence | undefined;
  /** A desktop action's pictures, in the order they came (revision 14's screen.capture). */
  captures?: CaptureView[] | undefined;
};

/** One stored picture of the screen (ADR-0039 §9): before or after an action, or a screenshot the model asked for. */
export type CaptureView = {
  phase: CapturePhase;
  id: string;
  width: number;
  height: number;
  /** A model's screenshot with the numbered marks it acted by (set-of-marks); before and after never are. */
  marked: boolean;
};

/** A keyed call the gateway saw during a step (ADR-0037): the key's name (null for an unknown one), where, and how. */
export type KeyUseView = {
  /** Whose message started the run: the key is theirs, and only they see its name (ADR-0037 §2). */
  sender: string;
  name: string | null;
  host: string;
  port: number;
  outcome: "ok" | HostKeyOutcome;
};

/** A model request the agent retried within a step: why (the runtime's code), the wait, and the provider's words. */
export type RetryView = { reason: string; delayMs: number; provider?: string };

export type StepView = {
  kind: "step";
  id: string;
  runId: string;
  step: number;
  attempt: number;
  text: string;
  reasoning: string;
  tools: Record<string, ToolView>;
  toolOrder: string[];
  durationMs: number | null;
  /** Its model retries, in order (WEB-U-016): shown with the step, never as transcript notices. */
  retries: RetryView[];
  /** When its current attempt started, while the model has sent nothing in it (WEB-U-099); otherwise null. */
  waitingSince: string | null;
  /** When its current attempt started, for its live time (WEB-U-123); null before step.started. */
  startedAt: string | null;
  /** When the model stopped thinking in its current attempt: its first words, its first call or its end (17.3). */
  thoughtUntil?: string | undefined;
  /** When its first attempt started (a retry restarts `startedAt`), for placing stored key uses; absent before then. */
  firstStartedAt?: string;
  /** Its keyed calls and refusals, each key, address and outcome once (live frames and stored rows; absent when none). */
  keys?: KeyUseView[];
};

export type Item =
  | StepView
  | {
      kind: "user";
      id: string;
      text: string;
      attachments: string[];
      runId?: string;
      /**
       * A helper's first turn carries its brief from the lead (ADR-0034, protocol revision 12): shown as the brief,
       * never as the person's words, and the turn's text (the gateway's frame) is not shown (WEB-U-223).
       */
      brief?: BriefView;
      /**
       * The gateway's wake (protocol revision 13): new team messages are waiting. Shown as the gateway's notice, never as
       * the person's words; the turn's text is the model's sentence and is not shown. Who wrote is in the team's log.
       */
      wake?: true;
    }
  | {
      kind: "notice";
      id: string;
      copy: Copy;
      template?: string;
      /** The provider's own words on a retry, as the runtime sanitised them (WEB-U-055); never translated. */
      provider?: string;
    };

export type RunStatus =
  "idle" | "queued" | "running" | "completed" | "cancelled" | "failed";
export type RunRecord = {
  status: RunStatus;
  startedAt: string;
  finishedAt: string | null;
  reason: string | null;
  /** The failure code of a failed run, which may select plain copy (failure-copy.ts). */
  code: string | null;
  /** The provider's own words on a failed run, as the runtime sanitised them (WEB-U-055); never translated. */
  providerMessage: string | null;
  promptTokens: number;
  completionTokens: number;
  /** Its stored key uses that couldn't be put on a step with confidence, each once (ADR-0037; absent when none). */
  keys?: KeyUseView[];
};
/** What one run loaded into the model's context, for the context panel (FR-11). */
export type ContextReport = Pick<
  Extract<Event, { type: "context.loaded" }>,
  "instructions" | "skills" | "skipped"
>;
export type Transcript = {
  lastSeq: number;
  /** Counts the task's message.queue frames: its held messages changed, so the composer reads them again (GW-I-129). */
  heldVersion: number;
  items: Item[];
  sandbox: SandboxState;
  queue: QueuePosition | null;
  /**
   * An environment that keeps stopping (the gateway's SandboxSummary.recovery, WEB-U-068): its losses in 10 minutes,
   * the seconds until the replacement (null: none), and when that is on the reader's clock (null in a replay).
   */
  recovery: {
    losses: number;
    seconds: number | null;
    /** When the notice came, on the reader's clock (null in a replay). */
    from: number | null;
    at: number | null;
  } | null;
  run: { id: string | null; status: RunStatus };
  runs: Record<string, RunRecord>;
  /** Each run's context report by run id; a resumed run's second report replaces its first. */
  context: Record<string, ContextReport>;
  usage: {
    prompt: number;
    completion: number;
    cached: number;
    reasoning: number;
  };
  /** A stream-level problem to show above the transcript, as translatable copy. */
  banner: Copy | null;
  /**
   * Whether the banner is the link's own, which a reconnect ends (WEB-U-234). A quota notice is not: the gateway says it
   * once per run, live only, so it stays across a reconnect.
   */
  bannerEndsOnReconnect: boolean;
  /** The task's short name, when the gateway named it on this stream (task.titled, WEB-U-081); live only. */
  title: string | null;
  /**
   * The latest team.changed on this stream (ADR-0034 S6, WEB-U-229): the team's version and last message seq, each the
   * highest seen, so the page reads the team or its messages only when one rose; live only, null before the first.
   */
  team: { id: string; version: number; lastSeq: number } | null;
  /** A desktop task's screen notices as they arrive live (ADR-0039 §6, PROTO-U-038); the stored ones are read. */
  screenNotices: ScreenNoticeView[];
  /** The latest failed action, by stable code; the view words it in the reader's language. */
  actionFailure: { generation: number; code: string } | null;
  /** When each blocked destination's line was last shown (live only, pruned after a minute). */
  egress: Readonly<Record<string, number>>;
  /**
   * The reader's clock minus the frames' stamps, in ms, the smallest seen on arrival (WEB-U-122): the runtime stamps
   * frames on its own clock, which may run ahead of or behind the reader's; live counts add it to a stamp. Null before
   * any stamped frame has arrived.
   */
  clockOffset: number | null;
  /** The runs whose decision engine already fell back: each says so once (WEB-U-093). */
  fellBack: readonly string[];
  /** Stored key uses refused while no run was going, each once (ADR-0037; absent when none). */
  taskKeys?: KeyUseView[];
};

/** A helper's brief from its lead, as the transcript shows it (WEB-U-223). */
export type BriefView = {
  role: string;
  objective: string;
  output: string;
  sources: string | null;
  boundaries: string;
};

/**
 * One screen notice: who took over, handed back (after how long, and how it ended), or signed the browser out (or
 * couldn't, and why), and when.
 */
export type ScreenNoticeView = {
  kind:
    | "screen.taken_over"
    | "screen.handed_back"
    | "screen.signed_out"
    | "screen.sign_out_failed";
  at: string;
  seconds?: number | undefined;
  ended?: "hand_back" | "socket_closed" | "restart" | undefined;
  reason?: "not_cleared" | "unconfirmed" | undefined;
};
