// Agent teams on the fake front (ADR-0034 "Slice S6 contract"): off unless a test resets with the "agent-teams"
// scenario, and then every session has a lead task with two helpers and a short message log. The routes answer as the
// contract says (the drift check holds them to openapi.json, and team.changed to the protocol schema): the team, its
// messages, the person's answer and the stop; scope=top; deleting a lead with its helpers; the administrator's two
// settings. Off, every team route is 403 teams_disabled, no task carries a team, and the session says so.
import { randomUUID } from "node:crypto";

import { onReset, seededMap } from "./state.mjs";
import { notify } from "./stream.mjs";

let on = false;
onReset((scenarios) => {
  on = scenarios.includes("agent-teams");
});

/** Whether agent teams are on for this install (the session's features.agent_teams). */
export const teamsOn = () => on;

/** Each session's team, by its tasks map; each team task's place in it, by the task. */
const teams = new WeakMap();
const places = new WeakMap();
/** Sessions already seeded: a deleted team stays deleted. */
const seeded = new WeakSet();
const MAX_ANSWER_BYTES = 16_000;
/** A time `minutes` from the seed's start, which is half an hour ago, so the team has time left whenever it is seeded. */
let seededAt = Date.now() - 31 * 60_000;
const at = (minutes) =>
  new Date(seededAt + minutes * 60_000).toISOString().replace(/\.\d{3}Z$/, "Z");

/**
 * Seeds this session's team once, while teams are on: "Report on Q3" leads a researcher (working, one report sent) and
 * a writer (queued second). `make` builds a task as the fake front stores one.
 */
export function seedTeam(conversations, make) {
  if (!on || seeded.has(conversations)) return;
  seeded.add(conversations);
  seededAt = Date.now() - 31 * 60_000;
  const leadTask = make({ title: "Report on Q3", title_source: "person" });
  const team = {
    id: "0195d8c0-0000-7000-8000-00000000a0ff",
    state: "running",
    stop_reason: null,
    lead: leadTask,
    started_at: at(0),
    deadline_at: at(120),
    budget: { tokens: 2_000_000, spent: 412_000 },
    helpers: [],
    messages: [],
    answered: new Set(),
    replies: new Map(),
    version: 1,
  };
  teams.set(conversations, team);
  places.set(leadTask, { team, role: "lead" });
  conversations.set(leadTask.id, leadTask);
  for (const [name, fields] of [
    [
      "researcher",
      {
        state: "working",
        position: null,
        tokens: 120_400,
        activity: { step: 4, tool: "shell", since: at(31) },
      },
    ],
    ["writer", { state: "queued", position: 2, tokens: 0, activity: null }],
  ]) {
    // Titled with its name only, as the team's start retitles a helper's task.
    const task = make({ title: name });
    const helper = {
      conversation: task,
      name,
      role: name,
      workspace: "own_copy",
      last_message: null,
      tokens_estimated: false,
      ...fields,
    };
    team.helpers.push(helper);
    // Its first turn is its brief from the lead (WEB-U-223): replayed as any task's history, checked against the protocol.
    task.history.push({
      type: "event",
      seq: 1,
      run_id: null,
      step: null,
      ts: at(1),
      event: {
        type: "message.user",
        text: BRIEFS[name].objective,
        attachments: [],
        lead_brief: {
          role: name,
          workspace: "own_copy",
          brief: BRIEFS[name],
        },
      },
    });
    places.set(task, { team, role: "member", helper });
    conversations.set(task.id, task);
  }
  post(
    team,
    "assign",
    { kind: "lead" },
    { kind: "member", name: "researcher" },
    "Collect the Q3 figures for each region.",
  );
  post(
    team,
    "assign",
    { kind: "lead" },
    { kind: "member", name: "writer" },
    "Draft the summary once the figures are in.",
  );
  post(
    team,
    "report",
    { kind: "member", name: "researcher" },
    { kind: "lead" },
    "North: 4.1 M, South: 3.6 M, East: 2.9 M.",
  );
  team.version = 1;
}

