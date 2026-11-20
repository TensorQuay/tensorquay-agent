// Channels on the fake front (ADR-0014), as the gateway's human API serves them: the channels a person can see,
// newest first, and create. One seeded channel, open to everyone signed in; a person's own ones are theirs.
import { randomUUID } from "node:crypto";

import { user } from "./fixtures.mjs";
import { seededMap } from "./state.mjs";

const NAME_CONTROL = /[\u0000-\u001f\u007f]/;
const channels = seededMap([
  [
    "0195d8c0-0000-7000-8000-00000000c4a1",
    {
      id: "0195d8c0-0000-7000-8000-00000000c4a1",
      name: "release",
      purpose: "Plan the 1.0 release",
      rule: "majority",
      state: "open",
      owner: { id: "0195d8c0-0000-7000-8000-00000000a0a0", name: "Ana" },
      created_at: "2026-09-27T09:00:00Z",
      ended_at: null,
      role: "member",
      member_count: 4,
      last_seq: 9,
      viewers: null,
    },
  ],
  // The channel page of 18.2 as the mock of 17 Nov 2026 drew it (WEB-E2E-145, the gallery WEB-E2E-146): two people,
  // three agents (two at work), a PDF, a long report, an open proposal with its thread, two to-dos, a budget.
  [
    "0195d8c0-0000-7000-8000-00000000c4a2",
    {
      id: "0195d8c0-0000-7000-8000-00000000c4a2",
      name: "Q4 pricing review",
      purpose: "Settle the Q4 prices",
      rule: "majority",
      state: "open",
      owner: { id: "0195d8c0-0000-7000-8000-00000000a0a0", name: "Ana" },
      created_at: "2026-09-27T09:00:00Z",
      ended_at: null,
      role: "member",
      member_count: 6,
      last_seq: 12,
      wake_budget: 300000,
      spent_tokens: 114000,
      viewers: null,
    },
  ],
  // A channel whose budget for today is used up (C-3, C-14; WEB-E2E-148): the header, the dialog and the notice say
  // when agents are woken again, in the viewer's own time.
  [
    "0195d8c0-0000-7000-8000-00000000c4a3",
    {
      id: "0195d8c0-0000-7000-8000-00000000c4a3",
      name: "Used up today",
      purpose: "",
      rule: "human",
      state: "open",
      owner: { id: "0195d8c0-0000-7000-8000-00000000a0a0", name: "Ana" },
      created_at: "2026-11-21T01:00:00Z",
      ended_at: null,
      role: "member",
      member_count: 1,
      last_seq: 1,
      wake_budget: 2000000,
      spent_tokens: 2000000,
      budget_resets_at: "2026-11-22T03:00:00Z",
      viewers: null,
    },
  ],
]);
const replies = seededMap();
/** Each channel's open streams (ADR-0014 point 11): a new message goes to every one. */
const listeners = new Map();
const frame = (message) =>
  `id: ${message.seq}\ndata: ${JSON.stringify({ type: "channel.message", ...message })}\n\n`;
