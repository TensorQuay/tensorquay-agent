// How long a stack spec waits for a task's environment to start, from the product's own bounds, not a guess.
//
// A start is the sandbox's create, its retries on a busy engine included, bounded by the create budget at 120 s
// (`[sandbox] create_budget_seconds`, its default, `DEFAULT_CREATE_BUDGET` in crates/tqa-sandbox/src/retry.rs; each
// attempt's own 60 s bound is cut to it, SBX-U-014), then the runtime's readiness, bounded by the gateway at 30 s
// (`[sandbox] ready_seconds`, its default and its largest, crates/tqa-gateway/src/config/sandbox.rs). Until both have
// run out, the product hasn't failed the start, so a spec that gives up sooner reports its own impatience: WEB-E2E-004's
// "Starting…" failures on a loaded host were a 44 s container create, inside the product's bound, against a 30 s wait
// for Done (13h, 1 Oct 2026).
//
// Like every harness liveness bound (crates/tqa-testkit/src/deadline.rs), these scale with `TQA_TEST_DEADLINE_SCALE`:
// an integer from 1 to 20, else 1. They bound waits; they never assert product timing.

import { parseScale } from "../deadline-scale";

export { parseScale };

export const DEADLINE_SCALE = parseScale(process.env.TQA_TEST_DEADLINE_SCALE);

/** The sandbox create's budget (its retries included) and the runtime's readiness bound, in the product. */
const CREATE_MS = 120_000;
const READY_MS = 30_000;

/** The longest a task's environment may take to start before the product itself calls the start failed, scaled. */
export const START_MS = (CREATE_MS + READY_MS) * DEADLINE_SCALE;

/** A liveness bound of the harness's own, scaled. */
export const scaled = (ms: number) => ms * DEADLINE_SCALE;

/** A wait that covers a start and then `runMs` of the run's own work: the first run of a task, or one after a switch. */
export const afterStart = (runMs: number) => START_MS + scaled(runMs);

/** A test's budget: room for `starts` starts, plus `restMs` for everything else it does. */
export const budget = (starts: number, restMs: number) =>
  starts * START_MS + scaled(restMs);
