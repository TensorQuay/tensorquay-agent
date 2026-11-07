"use client";

import { useQueries, type Query } from "@tanstack/react-query";
import { useMemo } from "react";

import { api } from "../api/client";
import type { RunChanges } from "../protocol/generated/api";
import type { Transcript } from "../transcript/types";

/** The gateway keeps this many snapshots per workspace ([files] snapshots_kept default). */
const RECENT_RUNS = 20;
const closed = (data: RunChanges | undefined) => data?.live === false;

/**
 * Which files each finished run changed, from its recorded changes (review of 068c): shell writes count too. A run
 * recorded before changes were (or any failure) is left out, and its turn keeps the count from file-edit tools; the
 * turn adds the files its own calls wrote to these paths (WEB-U-365).
 * Shares the Changes tab's cache. Bounded (answers-076 item 0): only the latest runs are asked, as the gateway keeps
 * no older snapshots, and a closed run's answer (`live: false`) never changes, so it is fetched once.
 */
export function useChangedFiles(
  conversationId: string,
  transcript: Transcript,
): Readonly<Record<string, readonly string[]>> {
  const runs = Object.entries(transcript.runs)
    .filter(([, run]) => run.status !== "running")
    .map(([id]) => id)
    .slice(-RECENT_RUNS);
  const results = useQueries({
    queries: runs.map((run) => ({
      queryKey: ["changes", conversationId, run],
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        api.runChanges(conversationId, run, signal),
      retry: false,
      staleTime: (query: Query<RunChanges>) =>
        closed(query.state.data) ? Infinity : 30_000,
      refetchOnWindowFocus: (query: Query<RunChanges>) =>
        !closed(query.state.data),
    })),
  });
  // The same object while the counts are the same, so the transcript isn't re-rendered for nothing (WEB-U-151).
  const counts = runs
    .flatMap((run, index) => {
      const data = results[index]?.data;
      return data
        ? [`${run}\u0000${data.files.map((file) => file.path).join("\u0002")}`]
        : [];
    })
    .join("\u0001");
  return useMemo(
    () =>
      Object.fromEntries(
        counts
          ? counts.split("\u0001").map((entry) => {
              const [run = "", paths = ""] = entry.split("\u0000");
              return [run, paths ? paths.split("\u0002") : []];
            })
          : [],
      ),
    [counts],
  );
}
