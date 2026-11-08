import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { SLOW_RUN } from "./front/slow-run.mjs";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

const style = (locator: Locator, property: string) =>
  locator.evaluate(
    (element, name) => getComputedStyle(element).getPropertyValue(name),
    property,
  );

/** The first line the slow scenario's first test run prints. */
const FIRST_OUTPUT =
  "test_addition (test_arithmetic.ArithmeticTests.test_addition) ... FAIL";

/**
 * A task running the slow scenario (the scripted slow model's long run, 65 ms chunks), held at the first time each
 * kind of step shows: the test checks the live run there, then lets it go on.
 */
async function startSlowRun(page: Page, baseURL: string | undefined) {
  if (!baseURL) throw new Error("The fake front must have a base URL");
  await signIn(page);
  const url = await createConversation(page);
  await page
    .context()
    .addCookies([{ name: "fixture_hold", value: "steps", url: baseURL }]);
  await page.goto(url);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(SLOW_RUN);
  await input.press("Enter");
  const task = url.split("/").at(-1) ?? "";
  const group = page.getByRole("group", { name: "Activity" });
  const line = group.locator(".activity-live");
  const words = line.locator(".shimmer");
  return {
    line,
    group,
    /**
     * Waits until the run holds at a step, and gives the live line's words there. A resume sent before the hold is
     * ignored, and the run would then stay held: the model's thinking holds once it shows (the line has become the
     * button that opens it), its answer once its first words show (no live line then), a test run's output once its
     * first line shows, a call once it has started.
     */
    reach: async (name: string, said: string | null) => {
      if (said === null) {
        await expect(
          group.locator(".activity-narration .stream-caret"),
        ).toHaveCount(1);
        await expect(line).toHaveCount(0);
        return null;
      }
      const held =
        name === "thinking"
          ? line.getByRole("button").locator(".shimmer")
          : words;
      await expect(held).toHaveText(said);
      if (name === "output")
        await expect(group.getByRole("group", { name: "Result" })).toHaveText(
          FIRST_OUTPUT,
        );
      return held;
    },
    // The words and the time after them: the line itself, or its button when it opens the thinking.
    said: words.locator(".."),
    // Sent only once the held state shows: a resume sent earlier is ignored, and the run would then stay held.
    resume: async () =>
      expect(
        (
          await page.request.post(
            `/api/v1/conversations/${task}/fixture-resume`,
          )
        ).status(),
      ).toBe(204),
  };
}

/**
 * The slow scenario's steps, in the order its run first shows each kind: what the live line says then (none while the
 * answer streams).
 */
const STEPS = [
  ["thinking", "Thinking…"],
  ["answer", null],
  ["writing", "Writing README.md…"],
  ["reading", "Reading arithmetic.py…"],
  ["listing", "Looking for Python files…"],
  ["tests", "Running the tests…"],
  ["output", "Running the tests…"],
  ["searching", "Looking for “return”…"],
  ["editing", "Editing arithmetic.py…"],
] as const;

// WEB-E2E-103: the live line names the step the run is on, from its call, with a soft shimmer across its words and
// the step's time after them (17.3; UI-GUIDELINES §8 and §13).
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-103 the live line names each step ${theme}`, async ({
    page,
    baseURL,
  }) => {
    test.setTimeout(90_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({
      colorScheme: theme,
      reducedMotion: "no-preference",
    });
    const run = await startSlowRun(page, baseURL);
    for (const [name, said] of STEPS)
      await test.step(`${name}: "${said}" shimmers, with its time`, async () => {
        const words = await run.reach(name, said);
        if (!words) {
          // The answer streams with its caret: no live line says anything then.
          await capture(page, `live-steps-${name}-${theme}`);
          await run.resume();
          return;
        }
        await expect(run.said).toHaveText(/ · \d+ s$/);
        expect(await style(words, "animation-name")).toBe("live-shimmer");
        expect(await style(words, "background-image")).toContain(
          "linear-gradient",
        );
        await capture(page, `live-steps-${name}-${theme}`);
        await run.resume();
      });
    await test.step("done: no live line, the work folded into its summary", async () => {
      await expect(
        page.getByRole("button", { name: /^Worked for/ }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(run.line).toHaveCount(0);
      await capture(page, `live-steps-done-${theme}`);
    });
  });

/** Where the run holds: how many calls have started, and the words of the one running. */
const LINES: Record<
  (typeof STEPS)[number][0],
  { started: number; running: string | null }
> = {
  thinking: { started: 0, running: null },
  answer: { started: 0, running: null },
  writing: { started: 1, running: "Write demo/README.md" },
  reading: { started: 3, running: "Read demo/arithmetic.py" },
  listing: { started: 5, running: "Looking for Python files…" },
  tests: {
    started: 6,
    running: "Running a command: python3 -B -m unittest discover -s demo -v…",
  },
  output: {
    started: 6,
    running: "Running a command: python3 -B -m unittest discover -s demo -v…",
  },
  searching: {
    started: 7,
    running: "Looking for “return” in demo/arithmetic.py…",
  },
  editing: { started: 8, running: "Edit demo/arithmetic.py" },
};

// WEB-E2E-104: each call is its own line from the moment it starts, with a small turning mark, and settles into its
// done line with how long it took; a running command's output grows in its open card (17.3; UI-GUIDELINES §8, §13).
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-104 each call is its own line, then its done line ${theme}`, async ({
    page,
    baseURL,
  }) => {
    test.setTimeout(90_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({
      colorScheme: theme,
      reducedMotion: "no-preference",
    });
    const run = await startSlowRun(page, baseURL);
    const group = page.getByRole("group", { name: "Activity" });
    const lines = group.locator(".activity-line");
    const running = group.locator(".activity-line:has(.activity-working)");
    const settled = group.locator(
      ".activity-line:not(:has(.activity-working)) .activity-duration",
    );
    for (const [name, said] of STEPS)
      await test.step(`${name}: the calls started so far, the running one turning`, async () => {
        await run.reach(name, said);
        const { started, running: words } = LINES[name];
        await expect(lines).toHaveCount(started);
        // Every call before the running one has settled into its done line, with how long it took.
        await expect(settled).toHaveCount(Math.max(started - 1, 0));
        if (words === null) await expect(running).toHaveCount(0);
        else {
          await expect(running).toHaveCount(1);
          await expect(running.locator(".activity-label")).toHaveText(words);
          // Its state is never motion alone: assistive technology reads "Running" in its name.
          await expect(
            running.locator(".activity-toggle"),
          ).toHaveAccessibleName(/Running/);
          await expect(running.locator(".activity-duration")).toHaveCount(0);
          expect(
            await style(
              running.locator(".activity-working svg"),
              "animation-name",
            ),
          ).toBe("spin");
        }
        if (name === "output")
          await expect(
            running.getByRole("group", { name: "Result" }),
          ).toHaveText(FIRST_OUTPUT);
        await capture(page, `call-lines-${name}-${theme}`);
        await run.resume();
      });
    await test.step("done: every call's line settled, with its duration", async () => {
      const summary = page.getByRole("button", { name: /^Worked for/ });
      await expect(summary).toBeVisible({ timeout: 30_000 });
      await summary.click();
      await expect(lines).toHaveCount(12);
      await expect(settled).toHaveCount(12);
      await expect(running).toHaveCount(0);
      await capture(page, `call-lines-done-${theme}`);
    });
  });