/**
 * Fixture only: the task `leadTask`, in no team, starts one now, as a lead's first start_member does (WEB-E2E-024's live
 * start, WEB-U-326): one helper, the researcher, at work, and its assignment in the log; then the lead's open stream
 * hears team.changed (never stored), the way the gateway announces a new team: at once, or `announceAfter` ms later, as
 * the gateway's once-a-second flush can (WEB-U-416). False if teams are off or the task is already in one.
 */
export function startTeam(leadTask, conversations, make, announceAfter = 0) {
  if (!on || places.has(leadTask)) return false;
  const team = {
    id: "0195d8c0-0000-7000-8000-00000000b0ff",
    state: "running",
    stop_reason: null,
    lead: leadTask,
    started_at: at(31),
    deadline_at: at(151),
    budget: { tokens: 2_000_000, spent: 0 },
    helpers: [],
    messages: [],
    answered: new Set(),
    replies: new Map(),
    version: 0,
  };
  places.set(leadTask, { team, role: "lead" });
  const task = make({ title: "researcher" });
  const helper = {
    conversation: task,
    name: "researcher",
    role: "researcher",
    workspace: "own_copy",
    last_message: null,
    state: "working",
    position: null,
    tokens: 0,
    tokens_estimated: false,
    activity: { step: 1, tool: "shell", since: at(31) },
  };
  team.helpers.push(helper);
  places.set(task, { team, role: "member", helper });
  conversations.set(task.id, task);
  post(
    team,
    "assign",
    { kind: "lead" },
    { kind: "member", name: "researcher" },
    "Collect the Q3 figures for each region.",
  );
  if (announceAfter > 0) setTimeout(() => changed(team), announceAfter);
  else changed(team);
  return true;
}

/** The seeded helpers' briefs from the lead. */
const BRIEFS = {
  researcher: {
    objective: "Collect the Q3 revenue for each region from the sales exports.",
    output: "A table of region, revenue and change from Q2, as q3-figures.csv.",
    sources: "The exports in data/sales/.",
    boundaries: "Read only; don't change the exports.",
  },
  writer: {
    objective: "Draft a one-page summary of the Q3 figures once they are in.",
    output: "summary.md, under 400 words.",
    sources: null,
    boundaries: "Use only the researcher's figures.",
  },
};

/** Appends a message in seq order; a helper's message to the lead becomes its last message. */
function post(team, type, from, to, text, extra = {}) {
  const message = {
    seq: team.messages.length + 1,
    type,
    from,
    to,
    at: new Date().toISOString(),
    text,
    ...extra,
  };
  team.messages.push(message);
  const helper = team.helpers.find(
    (each) => from.kind === "member" && each.name === from.name,
  );
  if (helper && to.kind === "lead")
    helper.last_message = {
      seq: message.seq,
      type,
      text: text.slice(0, 300),
      at: message.at,
    };
  return message;
}

const live = (team) => team.helpers.filter((each) => !each.deleted);
// A stopping or ended team waits for no one (ADR-0034 S6).
const question = (team) =>
  team.state !== "running"
    ? null
    : (team.messages
        .filter(
          (each) =>
            each.type === "person_question" && !team.answered.has(each.seq),
        )
        .at(-1) ?? null);

/** The team moved: its version rises and every team task's open stream hears it once (never stored). */
function changed(team) {
  team.version += 1;
  const frame = {
    type: "team.changed",
    team: team.id,
    version: team.version,
    last_seq: team.messages.length,
  };
  for (const task of [
    team.lead,
    ...live(team).map((each) => each.conversation),
  ])
    notify(task, frame);
}

/**
 * As the gateway's `can_stop` (GW-U-168): Stop helper would stop it while the team runs, unless it is stopped or failed
 * without having run (`failed_run`, which a test sets through the fixture's fields; a helper whose run failed keeps its
 * seat until stopped).
 */
