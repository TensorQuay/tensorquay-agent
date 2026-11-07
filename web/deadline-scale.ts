// The harness's load scale (crates/tqa-testkit/src/deadline.rs): `TQA_TEST_DEADLINE_SCALE`, an integer from 1 to 20,
// else 1. It scales liveness bounds (how long a test waits for something on its way), never a timing assertion. The
// unit tests' caps and the stack specs' start bounds both come from it.

/** The largest scale accepted, as the Rust harness's `MAX_SCALE`. */
export const MAX_SCALE = 20;

/**
 * The scale for `value`: an integer from 1 to {@link MAX_SCALE}, else 1 (unset, out of range or not a number; spaces
 * around it are ignored).
 */
export function parseScale(value: string | undefined): number {
  const text = value?.trim() ?? "";
  if (!/^\d+$/.test(text)) return 1;
  const scale = Number(text);
  return scale >= 1 && scale <= MAX_SCALE ? scale : 1;
}

/**
 * The unit tests' caps at `scale`: Testing Library's wait (a findBy… or waitFor) and each test's own, from their
 * defaults of 1 s and 5 s. A wait resolves as soon as its element is there, so a passing test costs no more; at scale 1
 * a test that is seconds late still fails, and the gate run's 4 rides out a host stalled by other jobs (WEB-U-074).
 */
export function unitCaps(scale: number): {
  asyncUtilMs: number;
  testMs: number;
} {
  return { asyncUtilMs: 1_000 * scale, testMs: 5_000 * scale };
}

/** The most unit-test workers accepted, and the count used without a valid `TQA_VITEST_MAX_WORKERS`. */
export const MAX_WORKERS = 8;
export const DEFAULT_WORKERS = 2;

/**
 * The unit tests' worker count for `value` (`TQA_VITEST_MAX_WORKERS`): an integer from 1 to {@link MAX_WORKERS}, else
 * {@link DEFAULT_WORKERS} (unset, out of range or not a number; spaces around it are ignored), so a loaded host's runs
 * can ask for fewer.
 */
export function parseWorkers(value: string | undefined): number {
  const text = value?.trim() ?? "";
  if (!/^\d+$/.test(text)) return DEFAULT_WORKERS;
  const workers = Number(text);
  return workers >= 1 && workers <= MAX_WORKERS ? workers : DEFAULT_WORKERS;
}
