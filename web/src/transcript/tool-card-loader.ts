import type { ToolCard } from "./ToolCard";

type Card = typeof ToolCard;

let loaded: Card | null = null;
let loading: Promise<Card> | null = null;

/**
 * Starts loading the tool card's code, once. A step's or a group's toggle calls it on the person's intent to open one
 * (warmToolCard), and the card's own lazy path when it opens, so a card renders at once instead of behind a
 * placeholder (the 13h UI review, #10: skeletons only after 300 ms).
 */
export function preloadToolCard(): Promise<Card> {
  loading ??= import("./ToolCard").then(
    ({ ToolCard: card }) => {
      loaded = card;
      return card;
    },
    (error: unknown) => {
      // A failed load is forgotten, so the next one (the card's own, when it opens) tries again.
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** Whether its code has been asked for (and not forgotten after a failure). */
export function toolCardRequested(): boolean {
  return loading !== null;
}

/** The person means to open a step (a pointer over it, a press, focus): start loading the card's code, once. */
export function warmToolCard(): void {
  // A head start only: a failed one leaves the card's lazy path to load the code when it opens.
  preloadToolCard().catch(() => undefined);
}

/** The tool card once its code is here; null before. */
export function loadedToolCard(): Card | null {
  return loaded;
}
