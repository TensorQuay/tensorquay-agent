import { writeFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  edgeLead,
  type EdgeHelper,
  events,
  helperStates,
  leadRow,
  stopIfRunning,
  stopStarted,
  team,
  writing,
} from "./agents-stack-api";
import { recordTexts } from "./header-texts";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// The owner's edge cases that agents-stack.spec.ts doesn't cover (E2E-AGENTS-026..030; the acceptance bar of 7 Oct,
// point 2) and 16n's and 16p's team fixes (E2E-AGENTS-031..035), on the real stack with the scripted model: each lead's
// EDGE- phrase picks its script in deploy/tests/fixtures/agents.json. They run before agents-stack.spec.ts (files run
// in name order on one worker), so E2E-AGENTS-020 stays last. Every case stops its team at the end; the afterEach stops
// any a failure left running.
test.skip(
  process.env.TQA_STACK_SUITE !== "agents",
  "Requires the agents stack suite",
);

test.afterEach(async ({ page, baseURL }) => {
  await stopStarted(page, baseURL);
});

/** Sets an administrator's limit for the test and returns how to put it back (a reset names the version it read). */
async function limit(page: Page, baseURL: string, key: string, value: number) {
  const listed = await page.request.get("/api/v1/admin/limits");
  expect(listed.ok()).toBe(true);
  const found = (
    (await listed.json()) as { items: { key: string; version: number }[] }
  ).items.find((item) => item.key === key);
  if (!found) throw new Error(`The gateway lists no ${key}`);
  const set = await page.request.put(`/api/v1/admin/limits/${key}`, {
    headers: writing(baseURL),
    data: { value, version: found.version },
  });
  expect(set.ok(), `set ${key} to ${value}`).toBe(true);
  const { version } = (await set.json()) as { version: number };
  return async () => {
    const reset = await page.request.delete(
      `/api/v1/admin/limits/${key}?version=${version}`,
      { headers: writing(baseURL) },
    );
    expect(reset.ok(), `reset ${key}`).toBe(true);
  };
}

