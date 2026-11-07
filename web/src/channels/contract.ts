/**
 * The channels contract as the web uses it (ADR-0014): the generated API types for people (G3), with the names the
 * pages use. The pages depend on `ChannelsApi`; tests pass an in-memory fake.
 */
import type {
  Channel,
  ChannelAuthor,
  ChannelList,
  ChannelMember,
  ChannelMembers,
  ChannelMessage,
  ChannelMessageBody,
  ChannelMessagePage,
  ChannelPost as GeneratedPost,
  ChannelProposalState,
  ChannelTaskBoard,
  NewChannel,
} from "../protocol/generated/api";

export type {
  Channel,
  ChannelList,
  ChannelMembers,
  ChannelMessage,
  ChannelMessagePage,
  NewChannel,
};
export type Author = ChannelAuthor;
export type MessageBody = ChannelMessageBody;
export type DecisionRule = Channel["rule"];
export type ChannelState = Channel["state"];
export type ChannelRole = ChannelMember["role"];
export type VoteChoice = Extract<MessageBody, { type: "vote" }>["choice"];
export type Outcome = Extract<MessageBody, { type: "decision" }>["outcome"];
export type ReportStatus = Extract<MessageBody, { type: "report" }>["status"];
/** A task's claim limit: 1 when left out, a number up to the member limit, or everyone. */
export type ClaimLimit = NonNullable<
  Extract<MessageBody, { type: "task" }>["claims"]
>;
/**
 * A member; an agent's `run_status` comes from the gateway's lifecycle projection (GW-I-113), and `can_open` says
 * whether the caller may open an agent's task (its owner, or an administrator's private view).
 */
export type Member = ChannelMember;
/** What a member posts; the gateway sets the author, so the web never sends one. */
export type ChannelPost = Omit<GeneratedPost, "author">;

/** The part of an EventSource the channel page uses, so tests can drive it. */
export type ChannelStream = {
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  readyState: number;
  close(): void;
};

/** A channel name's and purpose's bounds. */
export const MAX_NAME = 80;
export const MAX_PURPOSE = 2000;

export type ChannelsApi = {
  channels(signal?: AbortSignal): Promise<ChannelList>;
  createChannel(write: NewChannel, key: string): Promise<Channel>;
  channel(id: string, signal?: AbortSignal): Promise<Channel>;
  members(id: string, signal?: AbortSignal): Promise<ChannelMembers>;
  /** Messages past `since`, in seq order; `more` says whether another page follows. */
  messages(
    id: string,
    since: number,
    signal?: AbortSignal,
  ): Promise<ChannelMessagePage>;
  post(id: string, write: ChannelPost, key: string): Promise<ChannelMessage>;
  /** The task board (G6). */
  tasks(id: string, signal?: AbortSignal): Promise<ChannelTaskBoard>;
  /** Claims a task for the caller (G6); a repeat answers the existing claim. */
  claim(id: string, seq: number, key: string): Promise<ChannelMessage>;
  /**
   * The channel's live messages past `after` (G8): server-sent events, `channel.message` frames with their seq as the
   * event id (so a reconnect replays from the last one) and an unsequenced `channel.state` when the channel ends.
   */
  stream(id: string, after: number): ChannelStream;
  /** A proposal's state (G7). */
  proposal(
    id: string,
    seq: number,
    signal?: AbortSignal,
  ): Promise<ProposalState>;
  /**
   * Adds a member or changes its role (ADR-0014 point 4; WEB-U-066): the owner any member; a conversation's owner their
   * own conversation, never as lead.
   */
  setMember(
    id: string,
    kind: MemberKind,
    member: string,
    role: ChannelRole,
  ): Promise<Member>;
  /** Removes a member; a person may remove themselves, which is leaving. */
  removeMember(id: string, kind: MemberKind, member: string): Promise<void>;
  /**
   * Adds a person by exact email (the owner only; an unknown email is 404 `user_not_found`, as workspace shares say it).
   * Absent until the gateway offers it, and the page then offers no by-email form.
   */
  addPerson?(id: string, email: string, role: ChannelRole): Promise<Member>;
  /** The caller's own tasks, newest first (their first page), for adding a task's agent. */
  myTasks(signal?: AbortSignal): Promise<OwnTask[]>;
  /** Ends the channel (the owner or an administrator; ADR-0014 point 13). */
  endChannel(id: string): Promise<Channel>;
  /** Deletes the channel for everyone (the owner or an administrator). */
  deleteChannel(id: string): Promise<void>;
};

export type MemberKind = Member["kind"];
/** One of the caller's tasks, as the member picker lists it. */
/** One of the caller's own tasks, which the caller may add as an agent; when it was last active tells same-named ones apart. */
export type OwnTask = { id: string; title?: string; active_at?: string };

/** A proposal's state as the gateway computes it (G7), the one place the decision rules live. */
export type ProposalState = ChannelProposalState;

/** A task's claims and reports for the board (G6). */
export type TaskState = ChannelTaskBoard["items"][number];
