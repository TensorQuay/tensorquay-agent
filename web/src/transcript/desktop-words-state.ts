// The Desktop tool's step words' loading state, on its own so the task page's first scripts carry it without the step
// words' summaries (tool-summary.ts loads with the first turn, WEB-U-473).

/**
 * The Desktop tool's step words (src/screen/desktop-words.ts, WEB-U-422): loaded as a Desktop task's page opens, or at
 * the first `computer` step anywhere; until then a step reads "Used the desktop", and a page that knows it is a
 * Desktop task waits for them before its steps render (LiveConversation).
 */
let desktopWords: typeof import("../screen/desktop-words") | null = null;
/** The Desktop tool's words once they are here; null before. */
export const loadedDesktopWords = () => desktopWords;
let desktopLoading: Promise<void> | null = null;
let desktopFailed = false;
const desktopArrivals = new Set<() => void>();

export function loadDesktopWords(): Promise<void> {
  // The server renders without them, as hydration's first render does (as the failure copy, WEB-U-392).
  if (typeof window === "undefined") return Promise.resolve();
  desktopLoading ??= import("../screen/desktop-words").then(
    (loaded) => {
      desktopWords = loaded;
      desktopFailed = false;
      for (const listener of desktopArrivals) listener();
    },
    () => {
      // A chunk that failed (a tab open across a deploy): the page shows its steps with the general label, and the
      // next computer step's label asks again.
      desktopLoading = null;
      desktopFailed = true;
      for (const listener of desktopArrivals) listener();
    },
  );
  return desktopLoading;
}

/** Whether the Desktop tool's words are here, failed to load, or are on their way; `onDesktopWords` calls back on
 * each change. */
export const desktopWordsState = () =>
  desktopWords ? "ready" : desktopFailed ? "failed" : "waiting";
export function onDesktopWords(listener: () => void): () => void {
  desktopArrivals.add(listener);
  return () => desktopArrivals.delete(listener);
}
