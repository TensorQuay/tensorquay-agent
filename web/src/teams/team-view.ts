/**
 * The agent team as the page shows it (ADR-0034, the S6 contract): what the presentation reads, mapped from the
 * gateway's team answer by the binding, so the parts below don't depend on the wire's spelling.
 */
export type HelperState =
  | "working"
  | "waiting_lead"
  | "queued"
  | "stuck"
  | "done"
  | "stopped"
  | "failed"
  | "unknown";

export type HelperView = {
  conversation: string;
  name: string;
  state: HelperState;
  /** How many runs are ahead of a queued helper. */
  position?: number | undefined;
  /** Whose files it works on: the lead's, taken in turns (`my_files`), or its own copy (WEB-U-393). */
  workspace?: "my_files" | "own_copy" | undefined;
  /** Its latest message to the lead, cut by the gateway, its kind and when; null before it wrote. */
  lastMessage?: { text: string; type?: string; at?: string } | null;
  /** What it is doing now: the tool of its current step (null while the model thinks) and since when. */
  activity?: { tool: string | null; since: string } | null;
  /** What it has used of the team's budget, for its share of the work. */
  tokens?: number;
  /** Part of `tokens` is the relay's estimate: a step stopped before its model server reported its use. */
  tokensEstimated?: boolean;
  /** Whether Stop helper would stop it now, as the gateway decides (`can_stop`; a failed one may still hold its seat). */
  canStop?: boolean;
};

export type TeamView = {
  state: "running" | "stopping" | "ended";
  /** Why an ended team stopped, as the gateway stores it ("done", "budget", …); null while it runs. */
  stopReason?: string | null;
  budget: { tokens: number; spent: number };
  /** When the team stops for time; null without a deadline. */
  deadlineAt: string | null;
  helpers: HelperView[];
  /** The lead's open question to its person; null when none waits. */
  waitingForYou: { seq: number; text: string } | null;
};

/** Who wrote or receives a team message, as the page names them. */
export type Party =
  | { kind: "lead" }
  | { kind: "member"; name: string }
  | { kind: "members" }
  | { kind: "team" }
  | { kind: "person"; you?: boolean; name?: string }
  | { kind: "gateway" };

/** One message of the team's log (ADR-0034's S1 read shape, as the page needs it). */
export type TeamMessageView = {
  seq: number;
  type:
    | "assign"
    | "question"
    | "answer"
    | "report"
    | "ask"
    | "decision"
    | "person_question"
    | "person_answer"
    | "person_message"
    | "notice"
    | "done"
    | "ended";
  from: Party;
  to: Party;
  text: string;
  /** How many files a report handed back. */
  files?: number | undefined;
  /** The helper a person wrote to (`person_message`). */
  member?: string | undefined;
  /** A gateway notice's kind and its helper (`notice`). */
  notice?: { kind: string; member?: string; position?: number } | undefined;
};

/** The team's end note (`ended` in ADR-0034's S6 contract, from the one stop path). */
export type TeamEndView = {
  reason: string;
  /** The lead's own summary, when it said it was done. */
  summary: string | null;
  tokens: { total: number; members: { name: string; tokens: number }[] };
  reports: { member: string; seq: number }[];
  /** What the helpers' reports handed back. */
  files: { member: string; seq: number; name: string; size: number }[];
};
