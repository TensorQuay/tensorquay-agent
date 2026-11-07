import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ConversationControls } from "../conversations/ConversationControls";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, frame, runId } from "../test/frames";
import { shellData, workspace } from "../test/workspaces";
import { apply, emptyTranscript } from "../transcript/reducer";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  usePathname: () => "/",
}));

const lead = (state: "running" | "ended"): Conversation => ({
  id: conversationId,
  owner_id: conversationId,
  title: "Report on Q3",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: conversationId,
  mcp_servers: [],
  connectors: [],
  team: {
    role: "lead",
    id: "0195d8c0-0000-7000-8000-0000000000aa",
    state,
    helpers: 1,
  },
});

const ended = (outcome: "run.completed" | "run.failed") =>
  [
    frame(1, { type: "run.started", run_id: runId }),
    frame(
      2,
      outcome === "run.failed"
        ? {
            type: "run.failed",
            run_id: runId,
            code: "io",
            reason: "Runtime stopped",
          }
        : { type: "run.completed", run_id: runId },
    ),
  ].reduce(apply, emptyTranscript());

const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function header(
  conversation: Conversation,
  transcript: ReturnType<typeof ended>,
  outputs: { known: boolean; leadWaiting?: string | null; seen?: boolean },
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  shellData(cache, [
    workspace({ id: conversation.workspace_id, name: "Report on Q3" }),
  ]);
  return render(
    <QueryClientProvider client={cache}>
      <ConversationControls
        conversation={conversation}
        transcript={transcript}
        teamKnown={outputs.known}
        teamSeen={outputs.seen ?? false}
        leadWaiting={outputs.leadWaiting ?? null}
      />
    </QueryClientProvider>,
  );
}

// WEB-U-404 (16q int, E2E-AGENTS-032): a lead's page opened after its turn ended, while its team runs, read Done until
// the lazily loaded team layer had its words. The task's own record says it leads a running team, so until the layer
// has spoken the header says "Waiting…" in Done's (or Failed's) place, never Done; the layer's words then take over.
describe("WEB-U-404 a running team's lead never reads Done before its team layer has spoken", () => {
  it("says Waiting… in Done's place until the layer has the team", () => {
    const { rerender } = header(lead("running"), ended("run.completed"), {
      known: false,
    });
    const status = screen.getByLabelText("Run status");
    expect(status).not.toHaveTextContent("Done");
    expect(status).toHaveTextContent(/^Waiting…/);
    // The layer arrives with its words: they replace the bridge.
    const cache = new QueryClient();
    shellData(cache, [workspace({ id: conversationId, name: "Report on Q3" })]);
    rerender(
      <QueryClientProvider client={cache}>
        <ConversationControls
          conversation={lead("running")}
          transcript={ended("run.completed")}
          teamKnown
          leadWaiting="Waiting for the lead to finish the team"
        />
      </QueryClientProvider>,
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      "Waiting for the lead to finish the team",
    );
  });

  it("says Waiting… in Failed's place too, while the team runs", () => {
    header(lead("running"), ended("run.failed"), { known: false });
    const status = screen.getByLabelText("Run status");
    expect(status).not.toHaveTextContent(/^Failed/);
    expect(status).toHaveTextContent(/^Waiting…/);
  });

  // The opened page's first moment: no frame replayed yet, so the header reads the stored status (WEB-U-249). The mocked
  // e2e caught this window: the bridge has to hold there too.
  it("says Waiting… before the stream replays, from the stored status", () => {
    header({ ...lead("running"), run_status: "completed" }, emptyTranscript(), {
      known: false,
    });
    const status = screen.getByLabelText("Run status");
    expect(status).not.toHaveTextContent("Done");
    expect(status).toHaveTextContent(/^Waiting…/);
  });

  it("is Done for a lead whose team has ended, and once the layer knows there is nothing to wait for", () => {
    const { unmount } = header(lead("ended"), ended("run.completed"), {
      known: false,
    });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Done/);
    unmount();
    header(lead("running"), ended("run.completed"), { known: true });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Done/);
  });
});

// WEB-U-406 (16t int, E2E-AGENTS-032's third failure): the class, not each window. Any sign of a team holds the header
// at "Waiting…" until the layer has spoken: the stream's team frame before the task's record has its place, and a
// helper's running team as much as a lead's.
describe("WEB-U-406 any sign of a team: never Done or Failed before the team layer has spoken", () => {
  const noTeam = (): Conversation => ({ ...lead("running"), team: null });

  it("says Waiting… on the stream's team frame while the record has no team yet", () => {
    header(noTeam(), ended("run.completed"), { known: false, seen: true });
    const status = screen.getByLabelText("Run status");
    expect(status).not.toHaveTextContent("Done");
    expect(status).toHaveTextContent(/^Waiting…/);
  });

  it("says Waiting… in Failed's place on the team's frame too", () => {
    header(noTeam(), ended("run.failed"), { known: false, seen: true });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Waiting…/);
  });

  it("says Waiting… before the stream replays, from the stored status, on the team's frame", () => {
    header({ ...noTeam(), run_status: "completed" }, emptyTranscript(), {
      known: false,
      seen: true,
    });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Waiting…/);
  });

  it("says Waiting… for a helper in a running team until the layer has spoken", () => {
    const record = lead("running");
    header(
      { ...record, team: record.team && { ...record.team, role: "member" } },
      ended("run.completed"),
      { known: false },
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Waiting…/);
  });

  it("is Done for a task with no sign of a team", () => {
    header(noTeam(), ended("run.completed"), { known: false });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Done/);
  });
});
