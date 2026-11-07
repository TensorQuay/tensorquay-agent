import { afterEach, describe, expect, it, vi } from "vitest";

import { finishAll } from "../../e2e/motion";

// The audits' wait for a popover's entrance motion (e2e/motion.ts): it waits for every animation to finish, and one that
// never does fails within its bound by name, so a stalled animation is told apart from a slow host (TEST-PLAN §19a).
const motion = (
  finished: Promise<unknown>,
  extra: Record<string, unknown> = {},
) =>
  ({
    finished,
    playState: "running",
    currentTime: 120,
    ...extra,
  }) as unknown as Animation;
const element = (...animations: Animation[]) =>
  ({ getAnimations: () => animations }) as unknown as Element;

afterEach(() => {
  vi.useRealTimers();
});

describe("WEB-U-232 settled() waits for a popover's motion", () => {
  it("resolves once every animation has finished", async () => {
    await expect(
      finishAll(
        [
          element(motion(Promise.resolve())),
          element(motion(Promise.resolve())),
        ],
        1_000,
      ),
    ).resolves.toBeUndefined();
  });

  it("names an animation that never finishes once its bound has passed", async () => {
    vi.useFakeTimers();
    const waiting = finishAll(
      [
        element(
          motion(Promise.resolve(), { playState: "finished" }),
          motion(new Promise(() => undefined), {
            animationName: "lift-in",
            playState: "paused",
            currentTime: 80,
          }),
        ),
      ],
      5_000,
    );
    const outcome = expect(waiting).rejects.toThrow(
      "A popover's motion did not finish within 5000 ms: lift-in (paused at 80 ms)",
    );
    await vi.advanceTimersByTimeAsync(5_000);
    await outcome;
  });

  it("fails at once, not at its bound, when an animation is cancelled", async () => {
    vi.useFakeTimers();
    const cancelled = new DOMException("cancelled", "AbortError");
    await expect(
      finishAll([element(motion(Promise.reject(cancelled)))], 5_000),
    ).rejects.toBe(cancelled);
  });
});
