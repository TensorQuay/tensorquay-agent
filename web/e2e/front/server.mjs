import { randomUUID } from "node:crypto";
import { appendFileSync, writeFileSync } from "node:fs";
import http from "node:http";

import {
  accountRoute,
  invitationRoute,
  membership,
  otherTask,
  sessionUser,
  teamTasks,
} from "./accounts.mjs";
import { adminRoute, readinessRoute } from "./admin.mjs";
import { channelRoute } from "./channels.mjs";
import { userConnectorRoute } from "./connectors.mjs";
import { contractProblem, driftLog } from "./contract.mjs";
import { switchEnvironment } from "./environments.mjs";
import { changesRoute, filesRoute, missingAttachment } from "./files.mjs";
import { models, templates, user } from "./fixtures.mjs";
import { hostKeysRoute, keyOfUse } from "./host-keys.mjs";
import { page } from "./pages.mjs";
import { profileRoute, taskProfile } from "./profiles.mjs";
import {
  screenFixture,
  restartScreen,
  screenNoticesRoute,
  screenSignOutRoute,
  screenshotRoute,
  screenUpgrade,
  withDesktop,
} from "./screen.mjs";
import { resetFront } from "./state.mjs";
import {
  advanceAdmission,
  claimWaiting,
  withdrawWaiting,
  loseSandbox,
  begin,
  holdMessage,
  publishHeld,
  cancel,
  close,
  restartStreams,
  askAgain,
  resume,
  subscribe,
  notify,
} from "./stream.mjs";
import {
  deleteTeamTask,
  listed,
  seedTeam,
  startTeam,
  teamOf,
  teamRoute,
} from "./teams.mjs";
import { bindable, workspaceRoute } from "./workspaces.mjs";

const sessions = new Map();
const memberships = new Map();
const creationKeys = new WeakMap();
const maximumSessions = 256;
const json = (response, status, value, headers = {}) => {
  response.writeHead(status, {
    "content-type": "application/json",
    ...headers,
  });
  response.end(JSON.stringify(value));
};
const error = (response, status, code, message) =>
  json(response, status, { error: { code, message, retryable: false } });