function canStop(team, helper) {
  return (
    team.state === "running" &&
    helper.state !== "stopped" &&
    (helper.state !== "failed" || helper.failed_run === true)
  );
}

/** A task's `team` as its person reads it in the task list and the task itself; null for a task in no team. */
export function teamOf(conversation) {
  const place = on ? places.get(conversation) : undefined;
  if (!place || (place.helper?.deleted ?? false)) return null;
  const { team } = place;
  if (place.role === "lead") {
    const helpers = live(team);
    return {
      role: "lead",
      id: team.id,
      state: team.state,
      helpers: helpers.length,
      attention: {
        stuck: helpers.filter((each) => each.state === "stuck").length,
        failed: helpers.filter((each) => each.state === "failed").length,
      },
      waiting_for_you: question(team) !== null,
    };
  }
  return {
    role: "member",
    id: team.id,
    state: team.state,
    lead: { id: team.lead.id, title: team.lead.title ?? null },
    name: place.helper.name,
    helper_state: place.helper.state,
    can_stop: canStop(team, place.helper),
  };
}

/** scope=top leaves helpers to their lead, except in a search, which finds them (and while teams are off). */
export const listed = (conversation, url) =>
  url.searchParams.get("scope") !== "top" ||
  url.searchParams.has("q") ||
  teamOf(conversation)?.role !== "member";

/**
 * As the gateway's team read (GW-I-319): the fake lead reads nothing, so its place is 0 and every message in the log for
 * it (to it or to the whole team) that it didn't write waits unread.
 */
function leadPlace(team) {
  const unread = team.messages.filter(
    (each) =>
      (each.to.kind === "lead" || each.to.kind === "team") &&
      each.from.kind !== "lead",
  ).length;
  return { id: team.lead.id, read_seq: 0, unread };
}

function view(team) {
  const asked = question(team);
  return {
    id: team.id,
    state: team.state,
    stop_reason: team.stop_reason,
    lead: leadPlace(team),
    started_at: team.started_at,
    deadline_at: team.deadline_at,
    budget: team.budget,
    waiting_for_you: asked
      ? { seq: asked.seq, text: asked.text, at: asked.at }
      : null,
    helpers: live(team).map(({ conversation, failed_run, ...helper }) => ({
      conversation: conversation.id,
      ...helper,
      can_stop: canStop(team, { ...helper, failed_run }),
    })),
    version: team.version,
  };
}

/** The end note (point 10): one stored `ended` message to the lead, with the fields the page words itself. */
function end(team, reason) {
  team.state = "ended";
  team.stop_reason = reason;
  for (const helper of live(team))
    if (!["done", "failed"].includes(helper.state)) helper.state = "stopped";
  // As the gateway at a team's end (GW-I-311): a helper that never reported has its work handed back in a report of
  // the gateway's own, which the end note lists with the helpers' reports.
  for (const helper of live(team))
    if (
      !team.messages.some(
        (each) => each.type === "report" && each.from.name === helper.name,
      )
    )
      post(
        team,
        "report",
        { kind: "gateway" },
        { kind: "lead" },
        `The gateway hands back ${helper.name}'s work: ${helper.name} stopped before it reported (the team ${reason === "stopped" ? "was stopped" : "ended"}).`,
        { member: helper.name },
      );
  const reports = team.messages.filter((each) => each.type === "report");
  post(
    team,
    "ended",
    { kind: "gateway" },
    { kind: "lead" },
    "The team stopped. Reports and files handed back are listed here.",
    {
      reason,
      summary: null,
      tokens: {
        total: team.budget.spent,
        members: live(team).map((each) => ({
          name: each.name,
          tokens: each.tokens,
        })),
      },
      reports: reports.map((each) => ({
        member: each.from.name ?? each.member,
        seq: each.seq,
      })),
      files: [
        {
          member: "researcher",
          seq: reports[0]?.seq ?? 1,
          name: "q3-figures.csv",
          size: 2048,
        },
      ],
    },
  );
  changed(team);
}

