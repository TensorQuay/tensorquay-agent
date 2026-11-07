// The fake front's shared state goes back to its seed before every e2e test (e2e/test.ts), so each test, and each
// evidence screenshot, sees a fresh install plus what the test itself made, never what earlier tests left behind.
const resets = [];

/** Runs `reset(scenarios)` at every reset of the fake front, after the stores are back to their seeds. */
export function onReset(reset) {
  resets.push(reset);
}

/** A Map that starts from `entries` and goes back to a copy of them at every reset. */
export function seededMap(entries = []) {
  const seed = structuredClone([...entries]);
  const map = new Map(structuredClone(seed));
  resets.unshift(() => {
    map.clear();
    for (const [key, value] of structuredClone(seed)) map.set(key, value);
  });
  return map;
}

/** A Set that starts empty and is emptied at every reset. */
export function seededSet() {
  const set = new Set();
  resets.unshift(() => set.clear());
  return set;
}

/** Back to the seed, then the named scenarios' extra data (see each module's `onReset`). */
export function resetFront(scenarios = []) {
  for (const reset of resets) reset(scenarios);
}
