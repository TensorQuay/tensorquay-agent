import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { member } from "../test/channels";
import { settleLazy } from "../test/lazy";

import { ChannelTimeline } from "./ChannelTimeline";
import type { ChannelMessage, Member } from "./contract";

const CODER = "0195d8c0-0000-7000-8000-0000000000c1";
const REVIEWER = "0195d8c0-0000-7000-8000-0000000000c2";
const ANA = "0195d8c0-0000-7000-8000-000000000001";
const members: Member[] = [
  {
    kind: "conversation",
    id: CODER,
    name: "Coder",
    owner_name: "alice",
    role: "lead",
    added_at: "2026-09-27T09:00:00Z",
  },
  {
    kind: "conversation",
    id: REVIEWER,
    name: "Reviewer",
    owner_name: "bob",
    role: "member",
    added_at: "2026-09-27T09:00:00Z",
  },
  {
    kind: "user",
    id: ANA,
    name: "Ana",
    role: "member",
    added_at: "2026-09-27T09:00:00Z",
  },
];
const coder = { kind: "conversation" as const, id: CODER, name: "Coder" };
const reviewer = {
  kind: "conversation" as const,
  id: REVIEWER,
  name: "Reviewer",
};
const ana = { kind: "user" as const, id: ANA, name: "Ana" };
const at = (seq: number) =>
  new Date(Date.UTC(2026, 8, 27, 10, seq)).toISOString();
const post = (
  seq: number,
  author: ChannelMessage["author"],
  body: ChannelMessage["body"],
  parent?: number,
): ChannelMessage => ({
  seq,
  ts: at(seq),
  author,
  ...(parent === undefined ? {} : { parent }),
  body,
});
const LOG: ChannelMessage[] = [
  post(1, coder, { type: "text", text: "Starting on the **release** notes." }),
  post(2, ana, {
    type: "proposal",
    title: "Ship on Friday",
    text: "All green.",
  }),
  post(3, reviewer, {
    type: "vote",
    proposal_seq: 2,
    choice: "accept",
    reason: "Tests pass",
  }),
  post(
    4,
    { kind: "gateway", id: "gateway", name: "gateway" },
    {
      type: "decision",
      proposal_seq: 2,
      outcome: "accepted",
      rule: "majority",
    },
  ),
  post(5, ana, {
    type: "task",
    title: "Write the changelog",
    spec: "From the merged PRs.",
    claims: 2,
  }),
  post(6, coder, { type: "claim", task_seq: 5 }),
  post(7, coder, {
    type: "report",
    task_seq: 5,
    status: "completed",
    summary: "Changelog written.",
    refs: ["https://git.example.invalid/pr/12", "CHANGELOG.md"],
  }),
  post(8, reviewer, { type: "text", text: "Looks right." }, 7),
  post(9, coder, { type: "done", summary: "Release prepared." }),
];

function show(log = LOG, onReply = vi.fn()) {
  render(
    <ChannelTimeline messages={log} members={members} onReply={onReply} />,
  );
  return onReply;
}
const card = (seq: number) =>
  screen.getByRole("article", { name: new RegExp(`^#${seq} `) });

