import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { member } from "../test/channels";

import type { ChannelPost, Member, ProposalState } from "./contract";
import { majorityNeeds, ProposalPanel } from "./ProposalPanel";

const CODER = member("conversation", "Coder", {
  owner_name: "alice",
  role: "lead",
});
const ANA = member("user", "Ana", { role: "member" });
const BEN = member("user", "Ben", { role: "member" });
const members: Member[] = [CODER, ANA, BEN];
const author = (who: Member) => ({
  kind: who.kind,
  id: who.id,
  name: who.name,
});
const NOW = Date.parse("2026-09-27T10:00:00Z");

function state(extra: Partial<ProposalState> = {}): ProposalState {
  return {
    proposal_seq: 2,
    rule: "majority",
    state: "open",
    window_ends_at: new Date(NOW + 8 * 60_000).toISOString(),
    voting_members: 3,
    tally: { accept: 1, reject: 0 },
    votes: [{ author: author(CODER), choice: "accept", seq: 3 }],
    decision: null,
    ...extra,
  };
}
function show(
  proposal: ProposalState,
  {
    me = ANA,
    role = "member",
    ended = false,
    vote = vi.fn<(write: ChannelPost) => Promise<void>>(() =>
      Promise.resolve(),
    ),
  }: {
    me?: Member;
    role?: Member["role"] | null;
    ended?: boolean;
    vote?: (write: ChannelPost) => Promise<void>;
  } = {},
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ProposalPanel
        proposal={proposal}
        members={members}
        me={me.id}
        memberRole={role}
        ended={ended}
        onVote={vote}
      />
    </QueryClientProvider>,
  );
  return vote;
}
beforeEach(() => {
  vi.useFakeTimers({ now: NOW, toFake: ["Date"] });
});
afterEach(() => vi.useRealTimers());