/** A file in `task`'s workspace, as the Files panel reads it. */
async function fileText(page: Page, task: string, path: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${task}/files/content?path=${encodeURIComponent(path)}`,
  );
  expect(response.status(), path).toBe(200);
  return ((await response.json()) as { text: string | null }).text;
}

/** A catalogue key shown as text, such as "budget.output_tokens": the page must say words, never its keys. */
const RAW_KEY = /\b(?:team|budget|run|activity|notice|limits)\.[a-z]\w*\b/;

test("E2E-AGENTS-026 the lead's own Stop mid-run: its helper goes on, and the strip says so with Stop team", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-F stop the lead while a helper works";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "holder-one": "working" });
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  // The lead's second reply is held, so its run is still going: its Stop says it stops only the lead's step.
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toHaveAccessibleDescription(
    "Stops the lead's own step; its helpers keep working and can wake it. To stop them all, use Stop team in ⋯.",
    { timeout: afterStart(60_000) },
  );
  await stop.click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
    timeout: afterStart(60_000),
  });
  // The helper goes on, and the team with it; the strip says so, with Stop team, for a touch screen too (WEB-U-307).
  expect(await helperStates(page, leadId)).toEqual({ "holder-one": "working" });
  expect((await team(page, leadId))?.state).toBe("running");
  // The line sits under the strip, beside it on the lead's page; its Stop team is the only button of that name until
  // the task's ⋯ menu opens.
  await expect(page.getByText("Helpers are still working.")).toBeVisible({
    timeout: afterStart(30_000),
  });
  await page.getByRole("button", { name: "Stop team", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Stop the team?" })
    .getByRole("button", { name: "Stop team" })
    .click();
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(60_000),
    })
    .toBe("ended");
  expect(await helperStates(page, leadId)).toEqual({ "holder-one": "stopped" });
});

test("E2E-AGENTS-027 the team's budget used up: the team stops with its end note, in words", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  // The floor an administrator may set; it applies from the next team, so before the lead starts.
  const reset = await limit(page, baseURL, "agent_teams.token_budget", 10_000);
  try {
    const phrase = "EDGE-G use up the team's budget";
    const leadId = await edgeLead(page, baseURL, phrase);
    // spender's one reply reports 12,000 tokens: over the budget, so the team stops for it.
    await expect
      .poll(async () => (await team(page, leadId))?.stop_reason ?? null, {
        timeout: afterStart(120_000),
      })
      .toBe("budget");
    await expect
      .poll(async () => (await team(page, leadId))?.state, {
        timeout: afterStart(60_000),
      })
      .toBe("ended");
    expect((await helperStates(page, leadId)).spender).not.toBe("working");
    await page.goto(`/c/${encodeURIComponent(leadId)}`);
    await expect(
      page.getByText("The team ran out of budget before it finished."),
    ).toBeVisible({ timeout: afterStart(30_000) });
    // Words only: no catalogue key shown as text anywhere on the lead's page (the owner once saw one).
    await expect(page.locator("body")).not.toContainText(RAW_KEY);
  } finally {
    await reset();
  }
});

test("E2E-AGENTS-028 a hand-back taken twice: the second copy gets the next free name, nothing is overwritten", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-H take a helper's files twice";
  const leadId = await edgeLead(page, baseURL, phrase);
  // The lead takes writer's report twice: take_files never writes over a file, so the second is placed beside it.
  await expect
    .poll(
      async () =>
        (await events(page, leadId)).filter(
          (e) =>
            e.type === "tool.result" && /Placed 1 file/.test(e.content ?? ""),
        ).length,
      { timeout: afterStart(180_000) },
    )
    .toBe(2);
  const placed = (await events(page, leadId))
    .filter(
      (e) => e.type === "tool.result" && /Placed 1 file/.test(e.content ?? ""),
    )
    .map((e) => e.content ?? "");
  expect(placed[0]).toContain("notes.md as uploads/writer-notes.md");
  expect(placed[1]).toContain("notes.md as uploads/writer-notes-2.md");
  for (const path of ["uploads/writer-notes.md", "uploads/writer-notes-2.md"])
    expect(await fileText(page, leadId, path)).toBe("Notes from the writer.\n");
  await stopIfRunning(page, baseURL, leadId);
});

test("E2E-AGENTS-029 a helper ends without reporting, then the team idles: the notice and the idle stop", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  // Two one-minute limits and the reconciler's 15 s pass: about three minutes on a quiet stack.
  test.setTimeout(budget(3, 600_000));
  await bootstrap(page);
  const resetIdle = await limit(
    page,
    baseURL,
    "agent_teams.member_idle_minutes",
    1,
  );
  try {
    const resetStop = await limit(
      page,
      baseURL,
      "agent_teams.idle_stop_minutes",
      1,
    );
    try {
      const phrase = "EDGE-I a helper goes quiet and the team idles";
      const leadId = await edgeLead(page, baseURL, phrase);
      await expect
        .poll(async () => (await team(page, leadId))?.stop_reason ?? null, {
          timeout: afterStart(360_000),
          intervals: [5_000],
        })
        .toBe("idle");
      await page.goto(`/c/${encodeURIComponent(leadId)}`);
      await expect(
        page.getByText("quiet stopped without reporting."),
      ).toBeVisible({ timeout: afterStart(30_000) });
      await expect(
        page.getByText(
          "The team stopped after a while with nothing happening.",
        ),
      ).toBeVisible();
      await expect(page.locator("body")).not.toContainText(RAW_KEY);
    } finally {
      await resetStop();
    }
  } finally {
    await resetIdle();
  }
});

test("E2E-AGENTS-030 two teams of one person at once: each runs its own helper and stops on its own", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const first = "EDGE-J first of two teams";
  const second = "EDGE-J second of two teams";
  const firstId = await edgeLead(page, baseURL, first);
  const secondId = await edgeLead(page, baseURL, second);
  for (const leadId of [firstId, secondId])
    await expect
      .poll(async () => helperStates(page, leadId), {
        timeout: afterStart(180_000),
      })
      .toEqual({ "holder-one": "working" });
  // Each lead's page shows its own helper only, and the task list counts one under each.
  for (const leadId of [firstId, secondId]) {
    await page.goto(`/c/${encodeURIComponent(leadId)}`);
    await expect(
      page.getByRole("region", { name: "Helpers" }).getByRole("link"),
    ).toHaveCount(1);
    await expect(leadRow(page, leadId)).toHaveAccessibleName(/1 helper$/, {
      timeout: afterStart(45_000),
    });
  }
  // Stopping one team leaves the other running.
  await stopIfRunning(page, baseURL, firstId);
  expect((await team(page, secondId))?.state).toBe("running");
  expect(await helperStates(page, secondId)).toEqual({
    "holder-one": "working",
  });
  await stopIfRunning(page, baseURL, secondId);
});

/** The lead task's messages as its composer lists them (held, sent or dropped; a cancelled one is gone). */
async function queued(page: Page, leadId: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${leadId}/messages/queued`,
  );
  expect(response.ok()).toBe(true);
  const { items } = (await response.json()) as {
    items: { text: string; state: string }[];
  };
  return items.map(({ text, state }) => ({ text, state }));
}

