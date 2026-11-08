import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Team } from "../protocol/generated/api";
import { helper, leadId, team } from "../test/teams";

import { LeadTeam } from "./TeamOnPage";

function lead(given: Team, leadStopped: boolean) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <LeadTeam team={given} leadStopped={leadStopped} />
    </QueryClientProvider>,
  );
}

const waiting = (unread: number, extra: Partial<Team> = {}) =>
  team({
    lead: { id: leadId, read_seq: 12, unread },
    helpers: [helper(1, "researcher", { state: "done" })],
    ...extra,
  });

// WEB-U-423 (17.0's release candidate; GW-I-319): after a person stops a lead, the team's messages to it wait unread,
// and nothing said so (a review of the team wake before 17.0). The lead's strip now says how many, from the gateway's
// count, in its live region.
describe("WEB-U-423 a stopped lead's unread team messages", () => {
  it("says how many wait unread for the lead once it is stopped", () => {
    lead(waiting(3), true);
    const line = screen.getByText("3 team messages wait unread for the lead.");
    expect(line).toBeVisible();
    expect(line.closest("[role=status]")).not.toBeNull();
  });

  it("says one in the singular", () => {
    lead(waiting(1), true);
    expect(
      screen.getByText("1 team message waits unread for the lead."),
    ).toBeVisible();
  });

  it("says nothing while the lead runs, with none unread, or once the team has ended", () => {
    lead(waiting(3), false);
    lead(waiting(0), true);
    lead(waiting(3, { state: "ended", stop_reason: "stopped" }), true);
    expect(screen.queryByText(/wait(s)? unread for the lead/)).toBeNull();
  });
});
