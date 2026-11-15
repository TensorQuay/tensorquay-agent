import type { TranscriptTurns } from "./TranscriptTurns";

type Turns = { TranscriptTurns: typeof TranscriptTurns };

let loaded: Turns | null = null;
let loading: Promise<Turns> | null = null;

/**
 * Loads the turns' renderers once (WEB-U-473): the transcript view asks on its first turn, and renders them in the
 * same pass once they are here, so a transcript never flashes a placeholder after its first load.
 */
export function preloadTurns(): Promise<Turns> {
  loading ??= import("./TranscriptTurns").then(
    (module) => {
      loaded = module;
      return module;
    },
    (error: unknown) => {
      // A failed load is forgotten, so the next render asks again.
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** Whether their code has been asked for (and not forgotten after a failure). */
export function turnsRequested(): boolean {
  return loading !== null;
}

/**
 * The person means to send a message (the composer gets focus or a press): start loading the renderers, once, so a
 * new task's first turn renders at once instead of behind a placeholder. A head start only: a failed one leaves the
 * view's lazy path to load them with the turn.
 */
export function warmTurns(): void {
  preloadTurns().catch(() => undefined);
}

/** The renderers once their code is here; null before. */
export function loadedTurns(): Turns | null {
  return loaded;
}