/** How many runs `task` has started. */
async function runsStarted(page: Page, task: string) {
  return (await events(page, task)).filter((e) => e.type === "run.started")
    .length;
}

// E2E-AGENTS-031 (16n T2, GW-I-307; c16k real-world B5: "Add MySQL as a fourth database" waited until the whole team
// ended): while the lead's run is active, a person's message is held as before, cancellable, and copied into the team's
// log; the lead reads it at its next check of the team, and it then never starts a run of its own. EDGE-K's lead starts
// a helper, and its next model call holds a minute before it reads its team, so the message arrives during its run.
test("E2E-AGENTS-031 a message to a leading lead: held, read at its next check of the team, never a run of its own", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 360_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-K steer the team while it works",
  );
  // The team runs from the lead's start_member; its read comes a minute later.
  await expect
    .poll(async () => (await team(page, leadId))?.state ?? null, {
      timeout: afterStart(120_000),
    })
    .toBe("running");
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const input = page.getByRole("textbox", { name: "Message" });
  for (const text of ["Add MySQL as a fourth database.", "Drop DuckDB."]) {
    await input.fill(text);
    await input.press("Enter");
  }
  const held = page.getByRole("list", {
    name: "Messages waiting for the agent",
  });
  const item = (text: string) =>
    held.getByRole("listitem").filter({ hasText: text });
  await expect(item("Add MySQL as a fourth database.")).toContainText(
    "The lead gets this at its next check of the team: Add MySQL as a fourth database.",
  );
  await expect(
    item("Add MySQL as a fourth database.").getByRole("button", {
      name: "Cancel",
    }),
  ).toBeVisible();
  await expect(
    page.getByText("You told the lead: Add MySQL as a fourth database."),
  ).toBeVisible();
  // Cancelled before the lead reads it: it never reaches the lead.
  await item("Drop DuckDB.").getByRole("button", { name: "Cancel" }).click();
  await expect(item("Drop DuckDB.")).toHaveCount(0);
  // The lead reads at its next check, then finishes the team.
  await expect
    .poll(async () => (await team(page, leadId))?.stop_reason ?? null, {
      timeout: afterStart(150_000),
    })
    .toBe("done");
  const reads = (await events(page, leadId))
    .filter((e) => e.type === "tool.result")
    .map((e) => e.content ?? "");
  expect(
    reads.filter((content) =>
      content.includes(
        "The person wrote, while the team works: Add MySQL as a fourth database.",
      ),
    ),
  ).toHaveLength(1);
  expect(reads.filter((content) => content.includes("Drop DuckDB."))).toEqual(
    [],
  );
  // Delivered by the read: sent, nothing waiting, and no second run for it, even after the lead's run ended.
  expect(await queued(page, leadId)).toEqual([
    { text: "Add MySQL as a fourth database.", state: "sent" },
  ]);
  await page.waitForTimeout(10_000);
  expect(await runsStarted(page, leadId)).toBe(1);
  await expect(item("Add MySQL as a fourth database.")).toHaveCount(0);
  await expect(page.locator("body")).not.toContainText(RAW_KEY);
});

