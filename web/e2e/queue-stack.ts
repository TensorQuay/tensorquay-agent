import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, type Page } from "@playwright/test";

import type { ServerFrame } from "../src/protocol/generated/protocol";

import { parseStackFrame } from "./stack-frames";

type Ready = Extract<ServerFrame, { type: "session.ready" }>;
declare global {
  interface Window {
    __queueStatus: string[];
  }
}

/** Keep live snapshots separate from durable events and record bounded failure evidence. */
export async function observeQueue(page: Page) {
  const session = await page.context().newCDPSession(page);
  await session.send("Network.enable");
  const snapshots: Ready[] = [];
  const frames = new Map<number, ServerFrame>();
  const invalid: string[] = [];
  const errors: string[] = [];
  page.on("pageerror", (error) => {
    if (errors.length < 20) errors.push(error.message);
  });
  page.on("console", (message) => {
    if (errors.length < 20 && ["warning", "error"].includes(message.type()))
      errors.push(message.text());
  });
  session.on("Network.eventSourceMessageReceived", ({ data }) => {
    try {
      const frame = parseStackFrame(data);
      if (snapshots.length + frames.size >= 2048)
        throw new Error("Queue evidence exceeded its bound");
      if (frame.type === "session.ready") {
        expect(
          frame,
          "live snapshots must not advance the cursor",
        ).not.toHaveProperty("seq");
        snapshots.push(frame);
      } else if ("seq" in frame && frame.seq != null) {
        const previous = frames.get(frame.seq);
        if (previous)
          expect(frame, "replay must match the stored event").toEqual(previous);
        frames.set(frame.seq, frame);
      }
    } catch (error) {
      if (invalid.length < 20) invalid.push(String(error));
    }
  });
  await page.addInitScript(() => {
    window.__queueStatus = [];
    addEventListener(
      "DOMContentLoaded",
      () => {
        new MutationObserver(() => {
          const text = document.querySelector(
            '[aria-label="Run status"]',
          )?.textContent;
          if (
            text &&
            window.__queueStatus.at(-1) !== text &&
            window.__queueStatus.length < 64
          )
            window.__queueStatus.push(text);
        }).observe(document.body, {
          subtree: true,
          childList: true,
          characterData: true,
        });
      },
      { once: true },
    );
  });
  return { session, snapshots, frames, invalid, errors };
}

/**
 * Removes the person's earlier tasks: an earlier width's pair has finished and its outcomes are recorded, so each
 * width's screenshots show a sidebar with its own two tasks, not every earlier run's.
 */
export async function clearTasks(page: Page) {
  const origin = new URL(page.url()).origin;
  const listed = await page.request.get("/api/v1/conversations?limit=100");
  expect(listed.status()).toBe(200);
  const { items } = (await listed.json()) as { items: { id: string }[] };
  for (const { id } of items) {
    const removed = await page.request.delete(`/api/v1/conversations/${id}`, {
      headers: { Origin: origin },
    });
    expect(removed.status(), `delete task ${id}`).toBe(204);
  }
  await page.reload();
}

/** Hold the real admission slot with a scripted model call that answers only when stopped (the queue fixture's
 * `Hold the slot` reply waits 600 s), without sleeping. The holder's second turn gets the same held reply: prompt
 * selection keys on the task's first message and repeats its last reply (TK-U-036). */
export async function holdSlot(page: Page) {
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Hold the slot until I press Stop.");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  await expect(
    page.getByRole("button", { name: "Stop", exact: true }),
  ).toBeVisible();
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

/** A queued send is accepted once, keeps controls disabled, and survives a full replay. */
export async function queuedPrompt(page: Page, suffix: string) {
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Wait for a free slot, then answer.");
  const accepted = page.waitForResponse(
    (response) =>
      /\/conversations\/[^/]+\/messages$/.test(response.url()) &&
      response.request().method() === "POST",
  );
  await input.press("Enter");
  const response = await accepted;
  expect(response.status()).toBe(202);
  expect(response.headers()["x-tqa-execution-status"]).toBe("queued");
  await expect(page.getByLabel("Run status")).toHaveText(
    "Waiting for a free slot · next",
  );
  await expect(input).toBeDisabled();
  await expect(input).toHaveAttribute("placeholder", "Reply…");
  // WEB-U-161: the waiting is said once, in the header; the composer says what to do.
  await expect(page.getByText("You can reply once it starts.")).toHaveCount(1);
  // WEB-U-161: the duplicated words it replaced are gone: the bubble's "Queued" line and the composer's sentence.
  // Exact copy, not /queued/i: an agent's own answer may use the word (the fixture's does).
  await expect(page.getByText("Queued", { exact: true })).toHaveCount(0);
  await expect(page.getByText("Your message is queued.")).toHaveCount(0);
  await expect(page.getByText("Sending…", { exact: true })).toHaveCount(0);
  // The model and environment are in the header (WEB-U-113); while queued both are disabled.
  await expect(page.getByRole("button", { name: /^Model:/ })).toBeDisabled();
  await expect(
    page.getByRole("button", { name: /^Environment:/ }),
  ).toBeDisabled();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await queueCapture(page, `${suffix}-queued`);
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText(
    "Waiting for a free slot · next",
  );
  await expect(input).toBeDisabled();
  await expect(page.getByText(/Send a message to begin/)).toHaveCount(0);
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

/** A holder left running by a failed test would keep the only slot for 600 s and queue every later test: stop it. */
export async function release(page: Page) {
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  if (!(await stop.isVisible().catch(() => false))) return;
  // Stop acts at once (c9), with nothing to confirm.
  await stop.click().catch(() => undefined);
}

/** The holder presses Stop: its run ends as stopped and the slot is free. */
export async function stop(page: Page) {
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText("Stopped");
}

/** The queued task ran as soon as the slot was free and answered. */
export async function answered(page: Page, turn: number) {
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  await expect(
    page.getByText("The queued task ran once the slot was free."),
  ).toHaveCount(turn);
  // Finding T-d: a sleeping environment isn't news in the bar.
  await expect(
    page.locator(".conversation-bar .environment-status"),
  ).toHaveCount(0);
}

export async function queueCapture(page: Page, name: string) {
  const folder = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!folder) return;
  await mkdir(folder, { recursive: true });
  await page.screenshot({ path: join(folder, `queue-real-${name}.png`) });
}

/** Preserve exact live and durable frames even when admission or a browser assertion fails. */
export async function queueEvidence(
  page: Page,
  name: string,
  stream: Awaited<ReturnType<typeof observeQueue>>,
) {
  const folder = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!folder) return;
  await mkdir(folder, { recursive: true });
  await writeFile(
    join(folder, `queue-real-${name}.json`),
    JSON.stringify(
      {
        snapshots: stream.snapshots,
        frames: [...stream.frames.values()],
        invalid: stream.invalid,
        errors: stream.errors,
        displayed: await page.evaluate(() => window.__queueStatus),
      },
      null,
      2,
    ),
    { mode: 0o600 },
  );
  await queueCapture(page, `${name}-result`);
}
