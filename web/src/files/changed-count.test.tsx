import {
  focusManager,
  notifyManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId, runId, transcriptFrames } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { TranscriptView } from "../transcript/TranscriptView";
import type { Transcript } from "../transcript/types";

import { useChangedFiles } from "./use-changed-files";

function Harness({ transcript }: { transcript: Transcript }) {
  const changed = useChangedFiles(conversationId, transcript);
  return <TranscriptView transcript={transcript} changedFiles={changed} />;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

function show(answer: Response) {
  const fetch = vi.fn<typeof globalThis.fetch>(() => Promise.resolve(answer));
  vi.stubGlobal("fetch", fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <Harness transcript={transcriptFrames.reduce(apply, emptyTranscript())} />
    </QueryClientProvider>,
  );
  return fetch;
}
const change = (path: string) => ({
  path,
  status: "added",
  added: 1,
  removed: 0,
  binary: false,
  too_large: false,
});

// The turn summary counts the files the run changed, shell writes included, once they were recorded.
describe("WEB-U-024 files changed in the turn summary", () => {
  it("uses the run's recorded changes, which include files written by shell commands", async () => {
    const fetch = show(
      Response.json({
        run_id: runId,
        live: true,
        running: false,
        truncated: false,
        files: [change("report.md"), change("rows.txt")],
      }),
    );
    // The footer's own words: its details' copy for screen readers names the files too (WEB-U-461).
    expect(await screen.findByText(/^2 files changed$/)).toBeVisible();
    expect(fetch.mock.calls[0]?.[0]).toBe(
      `/api/v1/conversations/${conversationId}/runs/${runId}/changes`,
    );
  });

  it("keeps the tool count for a run recorded before changes were", async () => {
    show(
      Response.json(
        {
          error: {
            code: "changes_unavailable",
            message: "Not recorded.",
            retryable: false,
          },
        },
        { status: 404 },
      ),
    );
    expect(await screen.findByText(/^No files changed$/)).toBeVisible();
  });
});

// The hook is bounded - the gateway keeps 20 snapshots per workspace, and a closed run's answer
// never changes.
describe("WEB-U-024 the files-changed hook is bounded", () => {
  const runs = Array.from({ length: 50 }, (_, index) => `run-${index + 1}`);
  function manyRuns(): Transcript {
    let transcript = emptyTranscript();
    let seq = 0;
    for (const run of runs)
      for (const event of [
        { type: "run.started", run_id: run },
        { type: "run.completed", run_id: run },
      ] as const)
        transcript = apply(transcript, {
          type: "event",
          seq: ++seq,
          run_id: run,
          step: null,
          ts: new Date(Date.UTC(2026, 0, 1) + seq * 1000).toISOString(),
          event,
        });
    return transcript;
  }

  it("asks for at most the 20 latest runs, and a closed run only once", async () => {
    const fetch = vi.fn<typeof globalThis.fetch>((input) => {
      const run = /\/runs\/([^/]+)\/changes$/.exec(input as string)?.[1] ?? "";
      return Promise.resolve(
        Response.json({
          run_id: run,
          live: false,
          running: false,
          truncated: false,
          files: [change("a.txt")],
        }),
      );
    });
    vi.stubGlobal("fetch", fetch);
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const transcript = manyRuns();
    const { rerender } = render(
      <QueryClientProvider client={cache}>
        <Harness transcript={transcript} />
      </QueryClientProvider>,
    );
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(20));
    const asked = fetch.mock.calls.map(
      ([input]) => /\/runs\/([^/]+)\/changes$/.exec(input as string)?.[1],
    );
    expect(asked.sort()).toEqual(runs.slice(-20).sort());
    // A closed answer is final: a new render or a window focus asks nothing again. Query notifications run at once
    // here (by default they wait a timer turn), so a refetch would start within this step: no waiting on a clock.
    notifyManager.setScheduler((callback) => callback());
    try {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
      rerender(
        <QueryClientProvider client={cache}>
          <Harness transcript={transcript} />
        </QueryClientProvider>,
      );
      await act(async () => {});
      expect(cache.isFetching()).toBe(0);
      expect(fetch).toHaveBeenCalledTimes(20);
    } finally {
      notifyManager.setScheduler((callback) => setTimeout(callback, 0));
    }
  });
});