function publish(id, message) {
  for (const response of listeners.get(id) ?? [])
    response.write(frame(message));
}
const SEEDED = "0195d8c0-0000-7000-8000-00000000c4a1";
const CODER = {
  kind: "conversation",
  id: "0195d8c0-0000-7000-8000-00000000c0de",
  name: "Coder",
  owner_name: "ana",
  role: "lead",
  run_status: "running",
  // The gateway's per-caller answer: the fake's viewer owns Coder, not Reviewer.
  can_open: true,
  added_at: "2026-09-27T09:00:00Z",
};
const REVIEWER = {
  kind: "conversation",
  id: "0195d8c0-0000-7000-8000-00000000c0df",
  name: "Reviewer",
  owner_name: "ben",
  role: "member",
  run_status: "completed",
  can_open: false,
  added_at: "2026-09-27T09:00:00Z",
};
const PRICING = "0195d8c0-0000-7000-8000-00000000c4a2";
const agent = (id, title, state) => ({
  kind: "conversation",
  id: `0195d8c0-0000-7000-8000-0000000${id}`,
  // Its name in the channel (A12): named by its task's title when Ana added it.
  name: title,
  title,
  owner_name: "Ana",
  role: "member",
  run_status: state === "idle" ? "completed" : "running",
  state,
  can_open: true,
  added_at: "2026-09-27T09:00:00Z",
});
const person = (id, name, role) => ({
  kind: "user",
  id: `0195d8c0-0000-7000-8000-0000000${id}`,
  name,
  role,
  added_at: "2026-09-27T09:00:00Z",
});
const ANA = person("0a0a0", "Ana", "lead");
const DANIEL = person("0d0d1", "Daniel Okafor", "member");
const ANALYST = agent("0c0e1", "Pricing analyst", "working");
const CHECKER = agent("0c0e2", "Data checker", "waiting");
const FORECAST = agent("0c0e3", "Forecast", "idle");
/** The person signed in is a voting member here, listed like the others: the header's count and the tally agree. */
const VIEWER = {
  kind: "user",
  id: user.id,
  name: user.email.split("@")[0],
  role: "member",
  added_at: "2026-09-28T09:00:00Z",
};
const members = seededMap([
  [SEEDED, [CODER, REVIEWER]],
  [PRICING, [ANA, DANIEL, VIEWER, ANALYST, CHECKER, FORECAST]],
]);
const at = (minute) => `2026-09-27T10:${String(minute).padStart(2, "0")}:00Z`;
const by = (member) => ({
  kind: member.kind,
  id: member.id,
  name: member.name,
});
const logs = seededMap([
  [
    SEEDED,
    [
      {
        seq: 1,
        ts: at(1),
        author: by(CODER),
        body: { type: "text", text: "Starting on the **release** notes." },
      },
      {
        seq: 2,
        ts: at(2),
        author: by(CODER),
        body: {
          type: "proposal",
          title: "Ship on Friday",
          text: "All checks are green.",
          window_seconds: 600,
        },
      },
      {
        seq: 3,
        ts: at(3),
        author: by(REVIEWER),
        body: {
          type: "vote",
          proposal_seq: 2,
          choice: "accept",
          reason: "Tests pass",
        },
      },
      {
        seq: 4,
        ts: at(4),
        author: { kind: "gateway", id: "gateway", name: "gateway" },
        body: {
          type: "decision",
          proposal_seq: 2,
          outcome: "accepted",
          rule: "majority",
        },
      },
      {
        seq: 5,
        ts: at(5),
        author: by(CODER),
        body: {
          type: "task",
          title: "Write the changelog",
          spec: "From the merged pull requests.",
          claims: 2,
        },
      },
      {
        seq: 6,
        ts: at(6),
        author: by(REVIEWER),
        body: { type: "claim", task_seq: 5 },
      },
      {
        seq: 7,
        ts: at(7),
        author: by(REVIEWER),
        body: {
          type: "report",
          task_seq: 5,
          status: "completed",
          summary: "Changelog written.",
          refs: ["https://git.example.invalid/pr/12"],
        },
      },
      {
        seq: 8,
        ts: at(8),
        author: by(CODER),
        parent: 7,
        body: { type: "text", text: "Looks right." },
      },
      {
        seq: 9,
        ts: at(9),
        author: by(CODER),
        body: { type: "done", summary: "Release prepared." },
      },
    ],
  ],
  [
    PRICING,
    [
      {
        seq: 1,
        ts: at(12),
        author: by(ANA),
        body: {
          type: "text",
          text: "The Q3 numbers are in. Can we settle the EU price for the Pro plan this week?",
        },
        files: [{ name: "q3-revenue-by-region.pdf", size: 1258291 }],
      },
      {
        seq: 2,
        ts: at(12),
        author: by(ANA),
        body: {
          type: "text",
          text: "@Pricing analyst please start with churn by tier.",
        },
      },
      {
        seq: 3,
        ts: at(21),
        author: by(ANALYST),
        body: {
          type: "text",
          text: [
            "**Churn by tier, Q3** (from the PDF, pages 3 to 9)",
            "Pro churn in the EU was 1.8 % a month, under the 2.5 % line we set in June.",
            "Starter churn rose to 4.1 %, mostly in the first month after a trial.",
            "- Pro, EU: 1.8 % (Q2: 1.9 %)",
            "- Pro, US: 2.0 % (Q2: 2.2 %)",
            "- Starter, EU: 4.1 % (Q2: 3.4 %)",
            "- Starter, US: 3.6 % (Q2: 3.3 %)",
            "- Team: 0.9 %, flat",
            "- Enterprise: 0.4 %, flat",
            "A rise to €24 is inside what Pro customers absorbed in the 2025 change.",
            "Churn then moved 0.3 points for two months and came back.",
            "The trial change would matter more for Starter than any price.",
            "Method: monthly cohorts, active seats, refunds counted as churn.",
            "Data: the PDF's tables 2 to 5; region from billing country.",
          ].join("\n\n"),
        },
      },
      {
        seq: 4,
        ts: at(40),
        author: by(DANIEL),
        body: {
          type: "proposal",
          title: "Raise the EU Pro plan to €24 from 1 December",
          text: "Per the churn report above. Starter stays where it is until the trial change ships.",
          window_seconds: 86400,
        },
      },
      {
        seq: 5,
        ts: at(41),
        author: by(ANALYST),
        parent: 4,
        body: {
          type: "text",
          text: "Agreed: churn stays under the line at €24.",
        },
      },
      {
        seq: 6,
        ts: at(41),
        author: by(ANALYST),
        body: { type: "vote", proposal_seq: 4, choice: "accept" },
      },
      {
        seq: 7,
        ts: at(42),
        author: by(CHECKER),
        parent: 4,
        body: { type: "text", text: "I'll recheck VAT before I vote." },
      },
      {
        seq: 8,
        ts: at(43),
        author: by(ANA),
        body: {
          type: "task",
          title: "Recheck the EU VAT figures in the PDF",
          spec: "Pages 11 and 12: the German and French rows look swapped.",
        },
      },
      {
        seq: 9,
        ts: at(44),
        author: by(CHECKER),
        body: { type: "claim", task_seq: 8 },
      },
      {
        seq: 10,
        ts: at(45),
        author: by(ANA),
        body: {
          type: "task",
          title: "Draft the customer email for the new price",
          spec: "Short, with the date and what Pro customers get.",
        },
      },
      {
        seq: 11,
        ts: at(50),
        author: by(FORECAST),
        body: {
          type: "text",
          text: "At €24 with churn held at 1.8 %, EU Pro revenue rises about 9 % by March.",
        },
      },
      {
        seq: 12,
        ts: at(51),
        author: { kind: "gateway", id: "gateway", name: "gateway" },
        body: {
          type: "notice",
          kind: "wake_budget_80",
          text: "Woken agents have used 80 % of this channel's budget for today.",
        },
      },
    ],
  ],
  [
    "0195d8c0-0000-7000-8000-00000000c4a3",
    [
      {
        seq: 1,
        ts: "2026-11-21T03:00:00Z",
        author: { kind: "gateway", id: "gateway", name: "gateway" },
        body: {
          type: "notice",
          kind: "wake_budget_spent",
          resets_at: "2026-11-22T03:00:00Z",
          text: "Woken agents have used this channel's budget for today: no agent is woken here until it renews at 03:00 UTC on 22 November 2026, or until the channel's owner, a lead or an administrator raises it.",
        },
      },
    ],
  ],
]);

