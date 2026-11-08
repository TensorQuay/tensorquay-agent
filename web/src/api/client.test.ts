// @vitest-environment node
import { readFileSync } from "node:fs";

import { afterEach, describe, expect, it, vi } from "vitest";

import { setupStateOf } from "../auth/setup-state";
import { lookupInvitationOf } from "../join/lookup";
import { settingsOperations, settingsEndpoint } from "../settings/operations";
import { settingsRequests } from "../settings/requests";
import {
  answerTeam,
  readTeam,
  readTeamMessages,
  stopTeam,
} from "../teams/requests";
import { conversationId } from "../test/frames";
import { leadId, team, teamMessage } from "../test/teams";
import { templates } from "../test/templates";
import { LIMITS } from "../test/user";

import { ApiClient, ApiError } from "./client";
import { operations } from "./operations";

const user = {
  id: conversationId,
  email: "member@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const error = (
  status: number,
  code: string,
  message: string,
  headers: HeadersInit = {},
) =>
  Response.json(
    { error: { code, message, retryable: status === 429 } },
    { status, headers },
  );
function setup() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  const redirect = vi.fn<(path: string) => void>();
  const sleep = vi
    .fn<(milliseconds: number, signal?: AbortSignal) => Promise<void>>()
    .mockResolvedValue(undefined);
  const client = new ApiClient({
    fetch,
    redirect,
    sleep,
    currentPath: () => `/c/${conversationId}?view=latest`,
  });
  return { client, fetch, redirect, sleep };
}
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("GW-U-022 browser API contract", () => {
  it("uses only method/path templates present in the real OpenAPI document", () => {
    const document: unknown = JSON.parse(
      readFileSync(
        new URL("../../../crates/tqa-gateway/openapi.json", import.meta.url),
        "utf8",
      ),
    );
    expect(document).toHaveProperty("openapi", "3.1.0");
    for (const operation of [
      ...Object.values(operations),
      ...Object.values(settingsOperations),
    ]) {
      expect(document).toHaveProperty([
        "paths",
        operation.path,
        operation.method.toLowerCase(),
      ]);
    }
    // The settings pages' operations live in their own table, outside the conversation page's scripts (WEB-U-014).
    // ADR-0034 S6 added the team, its messages, the answer and the stop here, and the helpers' settings there.
    // ADR-0039 added a desktop task's screen, its notices, its screenshots and its Sign out everywhere here.
    expect(Object.keys(operations)).toHaveLength(49);
    expect(Object.keys(settingsOperations)).toHaveLength(79);
    expect(
      Object.keys(operations).filter((name) => name in settingsOperations),
    ).toEqual([]);
  });

  it("sends a settings endpoint through the same client, with its method, path and key", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await client.request(
      settingsEndpoint("adminConnectorDelete"),
      () => undefined,
      {
        parameters: { name: "tracker" },
        query: new URLSearchParams({ version: "2" }),
        key: "key-1",
      },
    );
    const [url, init] = fetch.mock.calls[0] ?? [];
    expect(url).toBe("/api/v1/admin/connectors/tracker?version=2");
    expect(init?.method).toBe("DELETE");
    expect(new Headers(init?.headers).get("idempotency-key")).toBe("key-1");
  });
});