// E2E-AGENTS-032 (16n T3, GW-I-306, WEB-U-398; c16k real-world B2: the lead ended its turn without done and its header
// said Done while the team ran on): EDGE-L's lead starts a helper that reports and finishes, reads its report, and ends
// every turn without done. The gateway reminds it only once nothing else can wake it (its helper quiet, nothing
// unread; the 16n integration), twice, never more; its header is never Done while the team runs, and after the second
// reminder says the lead stopped without finishing, with what to do. The team keeps running.
test("E2E-AGENTS-032 a lead that ends its turns without done: two reminders, never Done, then the lead stopped", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 360_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-L end a turn without done",
  );
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const status = page.getByLabel("Run status");
  const stopped =
    "The lead stopped without finishing the team. Stop the team, or send a message.";
  // The header's words over the whole case: the waiting line shows only between the lead's runs, so it is logged,
  // and Done must never show while the team runs.
  const seen = new Set<string>();
  // Each sample, and every text the header held between them (a one-render Done no poll reads, WEB-U-415), saved with
  // the case's evidence whatever its outcome: the header's words, then the team's state as read after them.
  await expect(status).toBeVisible();
  const texts = await recordTexts(page);
  const samples: { at: string; header: string; team: string | null }[] = [];
  const save = async () => {
    const path = test.info().outputPath("header-samples.json");
    await writeFile(
      path,
      JSON.stringify({ samples, texts: await texts() }, null, 1),
    );
    await test.info().attach("header samples", {
      path,
      contentType: "application/json",
    });
  };
  try {
    await expect
      .poll(
        async () => {
          const at = new Date().toISOString();
          const words = ((await status.textContent()) ?? "").trim();
          const state = (await team(page, leadId))?.state ?? null;
          samples.push({ at, header: words, team: state });
          if (state === "running") seen.add(words);
          return words;
        },
        { timeout: afterStart(240_000), intervals: [250] },
      )
      .toBe(stopped);
  } finally {
    await save();
  }
  test.info().annotations.push({
    type: "header words while the team ran",
    description: [...seen].join(" | "),
  });
  expect([...seen].filter((words) => words.startsWith("Done"))).toEqual([]);
  const reminder =
    "The lead's turn ended without finishing the team: it was reminded to call done.";
  await expect(page.getByText(reminder)).toHaveCount(2);
  // Never a third: the count holds, and the team still runs for its person to stop or steer.
  const runs = await runsStarted(page, leadId);
  await page.waitForTimeout(20_000);
  expect(await runsStarted(page, leadId)).toBe(runs);
  await expect(page.getByText(reminder)).toHaveCount(2);
  await expect(status).toHaveText(stopped);
  expect((await team(page, leadId))?.state).toBe("running");
  await expect(page.locator("body")).not.toContainText(RAW_KEY);
  await stopIfRunning(page, baseURL, leadId);
});

/** An administrator's limit as the gateway applies it now. */
async function limitValue(page: Page, key: string) {
  const listed = await page.request.get("/api/v1/admin/limits");
  expect(listed.ok()).toBe(true);
  const found = (
    (await listed.json()) as { items: { key: string; value: unknown }[] }
  ).items.find((item) => item.key === key);
  if (typeof found?.value !== "number")
    throw new Error(`The gateway lists no number for ${key}`);
  return found.value;
}

