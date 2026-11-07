"use client";

import { useState } from "react";

import { ChannelComposer } from "../channels/ChannelComposer";
import { ChannelFirstStep } from "../channels/ChannelFirstStep";
import { MemberList } from "../channels/ChannelMembers";
import { ChannelTimeline } from "../channels/ChannelTimeline";
import type {
  ChannelMessage,
  ChannelsApi,
  Member,
  ProposalState,
} from "../channels/contract";
import { MemberSearch, NOBODY, type Chosen } from "../channels/MemberSearch";
import { ProposalPanel } from "../channels/ProposalPanel";

/** A fixed set of tasks, so the search box's suggestions look the same on every visit. */
const api: Pick<ChannelsApi, "myTasks" | "addPerson"> = {
  myTasks: () =>
    Promise.resolve([
      { id: "design-task-1", title: "Fix the date range bug" },
      { id: "design-task-2", title: "Add CSV export" },
    ]),
  addPerson: () => Promise.reject(new Error("The reference adds no one.")),
};

const ANA = { kind: "user" as const, id: "design-ana", name: "Ana" };
const CODER = {
  kind: "conversation" as const,
  id: "design-coder",
  name: "Coder",
};
const MEMBERS: Member[] = [
  { ...ANA, role: "lead", added_at: "2026-09-28T09:00:00Z" },
  {
    ...CODER,
    owner_name: "ana",
    role: "member",
    added_at: "2026-09-28T09:00:00Z",
  },
];
/** One of each: an agent by its task's title at work, one done, a lead, one who reads only, a plain member. */
const ROSTER: Member[] = [
  {
    kind: "conversation",
    id: "design-planner",
    name: "Planner",
    owner_name: "ana",
    title: "Plan the release",
    role: "lead",
    run_status: "running",
    added_at: "2026-09-28T09:00:00Z",
  },
  {
    kind: "conversation",
    id: "design-reviewer",
    name: "Reviewer",
    owner_name: "ben",
    role: "member",
    run_status: "completed",
    added_at: "2026-09-28T09:00:00Z",
  },
  { ...ANA, role: "lead", added_at: "2026-09-28T09:00:00Z" },
  {
    kind: "user",
    id: "design-wu",
    name: "Wu",
    role: "observer",
    added_at: "2026-09-28T09:00:00Z",
  },
  {
    kind: "user",
    id: "design-cy",
    name: "Cy",
    role: "member",
    added_at: "2026-09-28T09:00:00Z",
  },
];
const at = (minute: number) =>
  `2026-09-28T09:${String(minute).padStart(2, "0")}:00Z`;
/** A short log: a message, a reply, and a proposal with its tally. */
const LOG: ChannelMessage[] = [
  {
    seq: 1,
    ts: at(2),
    author: ANA,
    body: { type: "text", text: "Let's aim for Friday; the checks are green." },
  },
  {
    seq: 2,
    ts: at(3),
    author: CODER,
    parent: 1,
    body: { type: "text", text: "Agreed. The notes are drafted." },
  },
  {
    seq: 3,
    ts: at(5),
    author: ANA,
    body: {
      type: "proposal",
      title: "Ship the release on Friday",
      text: "All checks are green.",
      window_seconds: 600,
    },
  },
];
const TALLY: ProposalState = {
  proposal_seq: 3,
  rule: "majority",
  state: "open",
  window_ends_at: "2999-01-01T00:00:00Z",
  voting_members: 3,
  tally: { accept: 2, reject: 0 },
  votes: [],
  decision: null,
};

function Search({
  initial,
  disabled = false,
}: {
  initial: Chosen;
  disabled?: boolean;
}) {
  const [chosen, setChosen] = useState(initial);
  return (
    <MemberSearch
      api={api}
      chosen={chosen}
      onChange={setChosen}
      disabled={disabled}
    />
  );
}

/** Who's in a new channel and a new channel's first step (WEB-U-062/063), reviewable in both themes. */
export function ChannelPatterns({ theme }: { theme: "light" | "dark" }) {
  return (
    <section
      className="design-panel stack"
      data-theme={theme}
      aria-label={`${theme} channel patterns`}
    >
      <h2 className="type-heading">Channel patterns · {theme}</h2>
      <p className="type-label">who&apos;s in it: only the creator</p>
      <Search initial={NOBODY} />
      <p className="type-label">who&apos;s in it: a task and a person</p>
      <Search
        initial={{
          agents: [{ id: "design-task-2", title: "Add CSV export" }],
          people: ["colleague@example.invalid"],
        }}
      />
      <p className="type-label">who&apos;s in it: while creating</p>
      <Search initial={NOBODY} disabled />
      <p className="type-label">
        a log: rows, a reply, a proposal&apos;s tally
      </p>
      <ChannelTimeline
        messages={LOG}
        members={MEMBERS}
        onReply={() => {}}
        after={(message) =>
          message.body.type === "proposal" ? (
            <ProposalPanel
              proposal={TALLY}
              members={MEMBERS}
              me="design-ana"
              memberRole="member"
              ended={false}
              onVote={() => Promise.resolve()}
            />
          ) : null
        }
      />
      <p className="type-label">the composer</p>
      <ChannelComposer
        memberRole="member"
        state="open"
        replyTo={null}
        onCancelReply={() => {}}
        onPost={() => Promise.resolve()}
      />
      <p className="type-label">members: agents by task, a dot for their run</p>
      <MemberList members={ROSTER} me="design-ana" />
      <p className="type-label">a new channel&apos;s first step</p>
      <ChannelFirstStep
        purpose="Get the 1.0 release out by Friday"
        onUse={() => {}}
      />
    </section>
  );
}