describe("typed JSON requests", () => {
  it("WEB-U-018 accepts only the generated template catalogue contract", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json(templates));
    await expect(client.templates()).resolves.toEqual(templates);
    expect(fetch.mock.calls[0]?.[0]).toBe("/api/v1/templates");
    // An undeclared capability (revision 14 declared desktop, so another stands for it) and a repeated one.
    for (const capabilities of [["gpu"], ["shell", "shell"]]) {
      fetch.mockResolvedValueOnce(
        Response.json({ items: [{ ...templates.items[0], capabilities }] }),
      );
      await expect(client.templates()).rejects.toMatchObject({
        code: "invalid_response",
      });
    }
  });
  it("reads only a boolean setup state and keys conversation updates", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json({ required: true }));
    await expect(setupStateOf(client)()).resolves.toEqual({ required: true });
    expect(fetch.mock.calls[0]?.[0]).toBe("/api/v1/setup");
    fetch.mockResolvedValueOnce(Response.json({ required: "yes" }));
    await expect(setupStateOf(client)()).rejects.toMatchObject({
      code: "invalid_response",
    });
    const action = client.updateConversation(conversationId, {
      route: "second",
    });
    fetch.mockResolvedValueOnce(error(503, "unavailable", "Try again"));
    await expect(action.execute()).rejects.toMatchObject({ status: 503 });
    fetch.mockResolvedValueOnce(error(503, "unavailable", "Try again"));
    await expect(action.execute()).rejects.toMatchObject({ status: 503 });
    expect(fetch.mock.calls[2]?.[1]?.method).toBe("PATCH");
    expect(fetch.mock.calls[2]?.[1]?.body).toBe('{"route":"second"}');
    expect(
      new Headers(fetch.mock.calls[2]?.[1]?.headers).get("idempotency-key"),
    ).toBe(
      new Headers(fetch.mock.calls[3]?.[1]?.headers).get("idempotency-key"),
    );
  });
  it("uses same-origin credentials and bounds malformed responses", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json({ role: "invented" }));
    await expect(client.me()).rejects.toMatchObject({
      code: "invalid_response",
    });
    fetch.mockResolvedValueOnce(Response.json(user));
    await expect(client.me()).resolves.toEqual(user);
    expect(fetch.mock.calls.at(-1)?.[1]?.signal).toBeInstanceOf(AbortSignal);
    expect(fetch).toHaveBeenLastCalledWith(
      "/api/v1/auth/me",
      expect.objectContaining({
        method: "GET",
        credentials: "same-origin",
        redirect: "error",
      }),
    );
  });

  it("redirects an expired session while preserving the local destination", async () => {
    const { client, fetch, redirect } = setup();
    fetch.mockResolvedValueOnce(error(401, "unauthorized", "Sign in required"));
    await expect(client.me()).rejects.toMatchObject({
      status: 401,
      code: "unauthorized",
      message: "Sign in required",
    });
    expect(redirect).toHaveBeenCalledWith(
      `/login?next=${encodeURIComponent(`/c/${conversationId}?view=latest`)}`,
    );
  });

  it.each(["/login", "/login?next=%2Fc%2Fexample", "/setup"])(
    "WEB-U-018 keeps a late catalogue 401 from redirecting %s after sign-out",
    async (destination) => {
      let currentPath = `/c/${conversationId}`;
      let respond: (response: Response) => void = () => {
        throw new Error("The request has not started");
      };
      const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(
        () =>
          new Promise((resolve) => {
            respond = resolve;
          }),
      );
      const redirect = vi.fn<(path: string) => void>();
      const client = new ApiClient({
        fetch,
        redirect,
        currentPath: () => currentPath,
      });
      const request = client.templates();
      currentPath = destination;
      respond(error(401, "unauthorized", "Sign in required"));
      await expect(request).rejects.toMatchObject({ status: 401 });
      expect(redirect).not.toHaveBeenCalled();
    },
  );

  it("shows login errors and Retry-After inline without redirecting or retrying", async () => {
    const { client, fetch, redirect, sleep } = setup();
    fetch.mockResolvedValueOnce(
      error(401, "unauthorized", "Credentials were not accepted"),
    );
    await expect(
      client.login({ email: user.email, password: "synthetic-password" }),
    ).rejects.toBeInstanceOf(ApiError);
    fetch.mockResolvedValueOnce(
      error(429, "busy", "Try again later", { "retry-after": "7" }),
    );
    await expect(
      client.login({ email: user.email, password: "synthetic-password" }),
    ).rejects.toMatchObject({ status: 429, retryAfter: 7 });
    expect(redirect).not.toHaveBeenCalled();
    expect(sleep).not.toHaveBeenCalled();
  });

  it("aborts a stalled fetch at the request deadline", async () => {
    vi.useFakeTimers();
    const { client, fetch } = setup();
    fetch.mockImplementation(
      (_url, init) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener(
            "abort",
            () => reject(new DOMException("Aborted", "AbortError")),
            { once: true },
          );
        }),
    );
    const result = client.me();
    const assertion = expect(result).rejects.toMatchObject({ code: "timeout" });
    await vi.advanceTimersByTimeAsync(10_000);
    await assertion;
  });

  it("encodes route parameters, handles bodyless success, and pages conversations", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await client.deleteConversation("an/id");
    expect(fetch.mock.calls[0]?.[0]).toBe("/api/v1/conversations/an%2Fid");
    fetch.mockResolvedValueOnce(
      Response.json({ items: [], next_cursor: null }),
    );
    await client.conversations("after/id");
    expect(fetch.mock.calls[1]?.[0]).toBe(
      "/api/v1/conversations?cursor=after%2Fid",
    );
  });

  it("WEB-U-193 reads a task's key uses a page at a time, each from the last one's next", async () => {
    const { client, fetch } = setup();
    fetch
      .mockResolvedValueOnce(
        Response.json({
          items: [],
          next: "0195d8c0-0000-7000-8000-0000000000aa",
        }),
      )
      .mockResolvedValueOnce(Response.json({ items: [], next: null }));
    expect(await client.keyUses("an/id", null)).toEqual({
      items: [],
      next: "0195d8c0-0000-7000-8000-0000000000aa",
    });
    await client.keyUses("an/id", "0195d8c0-0000-7000-8000-0000000000aa");
    expect(fetch.mock.calls.map(([url]) => url)).toEqual([
      "/api/v1/conversations/an%2Fid/key-uses?limit=100",
      "/api/v1/conversations/an%2Fid/key-uses?limit=100&before=0195d8c0-0000-7000-8000-0000000000aa",
    ]);
    // A page that breaks the contract is refused, like any other answer.
    fetch.mockResolvedValueOnce(
      Response.json({ items: [{ id: "x" }], next: null }),
    );
    await expect(client.keyUses("an/id", null)).rejects.toThrow();
  });
});