/** Each helper's own task, by name, as the team route says it. */
async function helperTasks(page: Page, leadId: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${leadId}/team`,
  );
  expect(response.status()).toBe(200);
  const { helpers } = (await response.json()) as { helpers: EdgeHelper[] };
  return Object.fromEntries(
    helpers.map((helper): [string, string] => [
      helper.name,
      helper.conversation,
    ]),
  );
}

/** What a person reads: the page's rendered text, never a folded Result's hidden one. */
const READ = { useInnerText: true } as const;

test("E2E-AGENTS-033 two helpers on the lead's files: each waits its turn on them, said as such, never a free slot", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  // Room for the lead and both helpers at once: what they wait for is the lead's files, never a slot (c16k B10b read
  // "Waiting for a free slot · next" with slots free).
  for (const key of ["admission.slots", "admission.per_user"])
    expect(await limitValue(page, key), key).toBeGreaterThanOrEqual(3);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-M two helpers on the lead's files",
  );
  // The lead's own run holds its files, so files-one waits for the lead and files-two behind files-one.
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "files-one": "queued", "files-two": "queued" });
  const tasks = await helperTasks(page, leadId);
  const turn = "Waiting for its turn on the lead's files";
  // WEB-U-419: each helper's composer names the task holding the files' lease, as the workspace's lease gives its
  // title, clipped to 60 code points with an ellipsis inside the quotes. Here the lead's run holds it.
  const composerWait = async (conversation: string) => {
    const task = (await (
      await page.request.get(`/api/v1/conversations/${conversation}`)
    ).json()) as { workspace_id: string };
    const files = (await (
      await page.request.get(`/api/v1/workspaces/${task.workspace_id}`)
    ).json()) as {
      lease?: { conversation_id: string; title?: string | null } | null;
    };
    expect(files.lease?.conversation_id, "the lead holds its files").toBe(
      leadId,
    );
    const title = [...(files.lease?.title ?? "")];
    expect(title.length, "the holder has a title").toBeGreaterThan(0);
    const named =
      title.length <= 60
        ? title.join("")
        : `${title.slice(0, 59).join("").trimEnd()}…`;
    return `Its turn on the lead's files comes when “${named}” is done with them. You can reply once it starts.`;
  };
  for (const name of ["files-one", "files-two"]) {
    await page.goto(`/c/${encodeURIComponent(tasks[name] ?? "")}`);
    const status = page.getByLabel("Run status");
    await expect(status, name).toHaveText(new RegExp(`^${turn}`), {
      timeout: afterStart(30_000),
    });
    // The header's one short word, and the whole in its tooltip (WEB-U-393: at 1280 beside Files it was cut).
    await expect(status).toHaveAttribute("data-short", "Waiting…");
    await expect(status).toHaveAttribute("title", new RegExp(`^${turn}`));
    await expect(status).not.toContainText("free slot");
    // WEB-U-410: the wait is said where the person would type, not only in the header; WEB-U-419: with the holder.
    const words = await composerWait(tasks[name] ?? "");
    await expect(page.getByText(words, { exact: true }), name).toBeVisible({
      timeout: afterStart(30_000),
    });
  }
  // The lead's strip says the same of each helper.
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const helpers = page.getByRole("region", { name: "Helpers" });
  for (const name of ["files-one", "files-two"])
    await expect(
      helpers.getByRole("link", { name: `${name}: ${turn}` }),
    ).toBeVisible({ timeout: afterStart(30_000) });
  await expect(helpers).not.toContainText(/free slot|slot \(/);
  await stopIfRunning(page, baseURL, leadId);
});

// E2E-AGENTS-036 (c17 U1, U3, U4 on the real stack; WEB-U-407, 408, 409): EDGE-Q's lead starts a helper on its own
// files, then calls `shell` with `exit 3` at every step (the last reply repeats), so the runtime stops its run as a
// repeated failure while the helper, which then holds the lead's files, keeps working. 45 s in, the helper reports and
// keeps the files (its next reply waits 10 min), so the lead's wake waits for them and its composer takes no message.
test("E2E-AGENTS-036 a lead whose step keeps failing while its helper works: one short word, no offer the composer can't take, no red row", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-Q a lead whose step keeps failing while its helper works",
  );
  // The lead's run failed (run_status is on the task list's items only).
  await expect
    .poll(
      async () =>
        (
          (await (
            await page.request.get("/api/v1/conversations?scope=top")
          ).json()) as { items: { id: string; run_status?: string }[] }
        ).items.find((item) => item.id === leadId)?.run_status,
      { timeout: afterStart(120_000) },
    )
    .toBe("failed");
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "slow-files": "working" });
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  // Each c17 check is soft, so a red run reports all three. WEB-U-408: the header's one short word, the whole in
  // its tooltip.
  const status = page.getByLabel("Run status");
  await expect.soft(status).toHaveAttribute("data-short", "Waiting…", {
    timeout: afterStart(30_000),
  });
  await expect
    .soft(status)
    .toHaveAttribute(
      "title",
      /^Waiting for its helpers: its last step kept failing/,
    );
  // WEB-U-407: the failed turn's next step offers a message while the composer takes one ...
  const next = page.getByText(
    "Its helpers are still working; it carries on when one reports.",
  );
  const offer = page.getByText("You can also send it a message.", {
    exact: false,
  });
  await expect.soft(next).toBeVisible({ timeout: afterStart(30_000) });
  // The helper reports 45 s after it starts: if its report already queued the lead's wake, this half tests nothing.
  const early = (
    (await (
      await page.request.get(`/api/v1/conversations/${leadId}/messages/queued`)
    ).json()) as { items: { team_wake?: unknown; state: string }[] }
  ).items.some((item) => item.team_wake && item.state !== "sent");
  expect(
    early,
    "setup: the helper's report came before the page was read",
  ).toBe(false);
  await expect(offer, "the composer is open before the report").toHaveCount(1);
  // WEB-U-409: the task list shows the lead without a red Failed while its team runs.
  const row = leadRow(page, leadId);
  await expect.soft(row).toHaveAccessibleName(/1 helper$/, {
    timeout: afterStart(30_000),
  });
  await expect.soft(row).not.toHaveAccessibleName(/Failed/);
  // ... and none once the helper's report has woken the lead and its run waits for the files the helper holds: the
  // gateway lists the lead's wake as waiting (read from the API, so a page without c17's words still gets here), the
  // composer says so (WEB-U-410), and takes no message (16p T4 offered one there).
  await expect
    .poll(
      async () =>
        (
          (await (
            await page.request.get(
              `/api/v1/conversations/${leadId}/messages/queued`,
            )
          ).json()) as { items: { team_wake?: unknown; state: string }[] }
        ).items.some((item) => item.team_wake && item.state !== "sent"),
      { timeout: afterStart(120_000) },
    )
    .toBe(true);
  await expect
    .soft(
      page.getByText("A helper is using the files; this task carries on", {
        exact: false,
      }),
    )
    .toBeVisible({ timeout: afterStart(30_000) });
  await expect.soft(next).toBeVisible();
  await expect.soft(offer).toHaveCount(0);
  await stopIfRunning(page, baseURL, leadId);
});