async function body(request) {
  let bytes = 0;
  let text = "";
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > 262144) throw new Error("Oversized fake request");
    text += chunk;
  }
  return text ? JSON.parse(text) : {};
}
/** A task as the fake front stores one, made outside a request (a scenario's seed). */
function fresh(member, fields) {
  const id = randomUUID();
  const now = new Date().toISOString();
  return {
    id,
    workspace_id: id,
    owner_id: member.account.id,
    title: "Task",
    model_route: "local",
    template: "workstation",
    pinned: false,
    title_source: "message",
    runtimeTemplate: "minimal",
    backend: "docker",
    state: "active",
    created_at: now,
    updated_at: now,
    last_activity_at: now,
    history: [],
    streams: new Set(),
    actions: new Map(),
    updates: new Map(),
    running: false,
    runs: 0,
    timer: null,
    ...fields,
  };
}
function metadata(value) {
  const {
    id,
    owner_id,
    title,
    title_source,
    model_route,
    template,
    pinned,
    backend,
    state,
    created_at,
    updated_at,
    last_activity_at,
    workspace_id,
    mcp_servers = [],
    connectors = [],
    team = null,
    profile,
  } = value;
  return {
    id,
    owner_id,
    title,
    title_source,
    model_route,
    template,
    pinned,
    backend,
    state,
    created_at,
    updated_at,
    last_activity_at,
    workspace_id,
    mcp_servers,
    connectors,
    team: teamOf(value) ?? team,
    run_status: runStatus(value.history ?? []),
    ...(profile ? { profile } : {}),
  };
}
/** As the gateway: the task's last run as recorded, "idle" before any (a first message still waiting for a slot). */
function runStatus(history) {
  const ended = {
    "run.completed": "completed",
    "run.failed": "failed",
    "run.cancelled": "cancelled",
  };
  for (let index = history.length - 1; index >= 0; index -= 1) {
    const type = history[index].type === "event" && history[index].event.type;
    if (type === "run.started" || type === "run.resumed") return "running";
    if (ended[type]) return ended[type];
  }
  return "idle";
}
function signIn(response, status, member) {
  if (sessions.size >= maximumSessions) {
    const oldest = sessions.keys().next().value;
    for (const value of sessions.get(oldest).values()) close(value);
    sessions.delete(oldest);
    memberships.delete(oldest);
  }
  const token = randomUUID();
  sessions.set(token, new Map());
  memberships.set(token, member);
  json(response, status, sessionUser(member), {
    "set-cookie": [
      `fixture_session=${token}; HttpOnly; SameSite=Strict; Path=/`,
      "fixture_setup=done; SameSite=Strict; Path=/",
    ],
  });
}
function patch(response, conversation, input, key) {
  if (typeof key !== "string" || !/^[!-~]{1,128}$/.test(key)) {
    error(response, 400, "bad_request", "An idempotency key is required.");
    return;
  }
  const fingerprint = JSON.stringify(input);
  const previous = conversation.updates.get(key);
  if (previous) {
    if (previous.fingerprint !== fingerprint)
      error(
        response,
        422,
        "idempotency_key_reused",
        "This key belongs to another request.",
      );
    else json(response, 200, previous.response);
    return;
  }
  if (input.route && conversation.running) {
    error(response, 409, "run_active", "A run is active.");
    return;
  }
  if (conversation.updates.size >= 1000) {
    error(response, 429, "busy", "The fixture update limit was reached.");
    return;
  }
  if (input.title !== undefined) {
    const title = input.title.trim();
    if (!title || Array.from(title).length > 200) {
      error(response, 400, "bad_request", "Invalid title.");
      return;
    }
    conversation.title = title;
    // A rename is the person's, never replaced (WEB-U-081).
    conversation.title_source = "person";
  }
  // The keep-alive pin (FR-33), set or cleared as the gateway does.
  if (input.pinned !== undefined) {
    if (typeof input.pinned !== "boolean") {
      error(response, 400, "bad_request", "Invalid pin.");
      return;
    }
    conversation.pinned = input.pinned;
  }
  if (input.route !== undefined) {
    if (!models.items.some((model) => model.name === input.route)) {
      error(response, 400, "bad_request", "Unknown model.");
      return;
    }
    conversation.model_route = input.route;
  }
  conversation.updated_at = new Date().toISOString();
  const result = metadata(conversation);
  conversation.updates.set(key, { fingerprint, response: result });
  json(response, 200, result);
}
async function handle(request, response) {
  const url = new URL(request.url, "http://localhost:3100");
  if (url.pathname === "/healthz") {
    response.end("ready");
    return;
  }
  // Test control, only in this fake (never the web build): back to the seed before each e2e test (e2e/test.ts).
  if (url.pathname === "/__fake/reset" && request.method === "POST") {
    const { scenarios = [] } = await body(request);
    resetFront(scenarios);
    response.statusCode = 204;
    response.end();
    return;
  }
  // Test control: the gateway restarting under a task's open page (WEB-U-234). With `gateway_link_ms`, it restarted
  // under a step: the runtime then gives the lost model call back (first-hour F7).
  if (url.pathname === "/__fake/restart" && request.method === "POST") {
    const id = url.searchParams.get("conversation");
    const linkMs = Number(url.searchParams.get("gateway_link_ms") ?? 0);
    for (const conversations of sessions.values()) {
      const conversation = conversations.get(id);
      if (!conversation) continue;
      restartStreams(conversation);
      if (linkMs > 0) askAgain(conversation, linkMs);
    }
    // A desktop task's screen restarts with the gateway: its Take over ends, as the new process records (ADR-0039).
    if (id) restartScreen(id);
    response.statusCode = 204;
    response.end();
    return;
  }
  // Test control: a desktop task's screen (ADR-0039; WEB-E2E's screen spec).
  if (await screenFixture(url, request, response, body)) return;
  if (
    readinessRoute(url, request, response, memberships, json, error, sessions)
  )
    return;
  if (!url.pathname.startsWith("/api/")) {
    page(request, response);
    return;
  }
  const method = request.method;
  const path = url.pathname;
  if (path === "/api/v1/setup" && method === "GET") {
    json(response, 200, {
      required: /(?:^|;\s*)fixture_setup=required(?:;|$)/.test(
        request.headers.cookie ?? "",
      ),
    });
    return;
  }
  if (path === "/api/v1/auth/bootstrap" && method === "POST") {
    const setup = await body(request);
    if (setup.token === "wrong") {
      error(response, 401, "unauthorized", "authentication required");
      return;
    }
    signIn(response, 201, membership());
    return;
  }
  if (path === "/api/v1/auth/login" && method === "POST") {
    const login = await body(request);
    if (login.password === "wrong") {
      error(
        response,
        401,
        "unauthenticated",
        "Email or password is incorrect.",
      );
      return;
    }
    signIn(response, 200, membership(login.email));
    return;
  }
  const reply = (status, value) => {
    if (value === undefined) {
      response.writeHead(status);
      response.end();
    } else json(response, status, value);
  };
  const refuse = (status, code, message) =>
    error(response, status, code, message);
  const joined = await invitationRoute(
    path,
    method,
    () => body(request),
    reply,
    refuse,
  );
  if (joined && joined !== true) {
    signIn(response, 201, joined);
    return;
  }
  if (joined) return;
  const token = /(?:^|;\s*)fixture_session=([^;]+)/.exec(
    request.headers.cookie ?? "",
  )?.[1];
  const conversations = sessions.get(token);
  const member = memberships.get(token);
  if (!conversations || !member) {
    error(response, 401, "unauthenticated", "Sign in to continue.");
    return;
  }
  // ADR-0034 S6: in the "agent-teams" scenario, a lead task and its helpers wait in every session.
  seedTeam(conversations, (fields) => fresh(member, fields));
  if (path === "/api/v1/auth/logout" && method === "POST") {
    for (const value of conversations.values()) close(value);
    sessions.delete(token);
    memberships.delete(token);
    response.writeHead(204, {
      "set-cookie": "fixture_session=; Max-Age=0; Path=/",
    });
    response.end();
    return;
  }
  if (path === "/api/v1/auth/me" && method === "GET") {
    json(response, 200, sessionUser(member));
    return;
  }
  if (
    await accountRoute(member, path, method, () => body(request), reply, refuse)
  )
    return;
  if (
    await profileRoute(member, url, method, () => body(request), reply, refuse)
  )
    return;
  if (
    await channelRoute(
      member,
      url,
      method,
      () => body(request),
      request.headers["idempotency-key"],
      reply,
      { request, response },
    )
  )
    return;
  if (
    await userConnectorRoute(
      member,
      url,
      method,
      () => body(request),
      reply,
      refuse,
    )
  )
    return;
  if (await hostKeysRoute(member, url, method, () => body(request), reply))
    return;
  if (
    await adminRoute(
      member,
      url,
      method,
      () => body(request),
      request,
      response,
      json,
    )
  )
    return;
  if (
    await workspaceRoute(
      member,
      conversations,
      url,
      method,
      () => body(request),
      reply,
      refuse,
      close,
    )
  )
    return;
  if (path === "/api/v1/models" && method === "GET") {
    json(response, 200, models);
    return;
  }
  if (path === "/api/v1/templates" && method === "GET") {
    json(response, 200, withDesktop(templates));
    return;
  }
  if (path === "/api/v1/conversations") {
    if (method === "GET") {
      const all = url.searchParams.get("all") === "true";
      if (all && member.account.role !== "admin") {
        refuse(403, "forbidden", "Administrators only.");
        return;
      }
      json(response, 200, {
        items: [
          ...[...conversations.values()]
            .filter((value) => listed(value, url))
            .map(metadata),
          ...(all ? teamTasks(member) : []),
        ],
        next_cursor: null,
      });
      return;
    }
    if (method === "POST") {
      const input = await body(request);
      const template =
        input.template ??
        templates.items.find((value) => value.is_default).name;
      if (
        !withDesktop(templates).items.some((value) => value.name === template)
      ) {
        error(response, 422, "unknown_template", "Unknown environment.");
        return;
      }
      const key = request.headers["idempotency-key"];
      if (typeof key !== "string" || !/^[!-~]{1,128}$/.test(key)) {
        error(response, 400, "bad_request", "An idempotency key is required.");
        return;
      }
      const keys = creationKeys.get(conversations) ?? new Map();
      creationKeys.set(conversations, keys);
      const fingerprint = JSON.stringify({
        title: input.title,
        model_route: input.model_route,
        template,
        workspace_id: input.workspace_id,
      });
      const previous = keys.get(key);
      if (previous) {
        if (previous.fingerprint !== fingerprint)
          error(
            response,
            422,
            "idempotency_key_reused",
            "This key belongs to another request.",
          );
        else json(response, 201, previous.response);
        return;
      }
      if (keys.size >= 1000) {
        error(
          response,
          429,
          "busy",
          "The fixture conversation limit was reached.",
        );
        return;
      }
      const refusal =
        input.workspace_id &&
        bindable(member, conversations, input.workspace_id);
      if (refusal) {
        error(
          response,
          refusal[0],
          refusal[1],
          "The workspace cannot be used.",
        );
        return;
      }
      const identity = randomUUID();
      // A task made from a profile keeps it, and takes the model and environment it sets (ADR-0012; WEB-U-051).
      const made = input.profile ? taskProfile(member, input.profile) : null;
      if (input.profile && !made) {
        error(response, 409, "profile_unusable", "The profile can't be used.");
        return;
      }
      const conversation = {
        id: identity,
        workspace_id: input.workspace_id ?? identity,
        owner_id: member.account.id,
        title: input.title,
        model_route: made?.model_route ?? input.model_route,
        template: made?.template ?? template,
        ...(made ? { profile: made.profile } : {}),
        pinned: false,
        // WEB-U-081: a title from the first message until the gateway names the task or its person renames it.
        title_source: "message",
        runtimeTemplate: "minimal",
        backend: "docker",
        state: "active",
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        last_activity_at: new Date().toISOString(),
        history: [],
        streams: new Set(),
        actions: new Map(),
        updates: new Map(),
        running: false,
        runs: 0,
        timer: null,
      };
      conversations.set(conversation.id, conversation);
      keys.set(key, { fingerprint, response: metadata(conversation) });
      json(response, 201, metadata(conversation));
      return;
    }
  }
  const route = /^\/api\/v1\/conversations\/([^/]+)(.*)$/.exec(path);
  const conversation = route && conversations.get(route[1]);
  // Another member's task: administrators read its metadata (no title); nothing else is allowed.
  const foreign = route && !conversation && otherTask(member, route[1]);
  if (foreign) {
    if (member.account.role !== "admin")
      refuse(404, "not_found", "Conversation not found.");
    else if (!route[2] && method === "GET") json(response, 200, foreign);
    else refuse(403, "forbidden", "Only the owner can do this.");
    return;
  }
  if (!conversation) {
    error(response, 404, "not_found", "Conversation not found.");
    return;
  }
  const suffix = route[2];
  // Fixture only: continues a run held after its narration (stream.mjs HOLD), so a test checks the live run first.
  if (suffix === "/fixture-resume" && method === "POST") {
    resume(conversation);
    response.writeHead(204);
    response.end();
    return;
  }
  // Fixture only: this task, in no team, starts one now (teams.mjs startTeam).
  if (suffix === "/team/_fixture/start" && method === "POST") {
    if (
      startTeam(
        conversation,
        conversations,
        (fields) => fresh(member, fields),
        Number(url.searchParams.get("announce_after_ms") ?? 0),
      )
    ) {
      response.writeHead(204);
      response.end();
    } else
      error(
        response,
        409,
        "conflict",
        "Teams are off or the task is in a team.",
      );
    return;
  }
  const answer = {
    json: (status, value) => json(response, status, value),
    error: (status, code, message) => error(response, status, code, message),
  };
  if (
    await teamRoute(
      conversation,
      suffix,
      method,
      url,
      () => body(request),
      request.headers["idempotency-key"],
      answer,
    )
  )
    return;
  if (await filesRoute(request, response, conversation, suffix, url, answer))
    return;
  if (screenshotRoute(suffix, request.method, response, answer)) return;
  if (screenNoticesRoute(suffix, request.method, conversation.id, answer))
    return;
  if (
    screenSignOutRoute(suffix, method, conversation.id, answer, (value) =>
      notify(conversation, value),
    )
  )
    return;
  if (
    request.method === "GET" &&
    changesRoute(conversation, suffix, url, answer)
  )
    return;
  if (suffix === "/_fixture/recovery" && method === "POST") {
    const input = await body(request);
    loseSandbox(conversation, input.state, input.losses, input.retry_in_secs);
    json(response, 200, { lost: true });
    return;
  }
  // Fixture only: the task handed to another owner, so the signed-in person sees it as someone it was shared with does
  // (the suggestion card without its buttons, 28b; no screen). The gateway never changes a task's owner.
  if (suffix === "/_fixture/owner" && method === "POST") {
    conversation.owner_id = (await body(request)).owner_id;
    json(response, 200, { owner_id: conversation.owner_id });
    return;
  }
  // Fixture only: the gateway claims the waiting message for its turn before the stream says so, the race a Cancel
  // can lose (409 message_sent, GW-I-173).
  if (suffix === "/_fixture/claim" && method === "POST") {
    claimWaiting(conversation);
    json(response, 200, { claimed: true });
    return;
  }
  if (suffix === "/_fixture/admission" && method === "POST") {
    if (advanceAdmission(conversation)) json(response, 200, { advanced: true });
    else
      error(
        response,
        409,
        "fixture_state",
        "The fixture has no queued message.",
      );
    return;
  }
  if (suffix === "/environment" && method === "PUT") {
    const result = switchEnvironment(
      conversation,
      await body(request),
      request.headers["idempotency-key"],
      metadata,
    );
    json(response, result.status, result.body);
    return;
  }
  if (!suffix && method === "GET") {
    json(response, 200, metadata(conversation));
    return;
  }
  if (!suffix && method === "PATCH") {
    patch(
      response,
      conversation,
      await body(request),
      request.headers["idempotency-key"],
    );
    return;
  }
  if (!suffix && method === "DELETE") {
    if (deleteTeamTask(conversation, conversations, url, refuse, close)) return;
    close(conversation);
    conversations.delete(conversation.id);
    response.writeHead(204);
    response.end();
    return;
  }
  if (suffix === "/events" && method === "GET") {
    subscribe(request, response, conversation, url);
    return;
  }
  // ADR-0037: the task's stored key uses, newest first, a page at a time.
  if (suffix === "/key-uses" && method === "GET") {
    const limit = Number(url.searchParams.get("limit") ?? 50);
    if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
      error(response, 400, "bad_request", "The limit is 1 to 100.");
      return;
    }
    // A deleted key's rows go with it; a key's name only to a sender who may still use it.
    const viewer = member.account.id;
    const rows = (conversation.keyUses ?? [])
      .flatMap((row) => {
        const used = keyOfUse(member.team, row.sender_id, viewer, row);
        if (!used) return [];
        const named = row.sender_id === viewer && used.usable;
        return [{ ...row, name: named ? row.name : null }];
      })
      .reverse();
    const before = url.searchParams.get("before");
    const from = before ? rows.findIndex((row) => row.id === before) + 1 : 0;
    const items = rows.slice(from, from + limit);
    json(response, 200, {
      items,
      next: from + limit < rows.length ? (items.at(-1)?.id ?? null) : null,
    });
    return;
  }
  // GW-I-129: the task's held, dropped and recently sent messages; cancelling a held one or dismissing a dropped one.
  if (suffix === "/messages/queued" && method === "GET") {
    json(response, 200, { items: conversation.held ?? [] });
    return;
  }
  const queued = /^\/messages\/queued\/([^/]+)$/.exec(suffix);
  if (queued && method === "DELETE") {
    const messages = conversation.held ?? [];
    const found = messages.find((m) => m.id === queued[1]);
    if (!found) {
      error(response, 404, "not_found", "No such message.");
      return;
    }
    if (found.state === "sent") {
      error(response, 409, "message_sent", "It was already sent.");
      return;
    }
    conversation.held = messages.filter((m) => m !== found);
    // As the gateway: every cancel or dismiss is a change the other tabs hear about.
    if (found.state === "waiting_for_slot") withdrawWaiting(conversation);
    else publishHeld(conversation);
    response.writeHead(204);
    response.end();
    return;
  }
  if (
    method === "POST" &&
    (suffix === "/messages" || suffix === `/runs/${conversation.run}/cancel`)
  ) {
    const input = await body(request);
    const key = request.headers["idempotency-key"];
    if (!key) {
      error(response, 400, "bad_request", "An idempotency key is required.");
      return;
    }
    if (
      suffix === "/messages" &&
      missingAttachment(conversation, input.attachments)
    ) {
      error(
        response,
        400,
        "attachment_missing",
        "An attachment is not an uploaded file of this task.",
      );
      return;
    }
    const fingerprint = JSON.stringify({ suffix, input });
    const previous = conversation.actions.get(key);
    if (previous && previous !== fingerprint) {
      error(
        response,
        422,
        "idempotency_key_reused",
        "This key belongs to another action.",
      );
      return;
    }
    // As the gateway (GW-I-129): a message's acceptance names it, the same id for a retry of the same key; one sent
    // while a run is active is held until the run ends.
    const ids = (conversation.messageIds ??= new Map());
    const heldKeys = (conversation.heldKeys ??= new Set());
    if (suffix === "/messages" && !ids.has(key))
      ids.set(key, String(ids.size + 1));
    if (!previous) {
      conversation.actions.set(key, fingerprint);
      if (suffix !== "/messages") cancel(conversation);
      else if (
        holdMessage(
          conversation,
          ids.get(key),
          input.text,
          input.attachments ?? [],
          { id: user.id, email: user.email },
        )
      )
        heldKeys.add(key);
      else {
        // Fixture only: a run held after its narration whatever its text (stream.mjs HOLD), while the test's
        // fixture_hold cookie is set, so a test can see the live run of a message it doesn't choose.
        conversation.holdNarration =
          /(?:^|;\s*)fixture_hold=narration(?:;|$)/.test(
            request.headers.cookie ?? "",
          );
        // Fixture only: the slow scenario (slow-run.mjs) held at each kind of step's first showing, for its checks.
        conversation.holdSteps = /(?:^|;\s*)fixture_hold=steps(?:;|$)/.test(
          request.headers.cookie ?? "",
        );
        begin(conversation, input.text, input.attachments ?? [], {
          id: ids.get(key),
          author: { id: user.id, email: user.email },
        });
      }
    }
    json(
      response,
      202,
      suffix === "/messages"
        ? {
            accepted: true,
            conversation_id: conversation.id,
            message_id: ids.get(key),
            held: heldKeys.has(key),
          }
        : { accepted: true, conversation_id: conversation.id },
      {
        "x-tqa-execution-status": conversation.admission
          ? "queued"
          : "delivered",
      },
    );
    return;
  }
  error(response, 404, "not_found", "Route not found.");
}

