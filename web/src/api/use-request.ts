"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError } from "./error";

/** Owns one bounded UI request and aborts it when its component leaves the page. */
export function useRequest() {
  const active = useRef<AbortController | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  useEffect(
    () => () => {
      active.current?.abort();
    },
    [],
  );
  async function run<T>(
    request: (signal: AbortSignal) => Promise<T>,
  ): Promise<{ ok: true; value: T } | { ok: false }> {
    if (active.current) return { ok: false };
    const controller = new AbortController();
    active.current = controller;
    setPending(true);
    setError(null);
    try {
      const value = await request(controller.signal);
      return controller.signal.aborted ? { ok: false } : { ok: true, value };
    } catch (cause) {
      if (!controller.signal.aborted)
        setError(
          cause instanceof Error
            ? cause
            : new ApiError(0, "request_failed", "The request failed."), // copy-lint: not user copy
        );
      return { ok: false };
    } finally {
      active.current = null;
      if (!controller.signal.aborted) setPending(false);
    }
  }
  return { run, pending, error };
}