describe("durable user actions", () => {
  it.each(["native", "fallback"])(
    "retries teardown 429 with the original body and %s idempotency key",
    async (generator) => {
      if (generator === "fallback")
        vi.stubGlobal("crypto", {
          getRandomValues: crypto.getRandomValues.bind(crypto),
        });
      const { client, fetch, sleep } = setup();
      const body = { text: "original" };
      const action = client.message(conversationId, body);
      body.text = "changed after creating the action";
      fetch.mockResolvedValueOnce(
        error(429, "busy", "Sandbox is stopping", { "retry-after": "2" }),
      );
      fetch.mockResolvedValueOnce(
        Response.json(
          {
            accepted: true,
            conversation_id: conversationId,
            message_id: "41",
            held: false,
          },
          { status: 202 },
        ),
      );
      await action.execute();
      expect(sleep).toHaveBeenCalledWith(2000, undefined);
      const first = fetch.mock.calls[0]?.[1];
      const second = fetch.mock.calls[1]?.[1];
      expect(first?.body).toBe('{"text":"original"}');
      expect(second?.body).toBe(first?.body);
      const key = new Headers(first?.headers).get("idempotency-key");
      expect(key).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
      expect(new Headers(second?.headers).get("idempotency-key")).toBe(key);
      fetch.mockResolvedValueOnce(
        Response.json(
          {
            accepted: true,
            conversation_id: conversationId,
            message_id: "41",
            held: false,
          },
          { status: 202 },
        ),
      );
      await action.execute();
      expect(
        new Headers(fetch.mock.calls[2]?.[1]?.headers).get("idempotency-key"),
      ).toBe(key);
    },
  );

  // GW-I-129: a message's acceptance names it and says whether it waits for the running run.
  it("decodes a message's acceptance with its id and whether it is held", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(
      Response.json(
        {
          accepted: true,
          conversation_id: conversationId,
          message_id: "42",
          held: true,
        },
        { status: 202, headers: { "x-tqa-execution-status": "queued" } },
      ),
    );
    await expect(
      client
        .message(conversationId, { text: "Also update the README." })
        .execute(),
    ).resolves.toMatchObject({
      message_id: "42",
      held: true,
      executionStatus: "queued",
    });
  });

  it("bounds retries and preserves explicit errors for permanent action failures", async () => {
    const { client, fetch, sleep } = setup();
    fetch.mockResolvedValue(
      error(429, "busy", "Try later", { "retry-after": "1" }),
    );
    await expect(
      client.cancel(conversationId, "run-example").execute(),
    ).rejects.toMatchObject({ status: 429 });
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(sleep).toHaveBeenCalledTimes(2);
    fetch.mockResolvedValueOnce(
      error(422, "idempotency_key_reused", "Request differs"),
    );
    await expect(
      client.message(conversationId, { text: "new" }).execute(),
    ).rejects.toMatchObject({ code: "idempotency_key_reused" });
    expect(fetch).toHaveBeenCalledTimes(4);
  });
});

