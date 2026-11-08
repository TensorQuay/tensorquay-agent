import type { stepWords } from "./step-words";

type StepWords = typeof stepWords;

let loaded: StepWords | null = null;
let loading: Promise<StepWords> | null = null;

/**
 * Loads the live line's step words once (17.3), outside the task page's first scripts (WEB-U-014): a live turn asks as
 * its run goes live, and the model thinks before a first call starts, so they are here by then. A failed load is
 * forgotten, so the next live turn asks again.
 */
export function loadStepWords(): Promise<StepWords> {
  loading ??= import("./step-words").then(
    (module) => {
      loaded = module.stepWords;
      return module.stepWords;
    },
    (error: unknown) => {
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** The step words once their code is here; null before. */
export function loadedStepWords(): StepWords | null {
  return loaded;
}
