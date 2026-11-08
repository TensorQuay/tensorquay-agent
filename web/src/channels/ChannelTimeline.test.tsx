import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

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
  // Light rows: the name, "agent" for a task's agent, a role only when it isn't a plain member, then seq and time.
  it("shows each post with its author, role, seq and time, in order", async () => {
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
    expect(within(first).getByText("Lead")).toBeVisible();
    expect(within(first).getByText("Agent")).toBeVisible();
    expect(within(first).getByRole("time")).toHaveAttribute("datetime", at(1));
    expect(within(first).getByText("release").tagName).toBe("STRONG");
    expect(within(first).getByText("#1")).toBeInTheDocument();
    // A plain member carries no role badge, and a person no kind badge.
    expect(within(card(2)).queryByText("Person")).toBeNull();
    expect(within(card(2)).queryByText("Member")).toBeNull();
    expect(within(card(3)).getByText("Agent")).toBeVisible();
    expect(within(card(3)).queryByText("Member")).toBeNull();
    expect(first.querySelectorAll(".status-pill")).toHaveLength(1);
  });

  // As in chat apps: consecutive posts by one author share one header line, so names and badges aren't repeated.
  it("groups consecutive posts by the same author under one header, within five minutes and until a thread", async () => {
    show();
    await settleLazy();
    const name = (seq: number) =>
      card(seq).querySelector(":scope > .channel-post-head .type-body-strong");
    // #6 and #7 are Coder's, a minute apart: #7 continues #6's group, keeping its own seq, time and label.
    expect(name(6)).toHaveTextContent("Coder · alice");
    expect(card(7)).toHaveAttribute("data-continued", "true");
    expect(name(7)).toBeNull();
    expect(card(7)).toHaveAccessibleName("#7 Coder · alice");
    expect(within(card(7)).getByText("#7")).toBeInTheDocument();
    // #9 is Coder's again, but #7's thread comes between: a new header.
    expect(name(9)).toHaveTextContent("Coder · alice");
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
