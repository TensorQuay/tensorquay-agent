// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../api/client";
import { ApiError } from "../api/error";

import {
  credentialFailure,
  gatewayCredentialsFor,
} from "./connector-credentials";

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/settings/connector-credentials",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return { url, method: init?.method, body: init?.body };
  };
  return { credentials: gatewayCredentialsFor(client), sent };
}

// The credentials page on the gateway's per-user API (G3; WEB-U-044): values go out once and never come back.
describe("WEB-U-044 the connector credentials client", () => {
  it("lists, sets and deletes by connector name, and refuses a malformed list", async () => {
    const item = {
      name: "tracker",
      display_name: null,
      scheme: "bearer",
      required: true,
      has_credential: false,
      credential_updated_at: null,
    };
    const { credentials, sent } = setup(
      Response.json({ items: [item] }),
      new Response(null, { status: 204 }),
      new Response(null, { status: 204 }),
      Response.json({ items: [{ name: "tracker" }] }),
    );
    expect((await credentials.connectors()).items).toEqual([item]);
    expect(sent(0)).toMatchObject({ url: "/api/v1/connectors", method: "GET" });
    await credentials.putCredential("tracker", "tok-1");
    expect(sent(1)).toEqual({
      url: "/api/v1/connectors/tracker/credential",
      method: "PUT",
      body: '{"value":"tok-1"}',
    });
    await credentials.deleteCredential("tracker");
    expect(sent(2)).toMatchObject({
      url: "/api/v1/connectors/tracker/credential",
      method: "DELETE",
    });
    await expect(credentials.connectors()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("names each refusal in plain words", () => {
    const refusal = (status: number, code: string) =>
      new ApiError(status, code, "x");
    expect(credentialFailure(refusal(400, "invalid_request"))).toBe(
      "credentials.valueInvalid",
    );
    expect(credentialFailure(refusal(403, "session_required"))).toBe(
      "credentials.sessionRequired",
    );
    expect(credentialFailure(refusal(409, "credential_not_used"))).toBe(
      "credentials.notUsed",
    );
    expect(credentialFailure(refusal(404, "not_found"))).toBe(
      "credentials.gone",
    );
    expect(credentialFailure(refusal(503, "unavailable"))).toBe(
      "credentials.saveFailed",
    );
    expect(credentialFailure(new Error("offline"))).toBe(
      "credentials.saveFailed",
    );
  });
});

// Vault slice 2: the reads of a person's own credential, paged (WEB-U-053).
describe("WEB-U-053 the connector credential reads client", () => {
  it("pages with limit and before, and refuses a malformed page", async () => {
    const page = {
      items: [
        {
          id: "0195d8c0-0000-7000-8000-000000000e01",
          consumer_kind: "route",
          consumer: "local",
          conversation_id: null,
          user_id: null,
          run_id: null,
          outcome: "ok",
          count: 1,
          first_at: "2026-09-26T09:00:00Z",
          last_at: "2026-09-26T09:00:00Z",
        },
      ],
      next: null,
    };
    const { credentials, sent } = setup(
      Response.json(page),
      Response.json(page),
      Response.json({ items: [{ id: "x" }], next: null }),
    );
    expect(await credentials.reads("tracker")).toEqual(page);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/connectors/tracker/credential/reads?limit=50",
      method: "GET",
    });
    await credentials.reads("tracker", "0195d8c0-0000-7000-8000-000000000e01");
    expect(sent(1).url).toBe(
      "/api/v1/connectors/tracker/credential/reads?limit=50&before=0195d8c0-0000-7000-8000-000000000e01",
    );
    await expect(credentials.reads("tracker")).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});
