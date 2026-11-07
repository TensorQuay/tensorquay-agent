import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { startedTeam } from "./started-team";
import { useTeamSlot } from "./team-slot";

/** A lead's run whose one call is `name`, ended `status`, then the run's end. */
const run = (id: string, name: string, status: "ok" | "error", from = 1) =>
  [
    frame(from, { type: "message.user", text: "Lead.", attachments: [] }),
    frame(from + 1, { type: "run.started", run_id: id }),
    frame(from + 2, { type: "step.started", step: 1, attempt: 1 }),
    frame(from + 3, {
      type: "tool.call",
      call_id: `c-${from}`,
      name,
      arguments: { role: "researcher" },
    }),
    frame(from + 4, { type: "tool.start", call_id: `c-${from}` }),
    frame(from + 5, { type: "tool.end", call_id: `c-${from}`, status }),
    frame(from + 6, {
      type: "tool.result",
      call_id: `c-${from}`,
      status,
      content: "Started.",
    }),
    frame(from + 7, { type: "run.completed", run_id: id }),
  ].map((value) => ({ ...value, run_id: id }));

const transcript = (...runs: ReturnType<typeof run>[]) =>
  runs.flat().reduce(apply, emptyTranscript());

// WEB-U-416 (16w int, E2E-AGENTS-032): a lead's first run starts its team and ends before the gateway's first
// team.changed (a once-a-second flush). The run's own ok start_member is the page's sign of its team at once: the
// gateway answers the call only once the team is in.
describe("WEB-U-416 the task's own transcript says it started a team", () => {
  it("names the last run when an ok start_member is in it", () => {
    expect(startedTeam(transcript(run(runId, "start_member", "ok")))).toBe(
      runId,
    );
  });

  it("is null for a refused start_member, another call, or one in an earlier run only", () => {
    expect(
      startedTeam(transcript(run(runId, "start_member", "error"))),
    ).toBeNull();
    expect(startedTeam(transcript(run(runId, "shell", "ok")))).toBeNull();
    expect(
      startedTeam(
        transcript(
          run("run-1", "start_member", "ok"),
          run("run-2", "shell", "ok", 9),
        ),
      ),
    ).toBeNull();
    expect(startedTeam(emptyTranscript())).toBeNull();
  });
});

const record = {
  id: conversationId,
  team: null,
  run_status: "completed",
} as unknown as Conversation;

function slot(started: string | null) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["conversation", conversationId], record);
  return renderHook(
    () =>
      useTeamSlot({
        conversation: record,
        teamsOn: true,
        live: null,
        connection: "live",
        viewer: null,
        bar: null,
        startedTeam: started,
      }),
    {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={cache}>{children}</QueryClientProvider>
      ),
    },
  );
}

afterEach(() => vi.restoreAllMocks());

// The guard: the sign holds the header at "Waiting…" only until a read of the task made after it answers. A read that
// still has no team (teams turned off, an old history) ends it, so a page never hangs on "Waiting…".
describe("WEB-U-416 the sign of a started team, and its guard", () => {
  it("counts as a sign of a team, and reads the task again", async () => {
    const read = vi
      .spyOn(api, "conversation")
      .mockReturnValue(new Promise(() => {}));
    const { result } = slot(runId);
    expect(result.current.joining).toBe(true);
    await waitFor(() =>
      expect(read).toHaveBeenCalledWith(conversationId, expect.anything()),
    );
  });

  it("stops counting once a read made after it still has no team", async () => {
    let answer: (value: Conversation) => void = () => {};
    vi.spyOn(api, "conversation").mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    const { result } = slot(runId);
    expect(result.current.joining).toBe(true);
    answer(record);
    await waitFor(() => expect(result.current.joining).toBe(false));
  });

  it("keeps counting while the read fails, or when the read has the team", async () => {
    vi.spyOn(api, "conversation").mockRejectedValue(
      new ApiError(503, "unavailable", "unavailable"),
    );
    const failing = slot(runId);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(failing.result.current.joining).toBe(true);
    vi.spyOn(api, "conversation").mockResolvedValue({
      ...record,
      team: { role: "lead", id: "team-1", state: "running" },
    });
    const found = slot(runId);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(found.result.current.joining).toBe(true);
  });

  it("is no sign without a started team", () => {
    expect(slot(null).result.current.joining).toBe(false);
  });
});