/** A channel as the API answers it: without the fake's own visibility list, with the caller's role. */
function shown(item, member) {
  const answer = { ...item };
  delete answer.viewers;
  return {
    ...answer,
    role: item.owner.id === member.account.id ? "lead" : item.role,
  };
}
const visible = (item, member) =>
  item.viewers === null || item.viewers.includes(member.account.id);

const refused = (reply, status, code) =>
  reply(status, { error: { code, message: code, retryable: false } });
const voting = (id) =>
  (members.get(id) ?? []).filter((one) => one.role !== "observer").length;

/** The task board from the log (ADR-0014 point 8): each task's resolved limit, its claims and its reports. */
function board(id) {
  const log = logs.get(id) ?? [];
  return log
    .filter((task) => task.body.type === "task")
    .map((task) => ({
      task_seq: task.seq,
      title: task.body.title,
      limit: task.body.claims === "all" ? voting(id) : (task.body.claims ?? 1),
      claims: log
        .filter(
          (one) => one.body.type === "claim" && one.body.task_seq === task.seq,
        )
        .map((one) => ({ author: one.author, seq: one.seq })),
      reports: log
        .filter(
          (one) => one.body.type === "report" && one.body.task_seq === task.seq,
        )
        .map((one) => ({
          author: one.author,
          seq: one.seq,
          status: one.body.status,
        })),
    }));
}