// E2E-AGENTS-038 (WEB-U-420 on the real stack): EDGE-S's lead starts a helper on its own files and ends its turn, so
// the helper takes the files; the helper reports after 45 s and keeps working in them. The lead's wake then waits for
// its files, as the team's notice. The lead's composer takes no message meanwhile (WEB-U-407), so a person's message
// can only come from another client: it is posted through the API, and waits beside the wake. The page reads both
// from the gateway's list when it opens (a message only waiting sends no stream frame, so an open page learns of
// another client's one at its next read): it lists the notice, then the message, on each open. (The other order, a
// person's message queued before the wake, doesn't happen: the gateway queues no wake for a lead with a message
// already queued (teams::wake). WEB-U-420's time-not-text order within one second is the unit test's.)
test("E2E-AGENTS-038 the team's waiting notice and the person's message after it keep the gateway's order", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-S the team's notice among the person's waiting messages",
  );
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "late-report": "working" });
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const notice = page.getByText("New team messages", { exact: true });
  await expect(notice).toBeVisible({ timeout: afterStart(120_000) });
  const run = Math.random().toString(36).slice(2, 8);
  const sent = await page.request.post(
    `/api/v1/conversations/${leadId}/messages`,
    {
      headers: writing(baseURL),
      data: { text: `After ${run}`, attachments: [] },
    },
  );
  expect(sent.ok()).toBe(true);
  // The person's waiting message: an article while it waits for a slot or the files, an item of the held list while
  // a run of the lead's holds it (GW-I-307), as the gateway lists it.
  const after = page
    .locator(
      'article[aria-label="Your message"], [aria-label="Messages waiting for the agent"] li',
    )
    .filter({ hasText: `After ${run}` });
  const queued = (await (
    await page.request.get(`/api/v1/conversations/${leadId}/messages/queued`)
  ).json()) as {
    items: { text: string; state: string; team_wake?: unknown }[];
  };
  test.info().annotations.push({
    type: "queued",
    description: JSON.stringify(
      queued.items.map((item) => [
        item.state,
        Boolean(item.team_wake),
        item.text.slice(0, 20),
      ]),
    ),
  });
  // Whether a precedes b in the page, by the located elements themselves (never by a copy of their text).
  const precedes = async (a: Locator, b: Locator) => {
    const [x, y] = [await a.elementHandle(), await b.elementHandle()];
    if (!x || !y) throw new Error("an element to order isn't there");
    return page.evaluate(
      ([one, two]) =>
        Boolean(
          one.compareDocumentPosition(two) & Node.DOCUMENT_POSITION_FOLLOWING,
        ),
      [x, y] as const,
    );
  };
  for (const visit of ["opened", "reloaded"]) {
    if (visit === "opened") await page.goto(`/c/${encodeURIComponent(leadId)}`);
    else await page.reload();
    await expect(after, visit).toHaveCount(1, { timeout: afterStart(30_000) });
    await expect(notice, visit).toHaveCount(1);
    expect(
      await precedes(notice, after),
      `${visit}: the notice, then the person's message`,
    ).toBe(true);
  }
  await stopIfRunning(page, baseURL, leadId);
});