// WEB-U-023: team accounts through the public API only.
describe("WEB-U-023 team account requests", () => {
  const stamp = "2026-01-01T00:00:00Z";
  const token = {
    id: conversationId,
    name: "CI",
    hint: "abcd",
    created_at: stamp,
    expires_at: stamp,
    last_used_at: null,
  };

  it("creates a token once and never stores it", async () => {
    const { client, fetch } = setup();
    const created = { ...token, token: `tqa_pat_${"a".repeat(43)}` };
    fetch.mockResolvedValueOnce(Response.json(created, { status: 201 }));
    await expect(
      settingsRequests(client).createToken({ name: "CI", expires_in_days: 90 }),
    ).resolves.toEqual(created);
    const [url, init] = fetch.mock.calls[0] ?? [];
    expect(url).toBe("/api/v1/tokens");
    expect(init?.method).toBe("POST");
    expect(init?.body).toBe(
      JSON.stringify({ name: "CI", expires_in_days: 90 }),
    );
  });

  it("revokes tokens and invitations by id and changes accounts", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await settingsRequests(client).revokeToken(conversationId);
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await settingsRequests(client).revokeInvitation(conversationId);
    const account = {
      id: conversationId,
      email: "person@example.invalid",
      role: "member",
      disabled: true,
      created_at: stamp,
      last_active_at: null,
    };
    fetch.mockResolvedValueOnce(Response.json(account));
    await expect(
      settingsRequests(client).changeUser(conversationId, { disabled: true }),
    ).resolves.toEqual(account);
    expect(fetch.mock.calls.map(([url, init]) => [init?.method, url])).toEqual([
      ["DELETE", `/api/v1/tokens/${conversationId}`],
      ["DELETE", `/api/v1/invitations/${conversationId}`],
      ["PATCH", `/api/v1/users/${conversationId}`],
    ]);
  });

  it("looks up and accepts invitations and lists every task for administrators", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(
      Response.json({
        email: "person@example.invalid",
        role: "member",
        expires_at: stamp,
      }),
    );
    await lookupInvitationOf(client)("secret");
    fetch.mockResolvedValueOnce(
      Response.json(
        {
          id: conversationId,
          email: "person@example.invalid",
          role: "member",
          internet: "none",
          limits: LIMITS,
          features: { agent_teams: false },
        },
        { status: 201 },
      ),
    );
    await client.acceptInvitation({
      token: "secret",
      password: "synthetic-password",
    });
    fetch.mockResolvedValueOnce(
      Response.json({ items: [], next_cursor: null }),
    );
    await client.conversations(undefined, undefined, true);
    expect(fetch.mock.calls.map(([url]) => url)).toEqual([
      "/api/v1/invitations/lookup",
      "/api/v1/invitations/accept",
      "/api/v1/conversations?all=true",
    ]);
  });
});

// ADR-0034 S6: the people's side of an agent team. Reads decode against the contract; the answer and the stop are
// durable actions with one Idempotency-Key across retries.
describe("WEB-U-220/224/225/226 the agent team requests", () => {
  const sent = (fetch: ReturnType<typeof setup>["fetch"], index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return {
      url,
      method: init?.method,
      key: new Headers(init?.headers).get("idempotency-key"),
      body: init?.body,
    };
  };

  it("lists the top-level tasks, leaving helpers to their lead", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(
      Response.json({ items: [], next_cursor: null }),
    );
    await client.conversations(undefined, undefined, false, "", true);
    expect(sent(fetch, 0).url).toBe("/api/v1/conversations?scope=top");
  });

  it("reads the team and its messages after a seq, refusing an answer off the contract", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json(team()));
    expect(await readTeam(client, leadId)).toEqual(team());
    expect(sent(fetch, 0)).toMatchObject({
      url: `/api/v1/conversations/${leadId}/team`,
      method: "GET",
    });
    const page = {
      items: [teamMessage(16, "report")],
      last_seq: 16,
      more: false,
    };
    fetch.mockResolvedValueOnce(Response.json(page));
    expect(await readTeamMessages(client, leadId, 15)).toEqual(page);
    expect(sent(fetch, 1).url).toBe(
      `/api/v1/conversations/${leadId}/team/messages?after=15&limit=200`,
    );
    fetch.mockResolvedValueOnce(Response.json({ ...team(), state: "paused" }));
    await expect(readTeam(client, leadId)).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("answers the lead's question and stops the team as durable actions", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(Response.json({ seq: 17 }, { status: 201 }));
    expect(
      await answerTeam(client, leadId, {
        question_seq: 14,
        text: "July to September",
      }).execute(),
    ).toEqual({ seq: 17 });
    expect(sent(fetch, 0)).toMatchObject({
      url: `/api/v1/conversations/${leadId}/team/answers`,
      method: "POST",
      body: JSON.stringify({ question_seq: 14, text: "July to September" }),
    });
    expect(sent(fetch, 0).key).toBeTruthy();
    fetch.mockResolvedValueOnce(
      Response.json({ team: { state: "stopping" } }, { status: 202 }),
    );
    expect(await stopTeam(client, leadId).execute()).toEqual({
      team: { state: "stopping" },
    });
    expect(sent(fetch, 1)).toMatchObject({
      url: `/api/v1/conversations/${leadId}/team/stop`,
      method: "POST",
      body: "{}",
    });
    expect(sent(fetch, 1).key).toBeTruthy();
  });

  it("deletes a lead task with its helpers only when asked to", async () => {
    const { client, fetch } = setup();
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await client.deleteConversation(leadId);
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await client.deleteConversation(leadId, undefined, true);
    expect([sent(fetch, 0).url, sent(fetch, 1).url]).toEqual([
      `/api/v1/conversations/${leadId}`,
      `/api/v1/conversations/${leadId}?with_helpers=true`,
    ]);
  });
});