/** A proposal's state from the log (ADR-0014 point 9): each voting member's latest vote, and its decision if any. */
function proposal(member, id, seq, reply) {
  const item = channels.get(id);
  if (!item || !visible(item, member)) return refused(reply, 404, "not_found");
  const log = logs.get(id) ?? [];
  const posted = log.find((one) => one.seq === seq);
  if (posted?.body.type !== "proposal")
    return refused(reply, 409, "not_a_proposal");
  const latest = new Map();
  for (const one of log)
    if (one.body.type === "vote" && one.body.proposal_seq === seq)
      latest.set(`${one.author.kind}:${one.author.id}`, {
        author: one.author,
        choice: one.body.choice,
        seq: one.seq,
      });
  const votes = [...latest.values()];
  const decision = log.find(
    (one) => one.body.type === "decision" && one.body.proposal_seq === seq,
  );
  reply(200, {
    proposal_seq: seq,
    rule: item.rule,
    state: decision ? "decided" : "open",
    window_ends_at: new Date(
      Date.parse(posted.ts) + (posted.body.window_seconds ?? 600) * 1000,
    ).toISOString(),
    // The voting members, the person viewing among them (a seed that lists them counts them once).
    voting_members:
      voting(id) +
      ((members.get(id) ?? []).some(
        (one) => one.kind === "user" && one.id === member.account.id,
      )
        ? 0
        : 1),
    tally: {
      accept: votes.filter((vote) => vote.choice === "accept").length,
      reject: votes.filter((vote) => vote.choice === "reject").length,
    },
    votes,
    decision: decision
      ? { seq: decision.seq, outcome: decision.body.outcome }
      : null,
  });
}

/** A person's claim (ADR-0014 point 8): a repeat answers the held claim; a full task is already_claimed. */
function claim(member, id, seq, reply) {
  const item = channels.get(id);
  if (!item || !visible(item, member)) return refused(reply, 404, "not_found");
  const task = board(id).find((one) => one.task_seq === seq);
  if (!task) return refused(reply, 409, "not_a_task");
  const log = logs.get(id) ?? [];
  const held = log.find(
    (one) =>
      one.body.type === "claim" &&
      one.body.task_seq === seq &&
      one.author.kind === "user" &&
      one.author.id === member.account.id,
  );
  if (held) return reply(200, held);
  if (task.claims.length >= task.limit)
    return refused(reply, 409, "already_claimed");
  const stored = {
    seq: (log.at(-1)?.seq ?? 0) + 1,
    ts: new Date().toISOString(),
    author: {
      kind: "user",
      id: member.account.id,
      name: member.account.email.split("@")[0],
    },
    body: { type: "claim", task_seq: seq },
  };
  logs.set(id, [...log, stored]);
  publish(id, stored);
  reply(201, stored);
}

