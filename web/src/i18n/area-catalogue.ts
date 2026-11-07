"use client";

import { useEffect, useState } from "react";

import { addReference, hasReference } from "./translate";

type Catalogue = { default: { messages: Record<string, string> } };

/**
 * Loads one area's English once (WEB-U-014), so the pages that never show it never download it; a failed
 * download may be tried again. Other languages load their whole catalogue already.
 */
export function areaLoader(
  area: string,
  load: () => Promise<Catalogue>,
): () => Promise<void> {
  let loading: Promise<void> | null = null;
  return () => {
    loading ??= load()
      .then((catalogue) => addReference(area, catalogue.default.messages))
      .catch((error: unknown) => {
        loading = null;
        throw error;
      });
    return loading;
  };
}

/** Whether an area's copy is ready, still loading or failed, with the retry for a failure. */
export function useAreaCatalogue(
  area: string,
  load: () => Promise<void>,
  preloaded: boolean,
) {
  // Once loaded, the area's catalogue serves every later view at once.
  const [state, setState] = useState<"loading" | "ready" | "failed">(
    preloaded && hasReference(area) ? "ready" : "loading",
  );
  useEffect(() => {
    if (state !== "loading") return;
    let active = true;
    load().then(
      () => active && setState("ready"),
      () => active && setState("failed"),
    );
    return () => {
      active = false;
    };
  }, [state, load]);
  return { state, retry: () => setState("loading") };
}
