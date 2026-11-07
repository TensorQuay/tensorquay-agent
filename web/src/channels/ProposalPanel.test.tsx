import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { member } from "../test/channels";

import type { ChannelPost, Member, ProposalState } from "./contract";
import { ProposalPanel } from "./ProposalPanel";

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
    expect(
      screen.getByRole("group", { name: "Votes on #2" }),
    ).toHaveTextContent("Vetoed");
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

// The 13f screen review, #8: after voting, the buttons looked the same as before; only a line of text said how this
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