// Every JSON response keeps the gateway's contract (contract.mjs); a drift is recorded for the e2e teardown.
writeFileSync(driftLog(3100), "");
function checkContract(request, response) {
  const end = response.end.bind(response);
  response.end = (chunk, ...rest) => {
    // A JSON body (writeHead's headers aren't readable here, so the body says what it is); a stream or page isn't.
    const text = chunk ? String(chunk).trimStart() : "";
    let body;
    if (text.startsWith("{") || text.startsWith("["))
      try {
        body = JSON.parse(text);
      } catch {
        body = undefined;
      }
    if (body !== undefined) {
      const pathname = new URL(request.url ?? "/", "http://front").pathname;
      const problem = contractProblem(
        request.method,
        pathname,
        response.statusCode,
        body,
      );
      if (problem) {
        console.error(`Fake front drifted from openapi.json: ${problem}`);
        appendFileSync(driftLog(3100), `${problem}\n`);
      }
    }
    return end(chunk, ...rest);
  };
}

const server = http.createServer(
  { requestTimeout: 10_000, headersTimeout: 5000 },
  (request, response) => {
    checkContract(request, response);
    void handle(request, response).catch((failure) => {
      // A fixture bug says what it was, not only that the request got a 500 (the contract's drift line).
      console.error(`fake front: ${request.method} ${request.url}:`, failure);
      if (!response.headersSent)
        error(
          response,
          500,
          "fixture_error",
          "The test front rejected a request.",
        );
      else response.destroy();
    });
  },
);
// A desktop task's screen (ADR-0039 §5): the WebSocket upgrade of its /screen path, owner only.
server.on("upgrade", (request, socket) => {
  const handled = screenUpgrade(request, socket, (upgrade, id) => {
    const token = /(?:^|;\s*)fixture_session=([^;]+)/.exec(
      upgrade.headers.cookie ?? "",
    )?.[1];
    const conversations = sessions.get(token);
    if (!conversations) return "unauthenticated";
    return conversations.get(id) ?? null;
  });
  if (!handled) socket.destroy();
});
// A real non-loopback HTTP origin exercises browser APIs outside a secure context.
server.listen(3100, "0.0.0.0");
function stop() {
  for (const conversations of sessions.values())
    for (const conversation of conversations.values()) close(conversation);
  server.closeAllConnections();
  server.close();
}
process.once("SIGTERM", stop);
process.once("SIGINT", stop);
