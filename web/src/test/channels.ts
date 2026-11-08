import { ApiError } from "../api/error";
import type {
  Channel,
  ChannelMessage,
  ChannelPost,
  ChannelsApi,
  Member,
  MessageBody,
  NewChannel,
  OwnTask,
  ProposalState,
} from "../channels/contract";

const idFrom = (name: string) =>
  `0195d8c0-0000-7000-8000-${[...name]
    .reduce((sum, char) => sum + char.charCodeAt(0), 0)
    .toString(16)
    .padStart(12, "0")}`;

/** A channel with a stable id from its name. */
export function channel(name: string, extra: Partial<Channel> = {}): Channel {
  return {
    id: idFrom(name),
    name,
    purpose: `About ${name}`,
    rule: "majority",
    state: "open",
    owner: { id: "0195d8c0-0000-7000-8000-000000000001", name: "Ana" },
    created_at: "2026-09-27T09:00:00Z",
    ended_at: null,
    role: "lead",
    member_count: 1,
    last_seq: 0,
    ...extra,
  };
}

/** A member with a stable id from its name. */
export function member(
  kind: Member["kind"],
  name: string,
  extra: Partial<Member> = {},
): Member {
  return {
    kind,
    id: idFrom(`${kind}:${name}`),
    name,
    role: "member",
    added_at: "2026-09-27T09:00:00Z",
    ...extra,
  };
}

/** A message by a member at a stable time from its seq. */
export function message(
  seq: number,
  by: Member,
  body: MessageBody,
  parent?: number,
): ChannelMessage {
  return {
    seq,
    ts: new Date(Date.UTC(2026, 8, 27, 10, seq)).toISOString(),
    author: { kind: by.kind, id: by.id, name: by.name },
    ...(parent === undefined ? {} : { parent }),
    body,
  };
}

/** A stream the test drives: `send` delivers a frame as the gateway would, `fail` closes it. */
export class FakeStream {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  readyState = 1;
  constructor(readonly after: number) {}
  send(frame: unknown) {
    this.onmessage?.(
      new MessageEvent("message", { data: JSON.stringify(frame) }),
    );
  }
  fail() {
    this.readyState = 2;
    this.onerror?.(new Event("error"));
  }
  close() {
    this.readyState = 2;
  }
}

/**
 * An in-memory channels API with the contract's semantics: creates and posts are recorded; an unknown channel is 404;
 * a post is written by `me` with the next seq. One refusal can be set for every create.
 */