test("E2E-AGENTS-034 a team's ends in words: Markdown end card, a failed start, a folded traceback, minutes", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  // The lead's done is held 61 s, so its run passes a minute.
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const leadId = await edgeLead(page, baseURL, "EDGE-N worded ends");
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(240_000),
      intervals: [5_000],
    })
    .toBe("ended");
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(60_000),
  });
  // The end card formats the lead's summary as its replies are: bold, never the marks (WEB-U-396, c16k B9c).
  const end = page.locator(".team-end");
  await expect(end.locator("strong")).toHaveText("Sights table");
  await expect(end).toContainText(
    "Sights table: three sights, each with its hours.",
    READ,
  );
  await expect(end).not.toContainText("**", READ);
  // A run past a minute says minutes, never "61 s" (WEB-U-395, c16k B9: "Worked for 837 s").
  const worked = page.getByRole("button", { name: /^Worked for/ }).first();
  await expect(worked).toHaveAccessibleName(
    /^Worked for 1 min( \d+ s)?( · .+)?$/,
  );
  if ((await worked.getAttribute("aria-expanded")) === "false")
    await worked.click();
  // A helper that couldn't start says so and why, in words (WEB-U-399, c16k B5: "unknown_reference: …").
  await expect(
    page.getByText("Couldn't start a helper: profiled"),
  ).toBeVisible();
  const why = page.getByText(
    "There's no agent profile named “no-such-profile”.",
  );
  if (!(await why.isVisible())) {
    const line = page
      .getByRole("button", { name: /Couldn't start a helper: profiled/ })
      .first();
    if ((await line.getAttribute("aria-expanded")) === "false")
      await line.click();
  }
  await expect(why).toBeVisible();
  // The lead's failed script reads as its error until Show more (WEB-U-400, c16k B9: ~20 frames before the error).
  const step = page.getByRole("button", { name: /python3 -c/ }).first();
  if ((await step.getAttribute("aria-expanded")) === "false")
    await step.click();
  const card = page.getByRole("article", { name: "shell tool call" });
  await expect(card).toContainText(
    "json.decoder.JSONDecodeError: Expecting value: line 1 column 1 (char 0)",
    READ,
  );
  await expect(card).not.toContainText(/Traceback|File "/, READ);
  await card.getByRole("button", { name: "Show more" }).click();
  await expect(card).toContainText(
    /File "\/usr\/lib\/python3[\d.]*\/json\//,
    READ,
  );
  // Words only, everywhere a person reads.
  const body = page.locator("body");
  await expect(body).not.toContainText("unknown_reference", READ);
  await expect(body).not.toContainText(/Worked for ([6-9]\d|\d{3,}) s\b/, READ);
  await expect(body).not.toContainText(RAW_KEY, READ);
});

// E2E-AGENTS-035 (16p T1c, GW-I-311, WEB-U-403; 16o live W1: two helpers stopped mid-run, and their files stayed in
// their own copies): EDGE-O's helper writes notes.md and ends its turn without reporting; when the team is stopped,
// the gateway hands back what it changed, as its own report in the log, and the end note lists the file.
test("E2E-AGENTS-035 a helper stopped before it reported is handed back by the gateway", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "EDGE-O hand back a stopped helper's files",
  );
  // scribe-o wrote its file and ended its turn with no report.
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(180_000),
    })
    .toEqual({ "scribe-o": "waiting_lead" });
  await stopIfRunning(page, baseURL, leadId);
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  await expect(
    page.getByText(
      "The gateway hands back scribe-o's work: scribe-o stopped before it reported (the team was stopped). Attached: 1 file it changed in its own copy; take it with take_files. Files it deleted aren't listed. · 1 file",
    ),
  ).toBeVisible({ timeout: afterStart(30_000) });
  await expect(page.getByText(/scribe-o reported/)).toHaveCount(0);
  await expect(page.getByText("notes.md").first()).toBeVisible();
  await expect(page.locator("body")).not.toContainText(RAW_KEY);
});
