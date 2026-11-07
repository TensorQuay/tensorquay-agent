import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TeamHelper } from "../protocol/generated/api";
import { helper, team } from "../test/teams";
import { Composer } from "../transcript/Composer";
import type { Transcript } from "../transcript/types";

import { LeadTeam } from "./TeamOnPage";
import { helpersAtWork } from "./use-team-page";

const HINT =
  "Stops the lead's own step; its helpers keep working and can wake it. To stop them all, use Stop team in ⋯.";

function lead(stillWorking: boolean, onStopTeam = vi.fn()) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <LeadTeam
        team={team({ helpers: [helper(1, "researcher")] })}
        stillWorking={stillWorking}
        onStopTeam={onStopTeam}
      />
    </QueryClientProvider>,
  );
  return onStopTeam;
}

// WEB-U-307 (GW-I-250; the lead, 7 Oct): a lead's Stop ends only its own step, and a helper's report can wake it again.
// The Stop button says so in its tooltip and description; on a touch screen, where no tooltip shows, the lead's strip
// says the helpers still work once the lead is stopped, with Stop team.
describe("WEB-U-307 a lead's Stop while its helpers work", () => {
  it("says the helpers still work once the lead is stopped, with Stop team, and only then", () => {
    const onStopTeam = lead(true);
    expect(screen.getByText("Helpers are still working.")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Stop team" }));
    expect(onStopTeam).toHaveBeenCalledOnce();
  });

  // WEB-U-307's review (P3): a helper waiting for its lead has ended its run, so it can't wake the lead; the strip and the
  // Stop hint mustn't say helpers still work for it.
  it("counts a helper at work, queued, stuck or unreadable, but not one waiting for the lead, done, stopped or failed", () => {
    const only = (state: TeamHelper["state"]) =>
      helpersAtWork(team({ helpers: [helper(1, "researcher", { state })] }));
    const atWork: TeamHelper["state"][] = [
      "working",
      "queued",
      "stuck",
      "unknown",
    ];
    const settled: TeamHelper["state"][] = [
      "waiting_lead",
      "done",
      "stopped",
      "failed",
    ];
    for (const state of atWork) expect(only(state), state).toBe(true);
    for (const state of settled) expect(only(state), state).toBe(false);
    expect(
      helpersAtWork(
        team({
          helpers: [
            helper(1, "researcher", { state: "waiting_lead" }),
            helper(2, "writer", { state: "working" }),
          ],
        }),
      ),
    ).toBe(true);
    expect(helpersAtWork(team({ state: "ended" }))).toBe(false);
    expect(helpersAtWork(undefined)).toBe(false);
  });

  it("says nothing of it while the lead hasn't been stopped", () => {
    lead(false);
    expect(screen.queryByText("Helpers are still working.")).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop team" })).toBeNull();
  });

  const props = {
    sandbox: "ready" as const,
    sending: false,
    cancelling: false,
    onSend: vi.fn().mockResolvedValue(true),
    onStop: vi.fn().mockResolvedValue(undefined),
  };
  const running: Transcript["run"] = { id: "run-1", status: "running" };

  it("puts what Stop does in the Stop button's tooltip and description", () => {
    render(<Composer {...props} run={running} stopHint={HINT} />);
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toHaveAttribute("title", HINT);
    expect(stop).toHaveAccessibleDescription(HINT);
  });

  it("keeps Send and a plain Stop as they were without a hint", () => {
    const { rerender } = render(<Composer {...props} run={running} />);
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).not.toHaveAttribute("title");
    expect(stop).not.toHaveAccessibleDescription();
    rerender(
      <Composer
        {...props}
        run={{ id: null, status: "idle" }}
        stopHint={HINT}
      />,
    );
    expect(screen.getByRole("button", { name: "Send" })).not.toHaveAttribute(
      "title",
    );
  });
  // The c16 a11y pass: the line appears after the lead's Stop, where no focus goes, so it is said by a polite live
  // region that is there before it, as a screen reader needs.
  it("says the line in a live region that was there before it, with Stop team after it, outside", () => {
    const view = (stillWorking: boolean) => (
      <QueryClientProvider client={new QueryClient()}>
        <LeadTeam
          team={team({ helpers: [helper(1, "researcher")] })}
          stillWorking={stillWorking}
          onStopTeam={vi.fn()}
        />
      </QueryClientProvider>
    );
    const { rerender } = render(view(false));
    const region = screen.getByRole("status", { name: "" });
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toBeEmptyDOMElement();
    rerender(view(true));
    expect(region).toHaveTextContent(/^Helpers are still working\.$/);
    // WEB-U-307: the region says only the line; Stop team follows it outside, so a screen reader
    // meets it as a button, never as part of the status.
    expect(within(region).queryByRole("button")).toBeNull();
    const stop = screen.getByRole("button", { name: "Stop team" });
    expect(region.contains(stop)).toBe(false);
    expect(
      region.compareDocumentPosition(stop) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
