import { readFile, writeFile } from "node:fs/promises";

import { expect, test, type Locator } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { env } from "./stack-watch";

// WEB-E2E-012 (Gate 7, ADR-0014): on the real stack, the demo driver (tests/e2e/gate7.py) runs two agents with the
// scripted fake model in real sandboxes on one channel; this spec is the person, the channel's lead. It watches the log
// arrive live and in order, votes so the gateway accepts one proposal by majority, vetoes the other, and claims and
// reports on a task beside an agent, all from the page. The driver checks the database, events and logs against what it
// writes. The log (tests/e2e/gate7_evidence.py): 1 text, 2–3 proposals, 4 the Builder's vote, 5 the person's vote,
// 6 decision, 7 the person's veto, 8 decision, 9 task, 10–11 the Planner's claim and report, 12–13 the person's,
// 14 done.
test.skip(
  process.env.TQA_STACK_SUITE !== "channels",
  "Requires the Gate 7 demo driver",
);
test.use({ trace: "retain-on-failure" });

const EMAIL = "channels@example.invalid";
/** An agent step: a model turn, and the sandbox's start the first time. */
/** An agent's task, which starts its environment first (e2e/stack-start.ts). */
const AGENT_MS = afterStart(90_000);
type Script = {
  agents: [string, string];
  proposal: string;
  counter: string;
  veto: string;
  task: string;
  report: string;
  person_report: string;
  done: string;
};

const post = (log: Locator, kind: string, text: string) =>
  log.locator(`article[data-kind="${kind}"]`).filter({ hasText: text });
/** A post's name: its seq and the agent's name, with its owner's after " · " (ADR-0014 point 5). */
const byAgent = (seq: number, name: string) =>
  new RegExp(`^#${seq} ${name}( · |$)`);
const seqOf = async (article: Locator) =>
  Number(
    ((await article.getAttribute("aria-label")) ?? "").match(/^#(\d+)/)?.[1],
  );

test("WEB-E2E-012 two agents and a person decide, veto and share a task on a channel, live", async ({
  page,
}) => {
  test.setTimeout(budget(3, 15 * 60_000));
  const script = JSON.parse(
    await readFile(env("TQA_GATE7_SCRIPT"), "utf8"),
  ) as Script;
  const [planner, builder] = script.agents;
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(EMAIL);
  await page
    .getByLabel("Password", { exact: true })
    .fill(env("TQA_STACK_PASSWORD"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.goto(`/channels/${env("TQA_CHANNEL_ID")}`);
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  const log = page.getByRole("list", { name: "Channel log" });

  // 1–6: both proposals and the Builder's vote on the first arrive live; the person's vote makes the majority (2 of 3).
  const proposal = post(log, "proposal", script.proposal);
  await expect(proposal).toBeVisible({ timeout: AGENT_MS });
  await expect(proposal).toHaveAttribute("aria-label", byAgent(2, planner));
  const counter = post(log, "proposal", script.counter);
  await expect(counter).toBeVisible({ timeout: AGENT_MS });
  await expect(counter).toHaveAttribute("aria-label", byAgent(3, builder));
  const votes = proposal.getByRole("group", {
    name: "Votes on #2",
    exact: true,
  });
  await expect(votes).toContainText("Accept 1 · Reject 0 · Not voted 2", {
    timeout: AGENT_MS,
  });
  // WEB-U-165: the line names the proposal by its title, a link to its post (#2).
  await expect(log).toContainText(
    "voted Accept on Approach A: a streaming parser",
  );
  await expect(
    log.getByRole("link", { name: "Approach A: a streaming parser" }).first(),
  ).toHaveAttribute("href", "#post-2");
  await votes.getByRole("button", { name: "Accept #2" }).click();
  await expect(votes).toContainText("Accepted by majority");
  const decision = log.getByRole("article", { name: /^#\d+ Decision$/ });
  await expect(decision.first()).toContainText("Accepted by majority");
  const decisionSeq = await seqOf(decision.first());

  // 7–8: the Builder's proposal, vetoed by the person after confirming.
  expect(script.veto).toBe(script.counter);
  await counter.getByRole("button", { name: "Veto #3" }).click();
  await page
    .getByRole("dialog", { name: "Veto proposal #3?" })
    .getByRole("button", { name: "Veto" })
    .click();
  await expect(
    counter.getByRole("group", { name: "Votes on #3", exact: true }),
  ).toContainText("Vetoed");
  const veto = post(
    log,
    "vote",
    "voted Veto on Approach B: one regular expression",
  );
  await expect(veto).toBeVisible();
  const vetoSeq = await seqOf(veto);
  await expect(decision).toHaveCount(2);

  // 9–13: the Planner posts the task, claims and reports on it; then the person claims the second place from the board
  // and reports.
  const taskPost = post(log, "task", script.task);
  await expect(taskPost).toBeVisible({ timeout: AGENT_MS });
  const taskSeq = await seqOf(taskPost);
  await expect(taskPost).toHaveAttribute(
    "aria-label",
    byAgent(taskSeq, planner),
  );
  const board = page.getByRole("list", { name: "To-dos" });
  const task = board.getByRole("listitem", { name: script.task });
  await expect(task).toContainText("1 of 2 places taken", {
    timeout: AGENT_MS,
  });
  await expect(task).toContainText(`${planner}`);
  await expect(post(log, "report", script.report)).toBeVisible({
    timeout: AGENT_MS,
  });
  await expect(task).toContainText("Completed");
  await task.getByRole("button", { name: `Take on #${taskSeq}` }).click();
  await expect(task).toContainText("2 of 2 places taken");
  await task
    .getByRole("button", { name: `Report back on #${taskSeq}` })
    .click();
  const report = page.getByRole("dialog", {
    name: `Report back on #${taskSeq}`,
  });
  await report.getByLabel("Summary").fill(script.person_report);
  await report.getByRole("button", { name: "Send report" }).click();
  await expect(task.getByText("Completed")).toHaveCount(2);
  await expect(post(log, "report", script.person_report)).toBeVisible();

  // 14: the Planner finishes; every post arrived live, in seq order.
  await expect(post(log, "done", script.done)).toBeVisible({
    timeout: AGENT_MS,
  });
  const order = await log
    .getByRole("article")
    .evaluateAll((items) =>
      items.map((item) =>
        Number((item.getAttribute("aria-label") ?? "").match(/^#(\d+)/)?.[1]),
      ),
    );
  expect(order).toEqual(Array.from({ length: 14 }, (_, index) => index + 1));
  await writeFile(
    env("TQA_STACK_RESULT"),
    JSON.stringify({
      proposal_seq: 2,
      decision_seq: decisionSeq,
      veto_seq: vetoSeq,
      task_seq: taskSeq,
      order,
    }),
  );

  // The feature matrix (G3, C6): each agent's run reads in words beside its dot; none failed or unknown.
  await page.getByRole("tab", { name: "Members" }).click();
  const members = page.getByRole("list", { name: "Members" });
  // An agent's row carries its run's dot; a person's doesn't.
  const agents = members
    .getByRole("listitem")
    .filter({ has: page.locator(".member-dot") });
  await expect(agents).toHaveCount(2);
  for (const agent of await agents.all())
    await expect(agent).toContainText(/Idle|Working|Done/);
  await expect(members).not.toContainText(/Failed|Status unknown/);
});