const byteLength = (text) => Buffer.byteLength(text, "utf8");

/**
 * Answers a team task's /team routes and the tests' own hooks; returns false for any other suffix. `answer` is the
 * server's { json, error }.
 */
export async function teamRoute(
  conversation,
  suffix,
  method,
  url,
  read,
  idempotency,
  answer,
) {
  if (!suffix.startsWith("/team")) return false;
  if (!on) {
    answer.error(403, "teams_disabled", "Agent teams are turned off.");
    return true;
  }
  const place = places.get(conversation);
  if (!place || (place.helper?.deleted ?? false)) {
    answer.error(404, "no_team", "This task is in no team.");
    return true;
  }
  const { team } = place;
  if (suffix === "/team" && method === "GET") answer.json(200, view(team));
  else if (suffix === "/team/messages" && method === "GET") {
    const after = Number(url.searchParams.get("after") ?? 0);
    const limit = Number(url.searchParams.get("limit") ?? 100);
    if (
      !Number.isSafeInteger(after) ||
      after < 0 ||
      !Number.isInteger(limit) ||
      limit < 1 ||
      limit > 200
    )
      answer.error(
        400,
        "invalid_request",
        "after is a seq; limit is 1 to 200.",
      );
    else {
      const items = team.messages.filter((each) => each.seq > after);
      answer.json(200, {
        items: items.slice(0, limit),
        // The page's last item, or `after` when it has none (ADR-0034 S6).
        last_seq: items.slice(0, limit).at(-1)?.seq ?? after,
        more: items.length > limit,
      });
    }
  } else if (suffix === "/team/answers" && method === "POST") {
    const replay = team.replies.get(idempotency);
    const input = replay ? null : await read();
    const asked = team.messages.find(
      (each) =>
        each.seq === input?.question_seq && each.type === "person_question",
    );
    if (replay) answer.json(201, replay);
    else if (
      typeof input.text !== "string" ||
      !input.text ||
      byteLength(input.text) > MAX_ANSWER_BYTES
    )
      answer.error(400, "invalid_request", "An answer is 1 to 16,000 bytes.");
    else if (team.state !== "running")
      answer.error(409, "team_stopped", "The team has stopped.");
    else if (!asked)
      answer.error(422, "unknown_reference", "No such question.");
    else if (team.answered.has(asked.seq))
      answer.error(409, "already_answered", "This question was answered.");
    else {
      team.answered.add(asked.seq);
      const reply = post(
        team,
        "person_answer",
        { kind: "person" },
        { kind: "lead" },
        input.text,
        {
          question_seq: asked.seq,
          by: conversation.owner_id,
        },
      );
      team.replies.set(idempotency, { seq: reply.seq });
      changed(team);
      answer.json(201, { seq: reply.seq });
    }
  } else if (suffix === "/team/stop" && method === "POST") {
    if (team.state === "ended")
      answer.error(409, "team_stopped", "The team has stopped.");
    else if (place.role === "lead") {
      if (team.state === "running") {
        team.state = "stopping";
        changed(team);
        // The one stop path finishes on its own; the end note follows.
        setTimeout(() => end(team, "stopped"), 200);
      }
      answer.json(202, { team: { state: "stopping" } });
    } else {
      const { helper } = place;
      // As the gateway: a failed helper is gone; a done one is stopped (it could be given more work); a repeat is
      // answered the same.
      if (helper.state === "failed")
        answer.error(409, "member_gone", "The helper had already failed.");
      else {
        if (helper.state !== "stopped") {
          helper.state = "stopped";
          helper.activity = null;
          post(
            team,
            "notice",
            { kind: "gateway" },
            { kind: "lead" },
            `${helper.name} was stopped by its person. Its task stays. Don't start another helper for its part unless your person asks.`,
            {
              kind: "member_stopped",
              member: helper.name,
            },
          );
          changed(team);
        }
        answer.json(202, { member: { name: helper.name, state: "stopped" } });
      }
    }
  } else if (suffix === "/team/_fixture" && method === "POST") {
    // Test hooks: the lead asks its person, a helper reports to the lead, a helper's state moves, or the team stops for
    // a reason.
    const input = await read();
    // Addressed to the person, as the gateway writes it (tqa-types ToKind::Person).
    if (input.ask)
      post(
        team,
        "person_question",
        { kind: "lead" },
        { kind: "person" },
        input.ask,
      );
    if (input.report)
      post(
        team,
        "report",
        { kind: "member", name: input.report.from },
        { kind: "lead" },
        input.report.text,
      );
    if (input.helper) {
      const helper = team.helpers.find((each) => each.name === input.helper);
      if (helper) Object.assign(helper, input.fields ?? {});
    }
    // A further helper like an existing one, under its own name (WEB-U-369: a strip with three long names).
    if (input.copy) {
      const like = team.helpers.find((each) => each.name === input.copy.from);
      if (like)
        team.helpers.push({
          ...like,
          name: input.copy.name,
          conversation: { ...like.conversation, id: randomUUID() },
        });
    }
    if (input.end) end(team, input.end);
    else changed(team);
    answer.json(200, view(team));
  } else answer.error(404, "not_found", "Not found.");
  return true;
}