export function fakeChannels({
  channels = [],
  members = {},
  messages = {},
  me = member("user", "Ana"),
  refuse,
  tasks = [],
  people = {},
  admin = false,
}: {
  channels?: Channel[];
  members?: Record<string, Member[]>;
  messages?: Record<string, ChannelMessage[]>;
  me?: Member;
  refuse?: ApiError;
  /** `me`'s own tasks, which `me` may add as agents. */
  tasks?: OwnTask[];
  /** People by exact email, for `addPerson`. */
  people?: Record<string, Member>;
  /** Whether `me` is an administrator, who may end or delete any channel. */
  admin?: boolean;
} = {}) {
  const store = [...channels];
  const logs = new Map(
    Object.entries(messages).map(([id, log]) => [id, [...log]]),
  );
  const creates: NewChannel[] = [];
  const posts: { id: string; write: ChannelPost; key: string }[] = [];
  const streams: FakeStream[] = [];
  const find = (id: string) => {
    const found = store.find((item) => item.id === id);
    if (!found) throw new ApiError(404, "not_found", "fixture refusal");
    return found;
  };
  const settle = <T>(work: () => T): Promise<T> => {
    try {
      return Promise.resolve(work());
    } catch (error) {
      return Promise.reject(
        error instanceof Error ? error : new Error("fixture failure"),
      );
    }
  };
  // The board and claims are assigned below, once the helpers they share exist.
  const api: Omit<
    ChannelsApi,
    | "tasks"
    | "claim"
    | "proposal"
    | "setMember"
    | "removeMember"
    | "myTasks"
    | "endChannel"
    | "deleteChannel"
  > &
    Partial<ChannelsApi> = {
    channels: () => Promise.resolve({ items: [...store], next_cursor: null }),
    createChannel(write) {
      creates.push(write);
      if (refuse) return Promise.reject(refuse);
      // Its creator owns and leads it (ADR-0014 point 4), as the gateway lists them.
      const made = channel(write.name, {
        purpose: write.purpose ?? "",
        rule: write.rule,
        member_count: 1,
        owner: { id: me.id, name: me.name },
      });
      members[made.id] = [{ ...me, role: "lead" }];
      store.unshift(made);
      return Promise.resolve(made);
    },
    channel: (id) => settle(() => find(id)),
    members: (id) =>
      settle(() => {
        find(id);
        return { items: [...(members[id] ?? [])] };
      }),
    messages: (id, since) =>
      settle(() => {
        find(id);
        const log = (logs.get(id) ?? []).filter((item) => item.seq > since);
        return {
          messages: log,
          next_since: log.at(-1)?.seq ?? since,
          more: false,
        };
      }),
    stream(_id, after) {
      const opened = new FakeStream(after);
      streams.push(opened);
      return opened;
    },
    post: (id, write, key) =>
      settle(() => {
        find(id);
        posts.push({ id, write, key });
        const log = logs.get(id) ?? [];
        const stored = message(
          (log.at(-1)?.seq ?? 0) + 1,
          me,
          write.body,
          write.parent,
        );
        logs.set(id, [...log, stored]);
        return stored;
      }),
  };
  const voting = (id: string) =>
    (members[id] ?? []).filter((item) => item.role !== "observer").length;
  const limitOf = (id: string, body: MessageBody) =>
    body.type !== "task" || body.claims === undefined
      ? 1
      : body.claims === "all"
        ? voting(id)
        : body.claims;
  /** The task board from the log (ADR-0014 point 8): each task's resolved limit, its claims and its reports. */
  const board = (id: string) => {
    const log = logs.get(id) ?? [];
    return log.flatMap((task) => {
      const body = task.body;
      if (body.type !== "task") return [];
      return [
        {
          task_seq: task.seq,
          title: body.title,
          limit: limitOf(id, body),
          claims: log.flatMap((item) =>
            item.body.type === "claim" && item.body.task_seq === task.seq
              ? [{ author: item.author, seq: item.seq }]
              : [],
          ),
          reports: log.flatMap((item) =>
            item.body.type === "report" && item.body.task_seq === task.seq
              ? [
                  {
                    author: item.author,
                    seq: item.seq,
                    status: item.body.status,
                  },
                ]
              : [],
          ),
        },
      ];
    });
  };
  api.tasks = (id) =>
    settle(() => {
      find(id);
      return { items: board(id) };
    });
  /** A claim by `me` (ADR-0014 point 8): a repeat answers the existing claim; a full task is already_claimed. */
  api.claim = (id, seq) =>
    settle(() => {
      find(id);
      const log = logs.get(id) ?? [];
      const task = board(id).find((item) => item.task_seq === seq);
      if (!task) throw new ApiError(409, "not_a_task", "fixture refusal");
      const held = log.find(
        (item) =>
          item.body.type === "claim" &&
          item.body.task_seq === seq &&
          item.author.kind === me.kind &&
          item.author.id === me.id,
      );
      if (held) return held;
      if (task.claims.length >= task.limit)
        throw new ApiError(409, "already_claimed", "fixture refusal");
      const stored = message((log.at(-1)?.seq ?? 0) + 1, me, {
        type: "claim",
        task_seq: seq,
      });
      logs.set(id, [...log, stored]);
      return stored;
    });
  /** A proposal's state from the log, as the contract says the gateway computes it (each member's latest vote). */
  api.proposal = (id, seq) =>
    settle(() => {
      const found = find(id);
      const log = logs.get(id) ?? [];
      const proposal = log.find((item) => item.seq === seq);
      if (proposal?.body.type !== "proposal")
        throw new ApiError(409, "not_a_proposal", "fixture refusal");
      const latest = new Map<string, ProposalState["votes"][number]>();
      for (const item of log)
        if (item.body.type === "vote" && item.body.proposal_seq === seq)
          latest.set(`${item.author.kind}:${item.author.id}`, {
            author: item.author,
            choice: item.body.choice,
            seq: item.seq,
          });
      const votes = [...latest.values()];
      const decision = log.find(
        (item) =>
          item.body.type === "decision" && item.body.proposal_seq === seq,
      );
      return {
        proposal_seq: seq,
        rule: found.rule,
        state: decision ? "decided" : "open",
        window_ends_at: new Date(
          Date.parse(proposal.ts) +
            (proposal.body.window_seconds ?? 600) * 1000,
        ).toISOString(),
        voting_members: (members[id] ?? []).filter(
          (item) => item.role !== "observer",
        ).length,
        tally: {
          accept: votes.filter((vote) => vote.choice === "accept").length,
          reject: votes.filter((vote) => vote.choice === "reject").length,
        },
        votes,
        decision:
          decision?.body.type === "decision"
            ? { seq: decision.seq, outcome: decision.body.outcome }
            : null,
      };
    });
  const roster = (id: string) => (members[id] ??= []);
  const owns = (id: string) => find(id).owner.id === me.id;
  const mine = (member: string) => tasks.some((task) => task.id === member);
  /** ADR-0014 point 4, as the gateway checks it: the owner anyone; a member their own tasks, never as lead; a person
   * themselves only to leave. */
  const mayManage = (
    id: string,
    kind: Member["kind"],
    member: string,
    role: Member["role"] | null,
  ) => {
    if (owns(id)) return;
    const belongs = roster(id).some(
      (item) => item.kind === "user" && item.id === me.id,
    );
    if (kind === "conversation" && belongs && mine(member) && role !== "lead")
      return;
    if (kind === "user" && member === me.id && role === null) return;
    throw new ApiError(403, "not_owner", "fixture refusal");
  };
  const place = (id: string, added: Member) => {
    const list = roster(id);
    const index = list.findIndex(
      (item) => item.kind === added.kind && item.id === added.id,
    );
    if (index >= 0) list[index] = added;
    else list.push(added);
    return added;
  };
  api.setMember = (id, kind, who, role) =>
    settle(() => {
      mayManage(id, kind, who, role);
      const known = roster(id).find(
        (item) => item.kind === kind && item.id === who,
      );
      const task = tasks.find((item) => item.id === who);
      if (!known && !(kind === "conversation" && task))
        throw new ApiError(404, "not_found", "fixture refusal");
      return place(
        id,
        known
          ? { ...known, role }
          : {
              ...member(kind, task?.title ?? who),
              id: who,
              owner_name: me.name,
              role,
            },
      );
    });
  api.removeMember = (id, kind, member) =>
    settle(() => {
      mayManage(id, kind, member, null);
      members[id] = roster(id).filter(
        (item) => !(item.kind === kind && item.id === member),
      );
    });
  api.addPerson = (id, email, role) =>
    settle(() => {
      if (!owns(id)) throw new ApiError(403, "not_owner", "fixture refusal");
      const person = people[email];
      if (!person) throw new ApiError(404, "user_not_found", "fixture refusal");
      return place(id, { ...person, role });
    });
  api.myTasks = () => Promise.resolve([...tasks]);
  const endable = (id: string) => {
    if (!owns(id) && !admin)
      throw new ApiError(403, "not_owner", "fixture refusal");
  };
  api.endChannel = (id) =>
    settle(() => {
      endable(id);
      const index = store.findIndex((item) => item.id === id);
      const ended = { ...find(id), state: "ended" as const };
      store[index] = ended;
      return ended;
    });
  api.deleteChannel = (id) =>
    settle(() => {
      endable(id);
      store.splice(
        store.findIndex((item) => item.id === id),
        1,
      );
    });
  return {
    api: api as ChannelsApi,
    creates,
    posts,
    streams,
    created: () => store[0],
    /** Ends a channel, as its owner would. */
    end(id: string) {
      const index = store.findIndex((item) => item.id === id);
      const found = store[index];
      if (found) store[index] = { ...found, state: "ended" };
    },
  };
}