// ADR-0014: a channel's log in seq order, each post by its type, with its author and role; replies under their
// parent; content written by agents and people is never trusted as HTML (WEB-U-063).
describe("WEB-U-063 the channel timeline", () => {
  // Light rows: the name, "agent" for a task's agent, then the time; no role pill and no seq on the screen (18.2,
  // WEB-U-520: roles live in Members; the seq stays in each post's accessible name).
  it("shows each post with its author and time, in order", async () => {
    show();
    await settleLazy();
    const posts = screen.getAllByRole("article");
    expect(posts.map((item) => item.getAttribute("aria-label"))).toEqual([
      "#1 Coder · alice",
      "#2 Ana",
      "#3 Reviewer · bob",
      "#4 Decision",
      "#5 Ana",
      "#6 Coder · alice",
      "#7 Coder · alice",
      "#8 Reviewer · bob",
      "#9 Coder · alice",
    ]);
    const first = card(1);
    expect(within(first).queryByText("Lead")).toBeNull();
    expect(within(first).getByText("Agent")).toBeVisible();
    expect(within(first).getByRole("time")).toHaveAttribute("datetime", at(1));
    expect(within(first).getByText("release").tagName).toBe("STRONG");
    expect(within(first).queryByText("#1")).toBeNull();
    // A plain member carries no role badge, and a person no kind badge.
    expect(within(card(2)).queryByText("Person")).toBeNull();
    expect(within(card(2)).queryByText("Member")).toBeNull();
    expect(within(card(3)).getByText("Agent")).toBeVisible();
    expect(within(card(3)).queryByText("Member")).toBeNull();
    expect(first.querySelectorAll(".status-pill")).toHaveLength(0);
  });

  // As in chat apps: consecutive posts by one author share one header line, so names and badges aren't repeated.
  it("groups consecutive posts by the same author under one header, within five minutes and until a thread", async () => {
    show();
    await settleLazy();
    const name = (seq: number) =>
      card(seq).querySelector(":scope > .channel-post-head .type-body-strong");
    // #6 and #7 are Coder's, a minute apart: #7 continues #6's group, keeping its own seq, time and label.
    expect(name(6)).toHaveTextContent("Coder");
    expect(card(7)).toHaveAttribute("data-continued", "true");
    expect(name(7)).toBeNull();
    expect(card(7)).toHaveAccessibleName("#7 Coder · alice");
    expect(
      card(7).querySelector(":scope > .channel-post-head time"),
    ).not.toBeNull();
    // #9 is Coder's again, but #7's thread comes between: a new header.
    expect(name(9)).toHaveTextContent("Coder");
    expect(card(1)).not.toHaveAttribute("data-continued");
  });

  it("starts a new header after five quiet minutes", async () => {
    const later = (seq: number, minute: number) => ({
      ...post(seq, ana, { type: "text", text: `Note ${seq}.` }),
      ts: new Date(Date.UTC(2026, 8, 27, 10, minute)).toISOString(),
    });
    show([later(1, 0), later(2, 4), later(3, 10)]);
    await settleLazy();
    expect(card(2)).toHaveAttribute("data-continued", "true");
    expect(card(3)).not.toHaveAttribute("data-continued");
  });

  it("reads every type in words", async () => {
    show();
    await settleLazy();
    expect(within(card(2)).getByText("Ship on Friday")).toBeVisible();
    expect(card(2)).toHaveTextContent("Proposal");
    expect(card(3)).toHaveTextContent(
      "voted Accept on Ship on Friday: Tests pass",
    );
    expect(card(4)).toHaveTextContent("Accepted by majority");
    expect(
      within(card(4)).getByRole("link", { name: "Ship on Friday" }),
    ).toHaveAttribute("href", "#post-2");
    expect(card(5)).toHaveTextContent("To-do");
    expect(card(5)).toHaveTextContent("Write the changelog");
    expect(card(5)).toHaveTextContent("Up to 2 may take it on");
    expect(card(6)).toHaveTextContent("took on Write the changelog");
    // Reads as a sentence, not "Completed report on #5".
    expect(card(7)).toHaveTextContent(
      "reported back on Write the changelog done",
    );
    expect(within(card(7)).queryByText("Completed")).toBeNull();
    expect(card(7)).toHaveTextContent("Changelog written.");
    expect(
      within(card(7)).getByRole("link", {
        name: "https://git.example.invalid/pr/12",
      }),
    ).toHaveAttribute("href", "https://git.example.invalid/pr/12");
    expect(within(card(7)).getByText("CHANGELOG.md").tagName).toBe("CODE");
    expect(card(9)).toHaveTextContent("finished: Release prepared.");
  });

  it("puts a reply under its parent, and offers Reply on each post", async () => {
    const onReply = show();
    await settleLazy();
    const thread = within(card(7)).getByRole("list", { name: "Replies to #7" });
    expect(within(thread).getByRole("article", { name: /^#8 / })).toBeVisible();
    fireEvent.click(
      within(card(1)).getByRole("button", { name: "Reply to #1" }),
    );
    expect(onReply).toHaveBeenCalledWith(1);
    // One-line events (a vote, a claim, "done") take no replies.
    for (const seq of [3, 6, 9])
      expect(
        within(card(seq)).queryByRole("button", { name: `Reply to #${seq}` }),
      ).toBeNull();
  });

  it("never renders a post's markup as HTML, nor links anything but http(s)", async () => {
    show([
      post(1, coder, {
        type: "text",
        text: '<img src=x onerror="alert(1)"> [run](javascript:alert(1))',
      }),
      post(2, coder, {
        type: "report",
        task_seq: 1,
        status: "failed",
        summary: "<b>no</b>",
        refs: ["javascript:alert(1)", "file:///etc/passwd"],
      }),
    ]);
    await settleLazy();
    expect(
      document.querySelector("img, b, a[href^='javascript'], a[href^='file']"),
    ).toBeNull();
    expect(card(2)).toHaveTextContent("<b>no</b>");
    expect(within(card(2)).getByText("javascript:alert(1)").tagName).toBe(
      "CODE",
    );
  });
});

// A screen review before 17.0: the log's vote, decision, claim and report lines said "on #2", a number people had to go
// and look up. They name the proposal's or task's title, a link to its post; "#n" only for a post not loaded.
describe("WEB-U-165 a line names the proposal or task it is about", () => {
  it("names the title, as a link to its post, with the whole title in the tooltip", async () => {
    show();
    await settleLazy();
    for (const [seq, seen] of [
      [3, "Ship on Friday"],
      [4, "Ship on Friday"],
      [6, "Write the changelog"],
      [7, "Write the changelog"],
    ] as const) {
      const link = within(card(seq)).getByRole("link", { name: seen });
      expect(link).toHaveAttribute("href", `#post-${seq < 5 ? 2 : 5}`);
      expect(link).toHaveAttribute("title", seen);
      expect(link).toHaveClass("channel-ref");
      expect(
        within(card(seq)).queryByRole("link", { name: /^#[25]$/ }),
      ).toBeNull();
    }
  });

  it("says #n for a proposal or task not loaded, or a post of another type", async () => {
    show([
      post(10, ana, { type: "text", text: "Hello." }),
      post(11, reviewer, { type: "vote", proposal_seq: 1, choice: "reject" }),
      post(12, coder, { type: "claim", task_seq: 10 }),
      post(13, reviewer, { type: "vote", proposal_seq: 10, choice: "accept" }),
      post(14, ana, { type: "task", title: "Tidy up", spec: "Later." }),
      post(15, reviewer, { type: "vote", proposal_seq: 14, choice: "accept" }),
    ]);
    await settleLazy();
    expect(card(11)).toHaveTextContent("voted Reject on #1");
    expect(within(card(11)).getByRole("link", { name: "#1" })).toHaveAttribute(
      "href",
      "#post-1",
    );
    expect(card(12)).toHaveTextContent("took on #10");
    expect(card(13)).toHaveTextContent("voted Accept on #10");
    // A task is not a proposal: a vote that names one says its number.
    expect(card(15)).toHaveTextContent("voted Accept on #14");
  });

  it("shows a title as text, never as markup", async () => {
    const title = "<b>Bold</b> **plan** [x](javascript:alert(1))";
    show([
      post(1, ana, { type: "task", title, spec: "Do it." }),
      post(2, coder, { type: "claim", task_seq: 1 }),
    ]);
    await settleLazy();
    const link = within(card(2)).getByRole("link", { name: title });
    expect(link.textContent).toBe(title);
    expect(link.children).toHaveLength(0);
    expect(link).toHaveAttribute("href", "#post-1");
  });
});

// WEB-U-515 (ADR-0014's amendment of 20 Nov 2026, A9): a post's files are downloads under it, each by its name and
// size; a post without files lists none.
describe("WEB-U-515 a post's files", () => {
  it("lists each file as a download", () => {
    const log: ChannelMessage[] = [
      {
        seq: 1,
        ts: "2026-09-27T10:00:00Z",
        author: { kind: "user", id: ANA, name: "Ana" },
        body: { type: "text", text: "The price list" },
        files: [
          { name: "q3.pdf", size: 2048 },
          { name: "notes.txt", size: 3 },
        ],
      },
      {
        seq: 2,
        ts: "2026-09-27T10:01:00Z",
        author: { kind: "user", id: ANA, name: "Ana" },
        body: { type: "text", text: "No file here" },
      },
    ];
    render(
      <ChannelTimeline
        messages={log}
        members={members}
        fileHref={(seq, index) => `/files/${seq}/${index}`}
      />,
    );
    const files = screen.getByRole("list", { name: "Files of #1" });
    const pdf = within(files).getByRole("link", { name: /q3\.pdf/ });
    expect(pdf).toHaveAttribute("href", "/files/1/0");
    expect(pdf).toHaveAttribute("download");
    expect(pdf).toHaveTextContent("q3.pdf · 2 kB");
    // A file reads as one, as a task's sent file does (the review of 20 Nov 2026): a chip with the file icon.
    expect(pdf.closest("li")).toHaveClass("attachment-chip");
    expect(pdf).toHaveClass("attachment-open");
    expect(pdf.querySelector("svg")).not.toBeNull();
    expect(within(files).getAllByRole("link")).toHaveLength(2);
    expect(screen.queryByRole("list", { name: "Files of #2" })).toBeNull();
  });
});

// WEB-U-520 (18.2 slice 3, the mock of 17 Nov 2026): one plain log. A run of posts by one author shows its face and name
// once; an agent by its name in the channel (WEB-U-542; its task's title only for an agent added before names, to its
// owner) with the agent mark, no role pill (roles live in Members); no seq numbers on the screen. A vote, a claim, a decision and a notice are each one muted line, a notice in the reader's own words.
describe("WEB-U-520 the chat log", () => {
  const analyst = {
    kind: "conversation" as const,
    id: CODER,
    name: "Coder",
  };
  const log: ChannelMessage[] = [
    {
      seq: 1,
      ts: "2026-09-27T10:00:00Z",
      author: analyst,
      body: { type: "text", text: "Churn by tier first." },
    },
    {
      seq: 2,
      ts: "2026-09-27T10:01:00Z",
      author: analyst,
      body: { type: "text", text: "Then the forecast." },
    },
    {
      seq: 3,
      ts: "2026-09-27T10:02:00Z",
      author: { kind: "user", id: ANA, name: "Ana" },
      body: { type: "proposal", title: "Raise the EU price", text: "To 24." },
    },
    {
      seq: 4,
      ts: "2026-09-27T10:02:30Z",
      author: { kind: "user", id: ANA, name: "Ana" },
      body: { type: "vote", proposal_seq: 3, choice: "accept" },
    },
    {
      seq: 5,
      ts: "2026-09-27T10:03:00Z",
      author: { kind: "gateway", id: "gateway", name: "gateway" },
      body: {
        type: "notice",
        kind: "member_deleted",
        member: "Forecast",
        text: "Forecast can't be woken: its task was deleted.",
      },
    },
  ];
  it("groups a run of posts, names an agent by its channel name once, and keeps events to one muted line", () => {
    render(
      <ChannelTimeline
        messages={log}
        members={members.map((item) =>
          item.id === CODER ? { ...item, title: "Pricing analyst" } : item,
        )}
      />,
    );
    const view = screen.getByRole("list", { name: "Channel log" });
    expect(within(view).getAllByText("Coder")).toHaveLength(1);
    expect(within(view).queryByText("Pricing analyst")).toBeNull();
    expect(within(view).getAllByText("Agent")).toHaveLength(1);
    expect(within(view).queryByText("Lead")).toBeNull();
    expect(view.textContent ?? "", view.textContent ?? "").not.toMatch(/#\d/);
    const vote = screen.getByRole("article", { name: /^#4/ });
    expect(vote).toHaveAttribute("data-line");
    const notice = screen.getByRole("article", { name: /^#5/ });
    expect(notice).toHaveAttribute("data-line");
    expect(
      within(notice).getByText(
        "Forecast can't be woken: its task was deleted.",
      ),
    ).toHaveClass("muted");
  });
});

// WEB-U-532 (C-15, 21 Nov 2026): a notice about a member names it as Members does. Two agents added before names both
// read "Agent" to everyone; to their owner, who sees their titles in Members, the notices read by those titles. A notice
// about one that is no longer a member keeps its own word.
describe("WEB-U-532 a notice names its member as Members does", () => {
  const OLD_A = "0195d8c0-0000-7000-8000-0000000000a1";
  const OLD_B = "0195d8c0-0000-7000-8000-0000000000b1";
  const unnamed = (id: string, title?: string): Member => ({
    kind: "conversation",
    id,
    name: "Agent",
    named: false,
    owner_name: "pat",
    role: "member",
    added_at: "2026-09-27T09:00:00Z",
    ...(title === undefined ? {} : { title }),
  });
  const notice = (seq: number, member_id?: string): ChannelMessage =>
    post(
      seq,
      { kind: "gateway", id: "gateway", name: "gateway" },
      {
        type: "notice",
        kind: "unreachable_runtime",
        member: "Agent",
        ...(member_id === undefined ? {} : { member_id }),
        text: "Agent can't be woken.",
      },
    );
  const log = [notice(1, OLD_A), notice(2, OLD_B), notice(3)];
  const line = (seq: number) =>
    screen.getByRole("article", { name: new RegExp(`^#${seq}`) }).textContent;

  it("shows their owner each unnamed agent by its title", () => {
    render(
      <ChannelTimeline
        messages={log}
        members={[
          unnamed(OLD_A, "Numbers helper"),
          unnamed(OLD_B, "Second opinion"),
        ]}
      />,
    );
    expect(line(1)).toContain(
      "Numbers helper can't be woken: its environment still runs an older version",
    );
    expect(line(2)).toContain("Second opinion can't be woken");
    // Without a member id, the notice's own word.
    expect(line(3)).toContain("Agent can't be woken");
  });

  it("shows anyone else the name Members shows them, never a title", () => {
    render(
      <ChannelTimeline
        messages={log}
        members={[unnamed(OLD_A), unnamed(OLD_B)]}
      />,
    );
    expect(line(1)).toContain("Agent can't be woken");
    expect(line(2)).toContain("Agent can't be woken");
    expect(`${line(1)}${line(2)}`).not.toMatch(/Numbers helper|Second opinion/);
  });
});

// WEB-U-521: in the log a decided proposal keeps its title on one line; its text folds with its votes.
describe("WEB-U-521 a decided proposal's text folds", () => {
  it("keeps the title and hides the text once the decision is in the log", async () => {
    const proposal: ChannelMessage = {
      seq: 1,
      ts: "2026-09-27T10:00:00Z",
      author: { kind: "user", id: ANA, name: "Ana" },
      body: {
        type: "proposal",
        title: "Raise the EU price",
        text: "To 24 from December.",
      },
    };
    const { rerender } = render(
      <ChannelTimeline messages={[proposal]} members={members} />,
    );
    await settleLazy();
    expect(screen.getByText("To 24 from December.")).toBeInTheDocument();
    rerender(
      <ChannelTimeline
        messages={[
          proposal,
          {
            seq: 2,
            ts: "2026-09-27T10:10:00Z",
            author: { kind: "gateway", id: "gateway", name: "gateway" },
            body: {
              type: "decision",
              proposal_seq: 1,
              outcome: "accepted",
              rule: "majority",
            },
          },
        ]}
        members={members}
      />,
    );
    const item = screen.getByRole("article", { name: /^#1/ });
    expect(within(item).getByText("Raise the EU price")).toBeVisible();
    expect(screen.queryByText("To 24 from December.")).toBeNull();
  });
});

// WEB-U-522 (18.2 slice 3, the mock of 17 Nov 2026): a long report (a post of more than 12 lines, or a report's summary)
// shows its first 6 lines and "Show the whole report · N lines"; opened, it shows all and can fold again. A short post
// is whole.
describe("WEB-U-522 a long report folds", () => {
  const long = Array.from(
    { length: 41 },
    (_, n) => `Line ${n + 1} of the churn report.`,
  ).join("\n\n");
  it("shows the first lines and opens to the whole", async () => {
    render(
      <ChannelTimeline
        messages={[
          {
            seq: 1,
            ts: "2026-09-27T10:00:00Z",
            author: { kind: "conversation", id: CODER, name: "Coder" },
            body: { type: "text", text: long },
          },
          {
            seq: 2,
            ts: "2026-09-27T10:01:00Z",
            author: { kind: "user", id: ANA, name: "Ana" },
            body: { type: "text", text: "Short." },
          },
        ]}
        members={members}
      />,
    );
    await settleLazy();
    expect(screen.getByText("Line 6 of the churn report.")).toBeInTheDocument();
    expect(screen.queryByText("Line 7 of the churn report.")).toBeNull();
    const open = screen.getByRole("button", {
      name: "Show the whole report · 41 lines",
    });
    fireEvent.click(open);
    await settleLazy();
    expect(
      screen.getByText("Line 41 of the churn report."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(screen.queryByText("Line 41 of the churn report.")).toBeNull();
    expect(
      screen.getAllByRole("button", { name: /Show the whole report/ }),
    ).toHaveLength(1);
  });
});

// WEB-U-530 (decided on 20 Nov 2026, C-6): a woken run that failed or was stopped is said in the log, in the reader's
// words: the member's name and, for a failure, the reason as the task page words it, never the raw code.
describe("WEB-U-530 a woken run that ended badly", () => {
  it("says the member and the reason in plain words", () => {
    const notice = (
      seq: number,
      body: ChannelMessage["body"],
    ): ChannelMessage => ({
      seq,
      ts: "2026-09-27T10:04:00Z",
      author: { kind: "gateway", id: "gateway", name: "gateway" },
      body,
    });
    render(
      <ChannelTimeline
        messages={[
          notice(1, {
            type: "notice",
            kind: "woken_run_failed",
            member: "Pricing analyst",
            code: "reply_cut",
            text: "Pricing analyst's run failed (reply_cut).",
          }),
          notice(2, {
            type: "notice",
            kind: "woken_run_stopped",
            member: "Data checker",
            text: "Data checker's run was stopped.",
          }),
        ]}
        members={[]}
      />,
    );
    const view = screen.getByRole("list", { name: "Channel log" });
    expect(view).toHaveTextContent(
      "Pricing analyst's run stopped: The model's reply was cut off at its output limit",
    );
    expect(view).toHaveTextContent("Data checker's run was stopped.");
    expect(view).not.toHaveTextContent("reply_cut");
  });
});

// TEST: WEB-U-553
// WEB-U-553 (18.3.0): a place given back is said in the log with the person's name as Members shows it and the
// to-do's number; a person who left the channel keeps the notice's own word.
describe("WEB-U-553 a place given back", () => {
  it("names the person and the to-do", () => {
    const ana = member("user", "Ana Lim");
    const notice = (seq: number, id: string): ChannelMessage => ({
      seq,
      ts: "2026-09-27T10:04:00Z",
      author: { kind: "gateway", id: "gateway", name: "gateway" },
      body: {
        type: "notice",
        kind: "place_given_back",
        member: "ana@example.invalid",
        member_id: id,
        task_seq: 4,
        text: "ana@example.invalid gave back their place on to-do #4; it is free again.",
      },
    });
    render(
      <ChannelTimeline
        messages={[notice(5, ana.id), notice(6, "someone-who-left")]}
        members={[ana]}
      />,
    );
    const lines = within(
      screen.getByRole("list", { name: "Channel log" }),
    ).getAllByText(/gave back their place/);
    expect(lines.map((line) => line.textContent)).toEqual([
      "Ana Lim gave back their place on #4; it is free again.",
      "ana@example.invalid gave back their place on #4; it is free again.",
    ]);
  });
});

// WEB-U-531 (decided on 20 Nov 2026, C-10): under a post the log shows its first three replies without a click, each
// with its author's name, then "N more replies" (or "N replies" when all show), which opens the thread.
describe("WEB-U-531 a post's replies in the log", () => {
  const post = (
    seq: number,
    who: { kind: "user" | "conversation"; id: string; name: string },
    text: string,
    parent?: number,
  ): ChannelMessage => ({
    seq,
    ts: `2026-09-27T10:0${seq}:00Z`,
    author: who,
    ...(parent ? { parent } : {}),
    body: { type: "text", text },
  });
  const mei = { kind: "user" as const, id: "u-mei", name: "Mei" };
  const analyst = {
    kind: "conversation" as const,
    id: "c-1",
    name: "Pricing analyst",
  };
  const checker = {
    kind: "conversation" as const,
    id: "c-2",
    name: "Data checker",
  };

  it("shows the first three replies with their authors, then how many more", async () => {
    const onThread = vi.fn();
    render(
      <ChannelTimeline
        messages={[
          post(1, mei, "What stands out in Q3?"),
          post(2, analyst, "EU Pro churn fell to 1.8 %.", 1),
          post(3, checker, "The EU rows check out.", 1),
          post(4, mei, "Good, and the US?", 1),
          post(5, analyst, "US Pro is flat at 2.0 %.", 1),
          post(6, checker, "US rows check out too.", 1),
        ]}
        members={[]}
        onThread={onThread}
      />,
    );
    await settleLazy();
    const log = screen.getByRole("list", { name: "Channel log" });
    const replies = within(log).getByRole("list", { name: "Replies to #1" });
    expect(replies).toHaveTextContent("Pricing analyst");
    expect(replies).toHaveTextContent("EU Pro churn fell to 1.8 %.");
    expect(replies).toHaveTextContent("Data checker");
    expect(replies).toHaveTextContent("The EU rows check out.");
    expect(replies).toHaveTextContent("Good, and the US?");
    expect(replies).not.toHaveTextContent("US Pro is flat");
    fireEvent.click(
      within(log).getByRole("button", { name: "2 more replies" }),
    );
    expect(onThread).toHaveBeenCalledWith(1);
  });

  it("says how many replies when all of them show", async () => {
    render(
      <ChannelTimeline
        messages={[
          post(1, mei, "What stands out in Q3?"),
          post(2, analyst, "EU Pro churn fell to 1.8 %.", 1),
          post(3, checker, "The EU rows check out.", 1),
        ]}
        members={[]}
        onThread={vi.fn()}
      />,
    );
    await settleLazy();
    const log = screen.getByRole("list", { name: "Channel log" });
    expect(within(log).getByText("The EU rows check out.")).toBeVisible();
    expect(
      within(log).getByRole("button", { name: "2 replies" }),
    ).toBeVisible();
  });
});

// TEST: WEB-U-542
// 18.2.1 (the lead's first-hour check, 23 Nov 2026): an agent's post shows the name it has in the channel, the one
// people write after "@" and Members shows, never its task's title. On 18.2 an agent added as "Analyst" posted as
// "Summarise Regional Sales Chart", its task's automatic title. One added before names keeps its title to its owner.
describe("WEB-U-542 an agent's post shows its name in the channel", () => {
  const AGENT = "0195d8c0-0000-7000-8000-0000000000c1";
  const OLD = "0195d8c0-0000-7000-8000-0000000000c2";
  const answer = (seq: number, id: string, name: string): ChannelMessage =>
    post(
      seq,
      { kind: "conversation", id, name },
      { type: "text", text: "South sold the most." },
    );

  it("names a named agent by its channel name, to its owner too", () => {
    render(
      <ChannelTimeline
        messages={[answer(1, AGENT, "Analyst"), answer(2, OLD, "Agent")]}
        members={[
          {
            kind: "conversation",
            id: AGENT,
            name: "Analyst",
            named: true,
            title: "Summarise Regional Sales Chart",
            owner_name: "first",
            role: "member",
            added_at: "2026-11-23T04:57:00Z",
          },
          {
            kind: "conversation",
            id: OLD,
            name: "Agent",
            named: false,
            title: "Numbers helper",
            owner_name: "first",
            role: "member",
            added_at: "2026-09-27T09:00:00Z",
          },
        ]}
      />,
    );
    const first =
      screen.getByRole("article", { name: /^#1/ }).textContent ?? "";
    expect(first).toContain("Analyst");
    expect(first).not.toContain("Summarise Regional Sales Chart");
    expect(screen.getByRole("article", { name: /^#2/ }).textContent).toContain(
      "Numbers helper",
    );
  });
});

// WEB-U-550 (18.3.0; the product owner, 27 Nov 2026: "the majority voted, but not closed"): a decision ends its proposal,
// never the channel. Under the newest decision, the person who can end the channel (its owner or an administrator)
// reads that it stays open and how to end it; nobody else, and no older decision, gets the line.
describe("WEB-U-550 a decision says the channel stays open, to whoever can end it", () => {
  const STAYS =
    "The channel stays open. End it from the … menu at the top once its work is done.";
  const second = post(
    10,
    { kind: "gateway", id: "gateway", name: "gateway" },
    {
      type: "decision",
      proposal_seq: 2,
      outcome: "rejected",
      rule: "majority",
    },
  );
  it("shows the line once, under the newest decision, when the viewer can end the channel", () => {
    render(
      <ChannelTimeline
        messages={[...LOG, second]}
        members={members}
        closeHint
      />,
    );
    expect(screen.getAllByText(STAYS)).toHaveLength(1);
    expect(card(10)).toHaveTextContent(STAYS);
    expect(card(4)).not.toHaveTextContent(STAYS);
  });
  it("shows nothing to someone who can't end the channel", () => {
    show([...LOG, second]);
    expect(screen.queryByText(STAYS)).toBeNull();
  });
});