/**
 * Deleting a lead with helpers: refused without with_helpers (409 has_helpers, no details), else its helpers go too.
 * Returns true when it answered (a refusal); otherwise deletes the helpers and leaves the lead's delete to the server.
 */
export function deleteTeamTask(
  conversation,
  conversations,
  url,
  refuse,
  close,
) {
  const place = on ? places.get(conversation) : undefined;
  if (!place) return false;
  const { team } = place;
  if (place.role === "member") {
    place.helper.deleted = true;
    post(
      team,
      "notice",
      { kind: "gateway" },
      { kind: "lead" },
      `${place.helper.name} was deleted.`,
      {
        kind: "member_deleted",
        member: place.helper.name,
      },
    );
    changed(team);
    return false;
  }
  const helpers = live(team);
  if (helpers.length && url.searchParams.get("with_helpers") !== "true") {
    refuse(409, "has_helpers", "This task leads helpers.");
    return true;
  }
  for (const helper of helpers) {
    close(helper.conversation);
    conversations.delete(helper.conversation.id);
  }
  if (team.state !== "ended") end(team, "lead_deleted");
  teams.delete(conversations);
  return false;
}

const settings = seededMap([
  ["value", { member_route: null, acceptance_check: null, version: 0 }],
]);

/** /api/v1/admin/agent-teams for administrators; returns false for any other path. */
export async function agentTeamsRoute(member, url, method, read, answer) {
  if (url.pathname !== "/api/v1/admin/agent-teams") return false;
  if (member.account.role !== "admin") answer.refuse(403, "forbidden");
  else if (!on) answer.refuse(403, "teams_disabled");
  else if (method === "GET") answer.ok(200, settings.get("value"));
  else if (method === "PUT") {
    const input = await read();
    const stored = settings.get("value");
    const check = input.acceptance_check;
    if (input.version !== stored.version) answer.refuse(409, "conflict");
    else if (input.member_route !== null && input.member_route !== "local")
      answer.refuse(422, "unknown_reference", [
        { field: "member_route", reason: "unknown_reference" },
      ]);
    else if (
      check !== null &&
      (/[\r\n\0]/.test(check) || byteLength(check) > 4096)
    )
      // As the gateway: a line break or NUL is value_invalid, too long value_too_long.
      answer.refuse(400, "invalid_request", [
        {
          field: "acceptance_check",
          reason: /[\r\n\0]/.test(check) ? "value_invalid" : "value_too_long",
        },
      ]);
    else {
      const next = { ...input, version: stored.version + 1 };
      settings.set("value", next);
      answer.ok(200, next);
    }
  } else return false;
  return true;
}