/** The slow scenario's first step's thinking, as its model streams it. */
const FIRST_THOUGHT =
  "A small arithmetic module makes the test failure easy to explain.";

// WEB-E2E-105: while the model thinks, its thinking streams in an open panel under the live line, fading in as it
// arrives; once the answer starts, it folds to "Thought for N s" where it was, and no live line speaks over the answer
// (17.3; UI-GUIDELINES §8).
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-105 thinking shows itself, then folds once the answer starts ${theme}`, async ({
    page,
    baseURL,
  }) => {
    test.setTimeout(90_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({
      colorScheme: theme,
      reducedMotion: "no-preference",
    });
    const run = await startSlowRun(page, baseURL);
    const panel = run.group.locator(".reasoning-live");
    const folded = run.group.locator("details.reasoning");
    await test.step("thinking: open under the live line, fading in", async () => {
      await run.reach("thinking", "Thinking…");
      await expect(run.line.getByRole("button")).toHaveAttribute(
        "aria-expanded",
        "true",
      );
      await expect(panel).toBeVisible();
      await expect(panel).toHaveText(FIRST_THOUGHT);
      expect(
        await style(panel.locator(".arriving").first(), "animation-name"),
      ).toBe("fade-in");
      await expect(folded).toHaveCount(0);
      await capture(page, `thinking-panel-thinking-${theme}`);
      await run.resume();
    });
    await test.step("answer: folded to “Thought for N s” where it was", async () => {
      await run.reach("answer", null);
      await expect(panel).toHaveCount(0);
      await expect(folded).toHaveCount(1);
      await expect(folded).not.toHaveAttribute("open");
      await expect(folded.locator("summary")).toHaveText(/^Thought for /);
      await expect(folded).toContainText(FIRST_THOUGHT);
      // §8: the caret after the answer's last word blinks.
      expect(
        await style(
          run.group.locator(".activity-narration .stream-caret"),
          "animation-name",
        ),
      ).toBe("caret-blink");
      await capture(page, `thinking-panel-answer-${theme}`);
      await run.resume();
    });
    await test.step("done: each step's thinking folded in its place", async () => {
      for (const [name, said] of STEPS.slice(2)) {
        await run.reach(name, said);
        await run.resume();
      }
      const summary = page.getByRole("button", { name: /^Worked for/ });
      await expect(summary).toBeVisible({ timeout: 30_000 });
      await summary.click();
      await expect(folded).toHaveCount(6);
      for (const each of await folded.all())
        await expect(each.locator("summary")).toHaveText(/^Thought for /);
      await capture(page, `thinking-panel-done-${theme}`);
    });
  });

test("WEB-E2E-103/104/105 reduced motion: the live line's words, a running call's mark, the thinking and the caret are still", async ({
  page,
  baseURL,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  const run = await startSlowRun(page, baseURL);
  for (const [name, said] of STEPS.slice(0, 3))
    await test.step(`${name}: "${said}" without the shimmer`, async () => {
      const words = await run.reach(name, said);
      if (!words) {
        // WEB-E2E-105: the caret holds still.
        expect(
          await style(
            run.group.locator(".activity-narration .stream-caret"),
            "animation-name",
          ),
        ).toBe("none");
        await run.resume();
        return;
      }
      // WEB-E2E-105: the thinking arrives without fading, its end state.
      if (name === "thinking")
        expect(
          await style(
            run.group.locator(".reasoning-live .arriving").first(),
            "animation-name",
          ),
        ).toBe("none");
      expect(await style(words, "animation-name")).toBe("none");
      expect(await style(words, "background-image")).toBe("none");
      expect(await style(words, "color")).toBe(
        await style(words.locator(".."), "color"),
      );
      // WEB-E2E-104: a running call's mark is still too.
      if (name === "writing")
        expect(
          await style(
            page.locator(".activity-line .activity-working svg"),
            "animation-name",
          ),
        ).toBe("none");
      expect(await axeViolations(page)).toEqual([]);
      await run.resume();
    });
});
