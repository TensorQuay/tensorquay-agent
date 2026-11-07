import { readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import {
  edgeLead,
  events,
  helperStates,
  leadRow,
  started,
  stopIfRunning,
  stopStarted,
  team,
  writing,
  type EdgeHelper,
} from "./agents-stack-api";
import { afterStart, budget, scaled } from "./stack-start";
import { bootstrap } from "./ux/flow";

/** The account menu's language, as a person sets it (the language is the account's, kept by the gateway). */
async function chooseLanguage(page: Page, current: string, name: string) {
  await page.getByRole("button", { name: /^(Account|账户)$/ }).click();
  await page
    .getByRole("menuitem", { name: new RegExp(`· ${current}$`) })
    .click();
  await page.getByRole("menuitemradio", { name }).click();
  await page.keyboard.press("Escape");
}

// E2E-AGENTS-020 (docs/evals/agent-teams.md section 1): an agent team end to end on the real stack, with the scripted
// model in deploy/tests/fixtures/agents.json. A lead starts an analyst on its own copy and a fixer on the lead's files,
// asks both, asks its person, decides, takes the analyst's files, reads its transcript and calls done; the script runs
// twice, the second time with the gateway restarted at a named step, through deploy/tests/demo_agents.py.
test.skip(
  process.env.TQA_STACK_SUITE !== "agents",
  "Requires the agents stack suite",
);

/** The person's message: its last clause, in no helper's brief, picks the lead's script in the fixture. */
const ASK =
  "Compare Q3 revenue by region in uploads/q3.csv with the targets in uploads/targets.md, and fix the rounding in " +
  "uploads/report.py.";
const FILES: Record<string, string> = {
  "q3.csv": "region,revenue\nnorth,120\nsouth,80\n",
  "targets.md": "north: 100\nsouth: 90\n",
  "report.py": "def rounded(value):\n    return round(value)\n",
};
const COMPARISON =
  "north: 120 against a target of 100\nsouth: 80 against a target of 90\n";

test.afterEach(async ({ page, baseURL }) => {
  await stopStarted(page, baseURL);
});

type TeamMessage = {
  seq: number;
  type: string;
  from: { kind: string; name?: string };
  text?: string;
  reason?: string;
  summary?: string;
  reports?: { member: string; seq: number }[];
  files?: { member: string; seq: number; name: string; size: number }[];
};

async function messages(page: Page, lead: string): Promise<TeamMessage[]> {
  const response = await page.request.get(
    `/api/v1/conversations/${lead}/team/messages?limit=200`,
  );
  expect(response.status()).toBe(200);
  return ((await response.json()) as { items: TeamMessage[] }).items;
}

async function fileText(page: Page, task: string, path: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${task}/files/content?path=${encodeURIComponent(path)}`,
  );
  expect(response.status(), path).toBe(200);
  return ((await response.json()) as { text: string | null }).text;
}

/** Waits for `name` among the driver's markers, bounded and scaled; a missing one fails naming it. */
async function marker(name: string, failed: string) {
  const directory = process.env.TQA_AGENTS_MARKERS ?? "";
  if (!directory) throw new Error("The driver must supply TQA_AGENTS_MARKERS");
  const bound = scaled(240_000);
  const started = Date.now();
  for (;;) {
    const failure = await readFile(join(directory, failed), "utf8").catch(
      () => null,
    );
    if (failure !== null)
      throw new Error(`The driver wrote ${failed}: ${failure}`);
    if (await readFile(join(directory, name), "utf8").catch(() => null)) return;
    if (Date.now() - started > bound)
      throw new Error(`Marker ${name} missing after ${bound / 1000} s`);
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

/**
 * What the person sees once the team has ended (the eval's views, checked where nothing moves): both helpers under the
 * lead in the task list, each helper's task opening with the lead's brief as the lead's words, the lead's question in
 * its card, answered, and the end note. "Waiting for you" itself is WEB-E2E-024's: here the scripted team moves on in
 * a second or two, faster than a person could look.
 */
async function seen(page: Page, task: string) {
  await test.step("the person sees the team's tasks, the answered question and the end note", async () => {
    await page.goto(`/c/${encodeURIComponent(task)}`);
    const sidebar = page.getByRole("region", { name: "Tasks", exact: true });
    await sidebar
      .getByRole("button", { name: "Helpers of Q3", exact: true })
      .click();
    const helpers = sidebar.getByRole("list", { name: "Helpers" });
    for (const name of ["analyst", "fixer"])
      await expect(
        helpers.getByRole("link", { name: new RegExp(`^${name}:`) }),
      ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "Answered", exact: true }),
    ).toContainText("Round half up or half even?");
    await expect(
      page.getByText("The team finished.", { exact: true }),
    ).toBeVisible();
    // The end note names each helper's reports and the files they handed back, as their reports named them.
    const note = page.getByRole("region", {
      name: "The team finished.",
      exact: true,
    });
    await expect(note).toContainText(
      "1 report from analyst and 1 report from fixer",
    );
    await expect(note).toContainText(
      /Handed back: comparison\.md \(\d+ B, analyst\), uploads\/report\.py \(\d+ B, fixer\)/,
    );
    for (const helper of (await team(page, task))?.helpers ?? []) {
      await page.goto(`/c/${encodeURIComponent(helper.conversation)}`);
      await expect(
        page.getByText(`Helper of Q3 · ${helper.name}`, { exact: true }),
      ).toBeVisible();
      await expect(
        page.getByText("Brief from the lead", { exact: true }),
      ).toBeVisible();
    }
  });
}

/** One run of the script; `restart` restarts the gateway once the fixer's report is in the team's log. */
async function run(page: Page, baseURL: string, restart: boolean) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: writing(baseURL),
    data: { title: restart ? "Q3 with a restart" : "Q3", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const { id: lead } = (await created.json()) as { id: string };
  started.push(`${lead}`);
  for (const [name, text] of Object.entries(FILES)) {
    const uploaded = await page.request.post(
      `/api/v1/conversations/${lead}/uploads?name=${name}`,
      {
        headers: {
          Origin: baseURL,
          "content-type": "application/octet-stream",
        },
        data: Buffer.from(text),
      },
    );
    expect(uploaded.status(), name).toBe(201);
  }
  const sent = await page.request.post(
    `/api/v1/conversations/${lead}/messages`,
    {
      headers: writing(baseURL),
      data: {
        text: ASK,
        attachments: Object.keys(FILES).map((name) => `uploads/${name}`),
      },
    },
  );
  expect(sent.ok()).toBe(true);

  await test.step("the lead starts two helpers, each from the lead's brief", async () => {
    await expect
      .poll(
        async () => (await team(page, lead))?.helpers.map((h) => h.name).sort(),
        {
          timeout: afterStart(60_000),
        },
      )
      .toEqual(["analyst", "fixer"]);
    for (const helper of (await team(page, lead))?.helpers ?? []) {
      await expect
        .poll(
          async () =>
            (await events(page, helper.conversation)).some(
              (e) => e.type === "message.user" && e.lead_brief,
            ),
          {
            timeout: afterStart(60_000),
          },
        )
        .toBe(true);
    }
  });

  await test.step("the lead asks its person, who answers on the team's route", async () => {
    let question = 0;
    await expect
      .poll(
        async () =>
          (question = (await team(page, lead))?.waiting_for_you?.seq ?? 0),
        {
          timeout: afterStart(120_000),
        },
      )
      .toBeGreaterThan(0);
    const answered = await page.request.post(
      `/api/v1/conversations/${lead}/team/answers`,
      {
        headers: writing(baseURL),
        data: { question_seq: question, text: "Half up." },
      },
    );
    expect(answered.status()).toBe(201);
  });

  if (restart)
    await test.step("restart: the gateway restarts once the fixer's report is in the log", async () => {
      await expect
        .poll(
          async () =>
            (await messages(page, lead)).some(
              (m) => m.type === "report" && m.from.name === "fixer",
            ),
          {
            timeout: afterStart(120_000),
          },
        )
        .toBe(true);
      // A restart asked earlier in the run (E2E-AGENTS-023) left its answer: cleared, so this wait is for this one.
      await rm(join(process.env.TQA_AGENTS_MARKERS ?? "", "restarted"), {
        force: true,
      });
      await writeFile(
        join(process.env.TQA_AGENTS_MARKERS ?? "", "restart-wanted"),
        "after the fixer's report",
      );
      await marker("restarted", "restart-failed");
    });

  return await test.step("the lead calls done and the team ends with one end note", async () => {
    await expect
      .poll(
        async () => {
          const now = await team(page, lead);
          return [now?.state, now?.stop_reason];
        },
        { timeout: afterStart(180_000) },
      )
      .toEqual(["ended", "done"]);
    const log = await messages(page, lead);
    const seqs = log.map((m) => m.seq);
    expect(seqs, "every message once, in order").toEqual(
      [...new Set(seqs)].sort((a, b) => a - b),
    );
    const notes = log.filter((m) => m.type === "ended");
    expect(notes).toHaveLength(1);
    const note = notes[0];
    if (!note) throw new Error("No end note");
    expect(note.reason).toBe("done");
    expect(note.summary).toContain("analyst-comparison.md");
    expect(new Set(note.reports?.map((r) => r.member))).toEqual(
      new Set(["analyst", "fixer"]),
    );
    expect(
      note.files?.some(
        (f) => f.member === "analyst" && f.name === "comparison.md",
      ),
    ).toBe(true);
    // The analyst's file handed back, and the fixer's fix on the lead's own files.
    expect(await fileText(page, lead, "uploads/analyst-comparison.md")).toBe(
      COMPARISON,
    );
    expect(await fileText(page, lead, "uploads/report.py")).toContain(
      "return int(value + 0.5)",
    );
    // Helpers' messages reached the lead marked as untrusted content.
    const results = (await events(page, lead))
      .filter((e) => e.type === "tool.result")
      .map((e) => e.content ?? "");
    expect(
      results.some(
        (text) =>
          text.includes('<untrusted-content source="team"') &&
          text.includes('from="analyst"'),
      ),
    ).toBe(true);
    // The lead finished its answer to the person after done: its run completed, not failed (ADR-0034 point 8).
    await expect
      .poll(
        async () =>
          (await events(page, lead))
            .filter(
              (e) => e.type === "run.completed" || e.type === "run.failed",
            )
            .at(-1)?.type,
        { timeout: afterStart(60_000) },
      )
      .toBe("run.completed");
    if (!restart) await seen(page, lead);
    return {
      variant: restart ? "restart" : "plain",
      lead,
      helpers: ((await team(page, lead))?.helpers ?? []).map((h) => h.name),
      stop_reason: note.reason ?? null,
      end_notes: notes.length,
    };
  });
}

// GW-I-260 (migration 0094): a helper whose run fails is `failed` with `failed_run`, keeps its seat, and can still be
// stopped, so the sheet offers Stop helper for it (GW-U-168's can_stop, WEB-U-308).
test("E2E-AGENTS-021 a helper whose run fails: its state, the lead's notice, the list's count and its Stop", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-A brief a helper that has no script";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => (await helperStates(page, leadId)).ghost, {
      timeout: afterStart(120_000),
    })
    .toBe("failed");
  // The team goes on (one helper failing doesn't end it); the lead is told, as a quiet line (below).
  expect((await team(page, leadId))?.state).toBe("running");
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const strip = page.getByRole("region", { name: "Helpers" });
  const chip = strip.getByRole("link", { name: "ghost: Failed" });
  await expect(chip).toBeVisible();
  await expect(chip).toHaveAttribute("data-tone", "danger");
  await expect(page.getByText("ghost failed.")).toBeVisible();
  // The list says so in the count's place, in the danger tone (§13b).
  await expect(leadRow(page, leadId)).toHaveAccessibleName(/1 failed$/, {
    timeout: afterStart(45_000),
  });
  // Its seat is still taken, so it can be stopped: the team read says so, and All helpers offers Stop helper.
  const read = await page.request.get(`/api/v1/conversations/${leadId}/team`);
  const { helpers } = (await read.json()) as {
    helpers: (EdgeHelper & { can_stop: boolean })[];
  };
  expect(helpers.find((helper) => helper.name === "ghost")?.can_stop).toBe(
    true,
  );
  await strip.getByRole("button", { name: "All helpers" }).click();
  const sheet = page.getByRole("dialog", { name: "All helpers" });
  await sheet.getByRole("button", { name: "More actions for ghost" }).click();
  await expect(
    page.getByRole("menuitem", { name: "Stop helper ghost" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await stopIfRunning(page, baseURL, leadId);
});

test("E2E-AGENTS-022 Stop team while helpers work: the dialog, every helper stopped, one end note", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-B stop a team while it works";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "holder-one": "working", "holder-two": "working" });
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const task = page.getByRole("region", { name: "Task", exact: true });
  await task
    .getByRole("button", { name: `More actions for ${phrase}`, exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Stop team" }).click();
  const dialog = page.getByRole("dialog", { name: "Stop the team?" });
  await expect(dialog).toContainText(
    "The lead and 2 helpers stop now. Their tasks and files stay.",
  );
  await dialog.getByRole("button", { name: "Stop team" }).click();
  await expect(page.getByText("You stopped the team.")).toBeVisible({
    timeout: afterStart(60_000),
  });
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(60_000),
    })
    .toBe("ended");
  expect(await helperStates(page, leadId)).toEqual({
    "holder-one": "stopped",
    "holder-two": "stopped",
  });
  // WEB-U-413: the lead whose team its person stopped reads Stopped, never Done, here and opened fresh.
  const status = page.getByLabel("Run status");
  await expect(status).toHaveText(/^Stopped/, { timeout: afterStart(30_000) });
  await page.reload();
  await expect(status).toHaveText(/^Stopped/, { timeout: afterStart(30_000) });
  const notes = (await messages(page, leadId)).filter(
    (m) => m.type === "ended",
  );
  expect(notes).toHaveLength(1);
  // Their tasks stay, each its own page.
  for (const helper of (await team(page, leadId))?.helpers ?? [])
    expect(
      (
        await page.request.get(`/api/v1/conversations/${helper.conversation}`)
      ).status(),
    ).toBe(200);
});

// E2E-AGENTS-037 (c17 U6, U7 on the real stack; WEB-U-411, 412): EDGE-R's lead starts a helper on its own files, so
// the helper waits for the lead's run, then asks its person a question and waits for the answer. Stop team then ends
// a question that is no longer needed and a helper that never started.
test("E2E-AGENTS-037 Stop team with a question open and a helper not started: each says why", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase =
    "EDGE-R stop a team with a question open and a helper not started";
  const leadId = await edgeLead(page, baseURL, phrase);
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  await expect(page.getByLabel("Run status")).toContainText("Waiting for you", {
    timeout: afterStart(120_000),
  });
  // The helper waits its turn on the lead's files: it hasn't started.
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(60_000),
    })
    .toEqual({ "late-files": "queued" });
  const task = page.getByRole("region", { name: "Task", exact: true });
  await task
    .getByRole("button", { name: `More actions for ${phrase}`, exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Stop team" }).click();
  await page
    .getByRole("dialog", { name: "Stop the team?" })
    .getByRole("button", { name: "Stop team" })
    .click();
  await expect(page.getByText("You stopped the team.")).toBeVisible({
    timeout: afterStart(60_000),
  });
  // WEB-U-424: the gateway's hand-back of late-files's work isn't counted as a report of late-files's. Soft, as below;
  // the card's share line first, so the count is checked on the drawn card.
  const endCard = page.getByRole("region", { name: "You stopped the team." });
  await expect(endCard).toContainText("Share of the work", {
    timeout: afterStart(30_000),
  });
  await expect.soft(endCard).not.toContainText("report from late-files");
  // WEB-U-411: the question says why it's no longer needed. Soft, so a red run reports WEB-U-412's case too.
  await expect
    .soft(
      page.getByRole("region", {
        name: "No longer needed: you stopped the team",
      }),
    )
    .toContainText("Which year should the notes cover?", {
      timeout: afterStart(30_000),
    });
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(60_000),
    })
    .toBe("ended");
  // WEB-U-412: the helper that never started says so, and never invites a first message.
  const late = (await team(page, leadId))?.helpers.find(
    (helper) => helper.name === "late-files",
  );
  if (!late) throw new Error("The team lists no late-files");
  await page.goto(`/c/${encodeURIComponent(late.conversation)}`);
  await expect
    .soft(
      page.getByText(
        "This helper was stopped before it started, when its team stopped. It made no changes.",
      ),
    )
    .toBeVisible({ timeout: afterStart(30_000) });
  await expect.soft(page.getByText("Send a message to begin")).toHaveCount(0);
  // WEB-U-425: its header reads Stopped, never Ready, in English and in Chinese (已停止).
  await expect
    .soft(page.getByLabel("Run status"))
    .toHaveText(/^Stopped/, { timeout: afterStart(30_000) });
  await chooseLanguage(page, "English", "简体中文");
  try {
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");
    await expect
      .soft(page.getByLabel("运行状态"))
      .toHaveText(/^已停止/, { timeout: afterStart(30_000) });
  } finally {
    // The language is the account's: the suite's next tests read English.
    await chooseLanguage(page, "简体中文", "English");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  }
});

test("E2E-AGENTS-023 a gateway restart while helpers work: the team comes back, then stops cleanly", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(4, 420_000));
  await bootstrap(page);
  const phrase = "EDGE-C restart while helpers work";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "holder-one": "working", "holder-two": "working" });
  const markers = process.env.TQA_AGENTS_MARKERS ?? "";
  await rm(join(markers, "restarted"), { force: true });
  await writeFile(join(markers, "restart-wanted"), "while helpers work");
  await marker("restarted", "restart-failed");
  await bootstrap(page);
  // After the restart the team is still running, with both helpers, none failed for the restart.
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(120_000),
    })
    .toBe("running");
  const after = await helperStates(page, leadId);
  expect(Object.keys(after).sort()).toEqual(["holder-one", "holder-two"]);
  expect(Object.values(after)).not.toContain("failed");
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  await expect(
    page
      .getByRole("region", { name: "Helpers" })
      .getByRole("link", { name: /^holder-one:/ }),
  ).toBeVisible();
  await stopIfRunning(page, baseURL, leadId);
  const notes = (await messages(page, leadId)).filter(
    (m) => m.type === "ended",
  );
  expect(notes, "one end note, restart or not").toHaveLength(1);
});

test("E2E-AGENTS-024 deleting a lead with live helpers: the dialog names them, and their tasks go with it", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-D delete a lead with live helpers";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(120_000),
    })
    .toEqual({ "holder-one": "working", "holder-two": "working" });
  const helpers = ((await team(page, leadId))?.helpers ?? []).map(
    (helper) => helper.conversation,
  );
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const task = page.getByRole("region", { name: "Task", exact: true });
  await task
    .getByRole("button", { name: `More actions for ${phrase}`, exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete task" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText(
    "Its 2 helpers, holder-one and holder-two, are deleted with it.",
  );
  await dialog.getByRole("button", { name: "Delete task" }).click();
  for (const id of [leadId, ...helpers])
    await expect
      .poll(
        async () =>
          (await page.request.get(`/api/v1/conversations/${id}`)).status(),
        { timeout: afterStart(60_000) },
      )
      .toBe(404);
  await expect(leadRow(page, leadId)).toHaveCount(0);
});

test("E2E-AGENTS-025 helpers per lead at 1: a second start is refused, and the strip shows one", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const key = "agent_teams.max_members";
  const listed = await page.request.get("/api/v1/admin/limits");
  expect(listed.ok()).toBe(true);
  const limit = (
    (await listed.json()) as { items: { key: string; version: number }[] }
  ).items.find((item) => item.key === key);
  if (!limit) throw new Error(`The gateway lists no ${key}`);
  const set = await page.request.put(`/api/v1/admin/limits/${key}`, {
    headers: writing(baseURL),
    data: { value: 1, version: limit.version },
  });
  expect(set.ok(), "set helpers per lead to 1").toBe(true);
  const { version } = (await set.json()) as { version: number };
  try {
    const phrase = "EDGE-E start more helpers than allowed";
    const leadId = await edgeLead(page, baseURL, phrase);
    await expect
      .poll(async () => helperStates(page, leadId), {
        timeout: afterStart(120_000),
      })
      .toEqual({ "holder-one": "working" });
    // The second start_member was refused member_limit (the cap), and the lead went on (its text reply came after).
    await expect
      .poll(
        async () =>
          (await events(page, leadId)).some(
            (e) =>
              e.type === "tool.result" && /member_limit/.test(e.content ?? ""),
          ),
        { timeout: afterStart(60_000) },
      )
      .toBe(true);
    await page.goto(`/c/${encodeURIComponent(leadId)}`);
    const strip = page.getByRole("region", { name: "Helpers" });
    await expect(strip.getByRole("link")).toHaveCount(1);
    await stopIfRunning(page, baseURL, leadId);
  } finally {
    // Back to the configuration file's value, whatever happened above: a reset names the version it read, and one
    // that didn't take would leave the next lead at one helper (the third edge run, 7 Oct), so it must succeed.
    const reset = await page.request.delete(
      `/api/v1/admin/limits/${key}?version=${version}`,
      { headers: writing(baseURL) },
    );
    expect(reset.ok(), "reset helpers per lead").toBe(true);
  }
});

// Last, so the edge cases run first when the stack lane is short (the lead, 7 Oct 17:16).
test("E2E-AGENTS-020 an agent team end to end, then again with a gateway restart", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(8, 900_000));
  await bootstrap(page);
  const runs = [
    await run(page, baseURL, false),
    await run(page, baseURL, true),
  ];
  if (process.env.TQA_STACK_RESULT)
    await writeFile(process.env.TQA_STACK_RESULT, JSON.stringify({ runs }));
});