/** Answers /api/v1/channels/{id}, its members and its messages; returns false for any other path. */
async function oneChannel(member, url, method, read, key, reply, raw) {
  const events = /^\/api\/v1\/channels\/([^/]+)\/events$/.exec(url.pathname);
  if (events && method === "GET") {
    const id = decodeURIComponent(events[1]);
    const item = channels.get(id);
    if (!item || !visible(item, member)) {
      refused(reply, 404, "not_found");
      return true;
    }
    // Server-sent events (ADR-0014 point 11): every message past Last-Event-ID (or `after`), then live.
    const { request, response } = raw;
    response.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      "x-accel-buffering": "no",
    });
    const after = Number(
      request.headers["last-event-id"] ?? url.searchParams.get("after") ?? 0,
    );
    for (const message of logs.get(id) ?? [])
      if (message.seq > after) response.write(frame(message));
    if (item.state === "ended")
      response.write(
        `data: ${JSON.stringify({ type: "channel.state", state: "ended" })}\n\n`,
      );
    const open = listeners.get(id) ?? new Set();
    open.add(response);
    listeners.set(id, open);
    request.on("close", () => open.delete(response));
    return true;
  }
  // A post's file (A9), as the gateway serves it: an attachment under its own name, the bytes a stand-in.
  const filed =
    /^\/api\/v1\/channels\/([^/]+)\/messages\/(\d+)\/files\/(\d+)$/.exec(
      url.pathname,
    );
  if (filed && method === "GET") {
    const id = decodeURIComponent(filed[1]);
    const item = channels.get(id);
    const file = (logs.get(id) ?? []).find(
      (one) => one.seq === Number(filed[2]),
    )?.files?.[Number(filed[3])];
    if (!item || !visible(item, member) || !file) {
      refused(reply, 404, "not_found");
      return true;
    }
    raw.response.writeHead(200, {
      "content-type": "application/octet-stream",
      "content-disposition": `attachment; filename="${file.name}"`,
    });
    raw.response.end(`the file ${file.name}`);
    return true;
  }
  const proposed = /^\/api\/v1\/channels\/([^/]+)\/proposals\/(\d+)$/.exec(
    url.pathname,
  );
  if (proposed && method === "GET") {
    proposal(
      member,
      decodeURIComponent(proposed[1]),
      Number(proposed[2]),
      reply,
    );
    return true;
  }
  const claimed = /^\/api\/v1\/channels\/([^/]+)\/tasks\/(\d+)\/claim$/.exec(
    url.pathname,
  );
  if (claimed && method === "POST") {
    claim(member, decodeURIComponent(claimed[1]), Number(claimed[2]), reply);
    return true;
  }
  // WEB-U-066: membership, end and delete, with the owner's authority (an administrator may also end or delete).
  const managed =
    /^\/api\/v1\/channels\/([^/]+)\/members\/(user|conversation)\/([^/]+)$/.exec(
      url.pathname,
    );
  const ending = /^\/api\/v1\/channels\/([^/]+)\/end$/.exec(url.pathname);
  const target = managed ?? ending;
  if (
    target &&
    (method === "PUT" || method === "DELETE" || method === "POST")
  ) {
    const item = channels.get(decodeURIComponent(target[1]));
    if (!item || !visible(item, member)) {
      refused(reply, 404, "not_found");
      return true;
    }
    const owns = item.owner.id === member.account.id;
    if (ending && method === "POST") {
      if (!owns && member.account.role !== "admin") {
        refused(reply, 403, "not_owner");
        return true;
      }
      item.state = "ended";
      item.ended_at = new Date().toISOString();
      for (const response of listeners.get(item.id) ?? [])
        response.write(
          `data: ${JSON.stringify({ type: "channel.state", state: "ended" })}\n\n`,
        );
      reply(200, shown(item, member));
      return true;
    }
    if (!managed) return false;
    const [kind, id] = [managed[2], decodeURIComponent(managed[3])];
    const leaving =
      method === "DELETE" && kind === "user" && id === member.account.id;
    if (!owns && !leaving) {
      refused(reply, 403, "not_owner");
      return true;
    }
    const list = members.get(item.id) ?? [];
    const rest = list.filter((one) => !(one.kind === kind && one.id === id));
    if (method === "DELETE") {
      members.set(item.id, rest);
      item.member_count = Math.max(1, rest.length);
      reply(204);
      return true;
    }
    const input = await read();
    const found = list.find((one) => one.kind === kind && one.id === id);
    if (!found) {
      refused(reply, 404, "not_found");
      return true;
    }
    // A name in the channel (A12) comes with the member when its owner gives one.
    const changed = {
      ...found,
      role: input.role,
      ...(typeof input.name === "string" ? { name: input.name } : {}),
    };
    members.set(item.id, [...rest, changed]);
    reply(200, changed);
    return true;
  }
  const match =
    /^\/api\/v1\/channels\/([^/]+)(\/members|\/messages|\/tasks)?$/.exec(
      url.pathname,
    );
  if (!match) return false;
  const item = channels.get(decodeURIComponent(match[1]));
  if (!item || !visible(item, member)) {
    refused(reply, 404, "not_found");
    return true;
  }
  const part = match[2];
  if (!part && method === "GET") reply(200, shown(item, member));
  else if (!part && method === "PATCH") {
    // The wake budget (C-1), as the gateway sets it: its owner, a person who leads it, or an administrator; the
    // channel says who set it.
    const shownItem = shown(item, member);
    if (
      shownItem.role !== "lead" &&
      item.owner.id !== member.account.id &&
      member.account.role !== "admin"
    ) {
      refused(reply, 403, "not_lead");
      return true;
    }
    const input = await read();
    item.wake_budget = input.wake_budget;
    item.spent_tokens = item.spent_tokens ?? 0;
    const log = logs.get(item.id) ?? [];
    const stored = {
      seq: (log.at(-1)?.seq ?? 0) + 1,
      ts: new Date().toISOString(),
      author: { kind: "gateway", id: "gateway", name: "gateway" },
      body: {
        type: "notice",
        kind: "wake_budget_set",
        member: member.account.email,
        text: `${member.account.email} set this channel's budget.`,
      },
    };
    logs.set(item.id, [...log, stored]);
    item.last_seq = stored.seq;
    publish(item.id, stored);
    reply(200, shown(item, member));
  } else if (!part && method === "DELETE") {
    if (item.owner.id !== member.account.id && member.account.role !== "admin")
      refused(reply, 403, "not_owner");
    else {
      channels.delete(item.id);
      reply(204);
    }
  } else if (part === "/tasks" && method === "GET")
    reply(200, { items: board(item.id) });
  else if (part === "/members" && method === "GET")
    reply(200, { items: members.get(item.id) ?? [] });
  else if (part === "/members" && method === "PUT") {
    // A person by exact email (WEB-U-066), as the gateway answers it: the owner only; their own address is 400.
    const input = await read();
    const email = String(input.email ?? "");
    const person = member.team.accounts.find(
      (one) => one.email === email && !one.disabled,
    );
    if (item.owner.id !== member.account.id) refused(reply, 403, "not_owner");
    else if (email === member.account.email)
      refused(reply, 400, "invalid_request");
    else if (!person) refused(reply, 404, "user_not_found");
    else {
      const added = {
        kind: "user",
        id: person.id,
        name: email.split("@")[0],
        role: input.role,
        added_at: new Date().toISOString(),
      };
      const list = (members.get(item.id) ?? []).filter(
        (one) => !(one.kind === "user" && one.id === person.id),
      );
      members.set(item.id, [...list, added]);
      item.member_count = list.length + 1;
      reply(200, added);
    }
  } else if (part === "/messages" && method === "GET") {
    const since = Number(url.searchParams.get("since") ?? 0);
    const limit = Number(url.searchParams.get("limit") ?? 50);
    const found = (logs.get(item.id) ?? []).filter((one) => one.seq > since);
    const page = found.slice(0, limit);
    reply(200, {
      messages: page,
      next_since: page.at(-1)?.seq ?? since,
      more: found.length > page.length,
    });
  } else if (part === "/messages" && method === "POST") {
    if (key && replies.has(key)) {
      reply(201, replies.get(key));
      return true;
    }
    const input = await read();
    const log = logs.get(item.id) ?? [];
    const stored = {
      seq: (log.at(-1)?.seq ?? 0) + 1,
      ts: new Date().toISOString(),
      author: {
        kind: "user",
        id: member.account.id,
        name: member.account.email.split("@")[0],
      },
      ...(input.parent ? { parent: input.parent } : {}),
      body: input.body,
    };
    logs.set(item.id, [...log, stored]);
    item.last_seq = stored.seq;
    publish(item.id, stored);
    if (key) replies.set(key, stored);
    reply(201, stored);
  } else return false;
  return true;
}

