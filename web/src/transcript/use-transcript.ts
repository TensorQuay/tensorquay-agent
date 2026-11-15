"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useSyncExternalStore } from "react";

import { api } from "../api/client";
import { openEventStream } from "../api/stream";

import { TranscriptStore } from "./store";

/**
 * Owns the native stream for one mounted conversation and exposes animation-frame snapshots. The task's record, as the
 * page last read it, lets the store catch up when the stream is held back (WEB-U-468).
 */
export function useTranscript(
  id: string,
  record?: { run_status?: string | undefined; last_activity_at: string },
) {
  const query = useQueryClient();
  const [store] = useState(
    () =>
      new TranscriptStore({
        open: openEventStream,
        schedule: (callback) => requestAnimationFrame(callback),
        cancel: (handle) => cancelAnimationFrame(handle),
        onClosed: () => {
          void query.invalidateQueries({ queryKey: ["session"] });
        },
        read: (after) => api.events(id, after),
        readRecord: () => api.conversation(id),
      }),
  );
  useEffect(() => {
    store.connect(id);
    return () => store.dispose();
  }, [store, id]);
  const status = record?.run_status;
  const at = record?.last_activity_at;
  useEffect(() => {
    if (at !== undefined) store.record(status, at);
  }, [store, status, at]);
  return useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getServerSnapshot,
  );
}
