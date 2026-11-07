"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useSyncExternalStore } from "react";

import { openEventStream } from "../api/stream";

import { TranscriptStore } from "./store";

/** Owns the native stream for one mounted conversation and exposes animation-frame snapshots. */
export function useTranscript(id: string) {
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
      }),
  );
  useEffect(() => {
    store.connect(id);
    return () => store.dispose();
  }, [store, id]);
  return useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getServerSnapshot,
  );
}