/** Answers /api/v1/channels and one channel's routes; returns false for any other path. */
export async function channelRoute(member, url, method, read, key, reply, raw) {
  if (await oneChannel(member, url, method, read, key, reply, raw)) return true;
  if (url.pathname !== "/api/v1/channels") return false;
  if (method === "GET") {
    reply(200, {
      items: [...channels.values()]
        .filter((item) => visible(item, member))
        .map((item) => shown(item, member))
        .reverse(),
      next_cursor: null,
    });
    return true;
  }
  if (method !== "POST") return false;
  if (key && replies.has(key)) {
    reply(201, replies.get(key));
    return true;
  }
  const input = await read();
  const details = [];
  const name = typeof input.name === "string" ? input.name : "";
  if (!name.trim() || name.length > 80 || NAME_CONTROL.test(name))
    details.push({ field: "name", reason: "name_invalid" });
  if (typeof input.purpose === "string" && input.purpose.length > 2000)
    details.push({ field: "purpose", reason: "value_too_long" });
  if (!["human", "majority"].includes(input.rule))
    details.push({ field: "rule", reason: "rule_invalid" });
  if (details.length) {
    reply(400, {
      error: {
        code: "invalid_request",
        message: "The channel is not valid.",
        retryable: false,
        details,
      },
    });
    return true;
  }
  const item = {
    id: randomUUID(),
    name,
    purpose: input.purpose ?? "",
    rule: input.rule,
    state: "open",
    owner: { id: member.account.id, name: member.account.email.split("@")[0] },
    created_at: new Date().toISOString(),
    ended_at: null,
    role: "lead",
    member_count: 1,
    last_seq: 0,
    viewers: [member.account.id],
  };
  channels.set(item.id, item);
  // Its creator is its lead (ADR-0014 point 4), as the gateway lists them.
  members.set(item.id, [
    {
      kind: "user",
      id: member.account.id,
      name: item.owner.name,
      role: "lead",
      added_at: item.created_at,
    },
  ]);
  const answer = shown(item, member);
  if (key) replies.set(key, answer);
  reply(201, answer);
  return true;
}