// ADR-0014 point 9: a proposal's tally and time left as the gateway computes them; a person voting member accepts,
// rejects (approves under the human rule) or vetoes, with a confirmation; nothing once decided (WEB-U-064).
describe("WEB-U-064 a proposal's votes and decision", () => {
  it("shows the tally, who has not voted, the time left and each latest vote", () => {
    show(state());
    const tally = screen.getByRole("group", { name: "Votes on #2" });
    expect(tally).toHaveTextContent("Accept 1 · Reject 0 · Not voted 2");
    // One bar for the whole tally, beside the counts in words (it was two bars, empty ones reading as broken).
    expect(within(tally).queryAllByRole("meter")).toEqual([]);
    const bars = tally.querySelectorAll(".proposal-bar");
    expect(bars).toHaveLength(1);
    expect(bars[0]).toHaveAttribute("aria-hidden", "true");
    const accepted = bars[0]?.querySelector<HTMLElement>("[data-tone='ok']");
    expect(accepted?.style.inlineSize).toBe(`${100 / 3}%`);
    expect(tally).toHaveTextContent("Closes in 8 minutes");
    // The product's chevron, not the browser's triangle.
    const who = within(tally).getByText("Who voted").closest("summary");
    expect(who?.querySelector("svg")).not.toBeNull();
    fireEvent.click(within(tally).getByText("Who voted"));
    expect(
      within(tally).getByRole("list", { name: "Latest votes" }),
    ).toHaveTextContent("Coder · alice: Accept");
  });

  it("lets a person accept or reject under majority, and veto after confirming", async () => {
    const vote = show(state());
    fireEvent.click(screen.getByRole("button", { name: "Accept #2" }));
    await vi.waitFor(() =>
      expect(vote).toHaveBeenLastCalledWith({
        body: { type: "vote", proposal_seq: 2, choice: "accept" },
      }),
    );
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Veto #2" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Veto #2" }));
    const confirm = screen.getByRole("dialog", { name: "Veto proposal #2?" });
    expect(confirm).toHaveTextContent(
      "A veto decides it at once, whatever the votes so far.",
    );
    fireEvent.click(within(confirm).getByRole("button", { name: "Veto" }));
    await vi.waitFor(() =>
      expect(vote).toHaveBeenLastCalledWith({
        body: { type: "vote", proposal_seq: 2, choice: "veto" },
      }),
    );
  });

  it("offers Approve and Reject under the human rule, and says how this person voted", () => {
    show(
      state({
        rule: "human",
        votes: [{ author: author(ANA), choice: "reject", seq: 4 }],
        tally: { accept: 0, reject: 1 },
      }),
    );
    expect(screen.getByRole("button", { name: "Approve #2" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Reject #2" })).toBeVisible();
    expect(screen.getByText("You voted Reject.")).toBeVisible();
  });

  it("shows the outcome and no controls once decided, and none for an observer or on an ended channel", () => {
    show(
      state({
        state: "decided",
        decision: { seq: 9, outcome: "vetoed" },
      }),
    );
    // Decided, it folds to one line with its outcome (WEB-U-521), and no vote control is offered.
    expect(
      screen.getByRole("button", { name: "Vetoed · 1 to 0" }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /#2$/ })).toBeNull();
  });

  it("gives an observer and an ended channel the tally without controls", () => {
    show(state(), { role: "observer" });
    expect(screen.queryByRole("button", { name: "Accept #2" })).toBeNull();
  });

  it("says why a vote was refused", async () => {
    show(state(), {
      vote: vi.fn(() =>
        Promise.reject(new ApiError(409, "proposal_closed", "x")),
      ),
    });
    fireEvent.click(screen.getByRole("button", { name: "Reject #2" }));
    expect(
      await screen.findByText("Voting on this proposal has closed."),
    ).toBeVisible();
  });
});

// A screen review before 17.0: after voting, the buttons looked the same as before; only a line of text said how this
// person voted. The chosen one is marked pressed, and drawn so, with a tick (not colour alone).
describe("WEB-U-166 the vote this person chose is marked", () => {
  it("presses the chosen choice only", () => {
    show(
      state({
        votes: [{ author: author(ANA), choice: "reject", seq: 4 }],
        tally: { accept: 0, reject: 1 },
      }),
    );
    const reject = screen.getByRole("button", { name: "Reject #2" });
    expect(reject).toHaveAttribute("aria-pressed", "true");
    expect(reject.querySelector("svg")).not.toBeNull();
    const accept = screen.getByRole("button", { name: "Accept #2" });
    expect(accept).toHaveAttribute("aria-pressed", "false");
    expect(accept.querySelector("svg")).toBeNull();
  });

  it("presses nothing before this person votes, whoever else has", () => {
    show(
      state({
        votes: [{ author: author(BEN), choice: "accept", seq: 4 }],
        tally: { accept: 1, reject: 0 },
      }),
    );
    for (const name of ["Accept #2", "Reject #2"])
      expect(screen.getByRole("button", { name })).toHaveAttribute(
        "aria-pressed",
        "false",
      );
  });

  it("presses Approve under the human rule", () => {
    show(
      state({
        rule: "human",
        votes: [{ author: author(ANA), choice: "accept", seq: 4 }],
        tally: { accept: 1, reject: 0 },
      }),
    );
    expect(screen.getByRole("button", { name: "Approve #2" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});

// WEB-U-521 (18.2 slice 3, decided on 20 Nov 2026): a decided proposal folds to one line, its outcome and the votes
// for and against; opening it shows the tally and each vote, and closing folds it again. An open one stays unfolded.
describe("WEB-U-521 a decided proposal folds to one line", () => {
  it("says the outcome and the count on one line, and opens to the votes", () => {
    show(
      state({
        state: "decided",
        tally: { accept: 2, reject: 0 },
        votes: [
          { author: author(CODER), choice: "accept", seq: 3 },
          { author: author(BEN), choice: "accept", seq: 4 },
        ],
        decision: { seq: 5, outcome: "accepted" },
      }),
    );
    const fold = screen.getByRole("button", { name: "Accepted · 2 to 0" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("group", { name: "Votes on #2" })).toBeNull();
    fireEvent.click(fold);
    expect(fold).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByRole("group", { name: "Votes on #2" }),
    ).toHaveTextContent("Accept 2 · Reject 0");
    fireEvent.click(fold);
    expect(screen.queryByRole("group", { name: "Votes on #2" })).toBeNull();
  });

  it("leaves an open proposal unfolded", () => {
    show(state());
    expect(screen.getByRole("group", { name: "Votes on #2" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /·.*to/ })).toBeNull();
  });
});

// WEB-U-526 (the review of 20 Nov 2026): Veto is a destructive action framed as a button (the danger variant, beside
// Accept and Reject), offered to a person voting member only (ADR-0014 point 9); the rule decides the question, so
// the open proposal says it on its closing line.
describe("WEB-U-526 a proposal's veto and rule", () => {
  it("frames Veto as a danger button for a person voting member, none for an observer", () => {
    show(state());
    expect(screen.getByRole("button", { name: "Veto #2" })).toHaveAttribute(
      "data-variant",
      "danger",
    );
    cleanup();
    show(state(), { role: "observer" });
    expect(screen.queryByRole("button", { name: "Veto #2" })).toBeNull();
  });

  it("says the rule on an open proposal's closing line, and not once decided", () => {
    show(state());
    expect(
      screen.getByText("Closes in 8 minutes · decided when 2 of 3 agree"),
    ).toBeVisible();
    cleanup();
    show(state({ rule: "human" }));
    expect(
      screen.getByText("Closes in 8 minutes · decided by a person"),
    ).toBeVisible();
    cleanup();
    show(
      state({
        state: "decided",
        decision: { seq: 9, outcome: "accepted" },
      }),
    );
    expect(screen.queryByText(/decided by/)).toBeNull();
  });
});

// WEB-U-539 (the owner's choice of 22 Nov 2026: keep the majority rule for 18.2 and say it on the proposal): under
// majority a proposal is decided when more than half of every voting member, people and agents alike, vote the same
// way, so an open proposal says how many that is. Tonight's runs had a person's own proposal time out 1 to 0 of 2.
describe("WEB-U-539 an open majority proposal says how many must agree", () => {
  it("names more than half of the voting members", () => {
    expect([1, 2, 3, 4, 5, 6].map(majorityNeeds)).toEqual([1, 2, 2, 3, 3, 4]);
    show(state({ voting_members: 4 }));
    expect(
      screen.getByText("Closes in 8 minutes · decided when 3 of 4 agree"),
    ).toBeVisible();
    cleanup();
    show(state({ voting_members: 2 }));
    expect(
      screen.getByText("Closes in 8 minutes · decided when 2 of 2 agree"),
    ).toBeVisible();
  });
});
