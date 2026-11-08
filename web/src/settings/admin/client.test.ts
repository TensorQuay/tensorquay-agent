// @vitest-environment node
import { describe, expect, it, vi } from "vitest";

import { ApiClient } from "../../api/client";
import { connector } from "../../test/connectors";
import { engine } from "../../test/decision-engines";
import { SIZE_CEILING, SIZE_DEFAULTS } from "../../test/limits";
import { probeResult, route } from "../../test/routes";
import { environment } from "../../test/templates-admin";

import {
  gatewayAdminFor,
  gatewayConnectorsFor,
  gatewayEgressFor,
  gatewayEnginesFor,
  gatewayLimitsFor,
  gatewayRoutesFor,
  gatewayTemplatesFor,
} from "./gateway";
import { fieldsOf } from "./routes";

/** The environment sizes every limits and templates list carries: the defaults and the ceiling. */
const SIZES = { defaults: SIZE_DEFAULTS, ceiling: SIZE_CEILING };

function setup(...answers: Response[]) {
  const fetch = vi.fn<typeof globalThis.fetch>();
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  const client = new ApiClient({
    fetch,
    redirect: vi.fn(),
    sleep: vi.fn().mockResolvedValue(undefined),
    currentPath: () => "/settings/models",
  });
  const sent = (index: number) => {
    const [url, init] = fetch.mock.calls[index] ?? [];
    return {
      url: typeof url === "string" ? url : url instanceof URL ? url.href : "",
      method: init?.method,
      key: new Headers(init?.headers).get("idempotency-key"),
      body:
        typeof init?.body === "string"
          ? (JSON.parse(init.body) as unknown)
          : null,
    };
  };
  return {
    routes: gatewayRoutesFor(client),
    egress: gatewayEgressFor(client),
    admin: gatewayAdminFor(client),
    limits: gatewayLimitsFor(client),
    connectors: gatewayConnectorsFor(client),
    engines: gatewayEnginesFor(client),
    templates: gatewayTemplatesFor(client),
    sent,
  };
}

// The Models page on the gateway's routes admin API (slice 3, ADR-0028; WEB-U-039).
describe("WEB-U-039 the routes admin client", () => {
  it("lists routes and refuses an answer that does not match the contract", async () => {
    const listed = { items: [route("local")], generation: 4 };
    const { routes, sent } = setup(
      Response.json(listed),
      Response.json({ items: [{ name: "local" }], generation: 4 }),
    );
    expect(await routes.routes()).toEqual(listed);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/routes",
      method: "GET",
    });
    await expect(routes.routes()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("creates without a version and replaces with the version read", async () => {
    const stored = route("qwen");
    const { routes, sent } = setup(
      Response.json(stored, { status: 201 }),
      Response.json({ ...stored, version: 3 }),
    );
    const fields = fieldsOf(stored);
    expect(await routes.putRoute("qwen", fields, undefined, "key-1")).toEqual(
      stored,
    );
    expect(sent(0)).toEqual({
      url: "/api/v1/admin/routes/qwen",
      method: "PUT",
      key: "key-1",
      body: fields,
    });
    await routes.putRoute("qwen", fields, 2, "key-2");
    expect(sent(1)).toMatchObject({
      key: "key-2",
      body: { ...fields, version: 2 },
    });
  });

  it("deletes with the version read and names the export and the secrets", async () => {
    const { routes, sent } = setup(
      new Response(null, { status: 204 }),
      Response.json({
        items: [
          {
            ref: "secrets/openai",
            updated_at: "2026-09-26T08:00:00Z",
            version: 1,
            seed: null,
            last_set_by: "admin",
            routes: [],
            rotated_at: "2026-09-25T09:00:00Z",
            rotate_after_days: null,
            rotation_due: false,
            consumers: [],
          },
        ],
        limit: 100,
      }),
    );
    await routes.deleteRoute("local", 3, "key-3");
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/routes/local?version=3",
      method: "DELETE",
      key: "key-3",
    });
    expect(routes.exportUrl()).toBe("/api/v1/admin/routes/export");
    expect(await routes.secretRefs()).toEqual(["secrets/openai"]);
  });
});

// The Egress page on the allow-list admin API (slice 4, ADR-0028; WEB-U-040).
describe("WEB-U-040 the egress admin client", () => {
  it("lists, creates and replaces rules by host with the version read", async () => {
    const rule = {
      host: "api.example.com",
      ports: [443],
      internal: false,
      version: 1,
      seed: null,
      last_set_by: "admin",
      preset: null,
      updated_at: "2026-09-26T08:00:00Z",
    };
    const { egress, sent } = setup(
      Response.json({ disabled: false, items: [rule], limit: 256 }),
      Response.json(rule, { status: 201 }),
      Response.json({ ...rule, version: 2 }),
    );
    expect((await egress.rules()).items).toEqual([rule]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/egress",
      method: "GET",
    });
    await egress.putRule(
      "api.example.com",
      { ports: [443], internal: false },
      undefined,
      "key-1",
    );
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/egress/api.example.com",
      method: "PUT",
      key: "key-1",
      body: { ports: [443], internal: false },
    });
    // An internal server's rule (WEB-U-188) says so in the body.
    await egress.putRule(
      "api.example.com",
      { ports: [443, 8443], internal: true },
      1,
      "key-2",
    );
    expect(sent(2)).toMatchObject({
      body: { ports: [443, 8443], internal: true, version: 1 },
    });
  });

  it("deletes a rule with the version read, and refuses a malformed list", async () => {
    const { egress, sent } = setup(
      new Response(null, { status: 204 }),
      Response.json({ items: [] }),
    );
    await egress.deleteRule("*.example.com", 3, "key-3");
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/egress/*.example.com?version=3",
      method: "DELETE",
      key: "key-3",
    });
    await expect(egress.rules()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});

// The usage and secrets requests moved with the admin schema in (b); they are checked here as well (WEB-U-034/035).
describe("WEB-U-034/035 the usage and secrets admin client", () => {
  it("asks for usage with its query and validates the report", async () => {
    const figures = {
      requests: 1,
      reported: 1,
      estimated: 0,
      prompt_tokens: 10,
      completion_tokens: 2,
      cached_tokens: null,
      reasoning_tokens: null,
    };
    const report = {
      from: "2026-09-01",
      to: "2026-09-26",
      group: "day",
      rows: [],
      totals: figures,
      truncated: false,
    };
    const { admin, sent } = setup(Response.json(report));
    const query = {
      from: "2026-09-01",
      to: "2026-09-26",
      group: "day",
    } as const;
    expect(await admin.usage(query)).toEqual(report);
    expect(sent(0).url).toBe(
      "/api/v1/admin/usage?from=2026-09-01&to=2026-09-26&group=day",
    );
  });

  it("sends a secret's value once, with the version only on a replace, and deletes by name", async () => {
    const stored = {
      ref: "secrets/openai",
      updated_at: "2026-09-26T08:00:00Z",
      version: 1,
      seed: null,
      last_set_by: "admin",
      routes: [],
      rotated_at: "2026-09-25T09:00:00Z",
      rotate_after_days: null,
      rotation_due: false,
      consumers: [],
    };
    const { admin, sent } = setup(
      Response.json(stored, { status: 201 }),
      Response.json({ ...stored, version: 2 }),
      new Response(null, { status: 204 }),
    );
    await admin.putSecret({ name: "openai", value: "fixture-value" }, "key-1");
    expect(sent(0)).toEqual({
      url: "/api/v1/admin/secrets/openai",
      method: "PUT",
      key: "key-1",
      body: { value: "fixture-value" },
    });
    await admin.putSecret(
      { name: "openai", value: "fixture-value-2", version: 1 },
      "key-2",
    );
    expect(sent(1).body).toEqual({ value: "fixture-value-2", version: 1 });
    await admin.deleteSecret("openai", 2, "key-3");
    expect(sent(2)).toMatchObject({
      url: "/api/v1/admin/secrets/openai?version=2",
      method: "DELETE",
    });
  });
});

// Who may use a shared secret, set with the version read; a route or connector allows one by adding
// itself to the secret's current list (WEB-U-054).
describe("WEB-U-054 the secret bindings client", () => {
  const stored = {
    ref: "secrets/shared",
    updated_at: "2026-09-27T08:00:00Z",
    version: 3,
    seed: null,
    last_set_by: "admin",
    routes: [],
    rotated_at: "2026-09-27T08:00:00Z",
    rotate_after_days: null,
    rotation_due: false,
    consumers: [{ kind: "connector", name: "tracker" }],
  };
  it("sends the list with the version read", async () => {
    const { admin, sent } = setup(Response.json(stored));
    await admin.setConsumers(
      "shared",
      [{ kind: "connector", name: "tracker" }],
      2,
    );
    expect(sent(0)).toEqual({
      url: "/api/v1/admin/secrets/shared/consumers",
      method: "PUT",
      key: null,
      body: { consumers: [{ kind: "connector", name: "tracker" }], version: 2 },
    });
  });

  it("allows a route by adding it to the secret's current list, with its current version", async () => {
    const { routes, sent } = setup(
      Response.json({ items: [stored], limit: 4096 }),
      Response.json({ ...stored, version: 4 }),
    );
    await routes.allowSecret("secrets/shared", "local");
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/secrets",
      method: "GET",
    });
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/secrets/shared/consumers",
      method: "PUT",
      key: null,
      body: {
        consumers: [
          { kind: "connector", name: "tracker" },
          { kind: "route", name: "local" },
        ],
        version: 3,
      },
    });
  });

  it("allows a connector the same way", async () => {
    const { connectors, sent } = setup(
      Response.json({ items: [stored], limit: 4096 }),
      Response.json({ ...stored, version: 4 }),
    );
    await connectors.allowSecret("secrets/shared", "wiki");
    expect(sent(1).body).toEqual({
      consumers: [
        { kind: "connector", name: "tracker" },
        { kind: "connector", name: "wiki" },
      ],
      version: 3,
    });
  });
});

// An administrator's view of a shared secret's reads, paged (WEB-U-053).
describe("WEB-U-053 the secret reads admin client", () => {
  it("pages with limit and before", async () => {
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
    const { admin, sent } = setup(Response.json(page), Response.json(page));
    expect(await admin.secretReads("local")).toEqual(page);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/secrets/local/reads?limit=50",
      method: "GET",
    });
    await admin.secretReads("local", "0195d8c0-0000-7000-8000-000000000e01");
    expect(sent(1).url).toBe(
      "/api/v1/admin/secrets/local/reads?limit=50&before=0195d8c0-0000-7000-8000-000000000e01",
    );
  });
});

// A secret's rotation reminder, set or cleared with the version read (WEB-U-052).
describe("WEB-U-052 the rotation reminder client", () => {
  it("sends the days, or null, with the version read, and parses the secret", async () => {
    const stored = {
      ref: "secrets/openai",
      updated_at: "2026-09-27T08:00:00Z",
      version: 4,
      seed: null,
      last_set_by: "admin",
      routes: [],
      rotated_at: "2026-06-01T09:00:00Z",
      rotate_after_days: 90,
      rotation_due: true,
      consumers: [],
    };
    const { admin, sent } = setup(
      Response.json(stored),
      Response.json({ ...stored, version: 5, rotate_after_days: null }),
    );
    expect(await admin.setReminder("openai", 90, 3)).toEqual(stored);
    expect(sent(0)).toEqual({
      url: "/api/v1/admin/secrets/openai/reminder",
      method: "PUT",
      key: null,
      body: { rotate_after_days: 90, version: 3 },
    });
    await admin.setReminder("openai", null, 4);
    expect(sent(1).body).toEqual({ rotate_after_days: null, version: 4 });
  });
});

// Paste key on the route form creates the secret only (never a replace) and answers its reference (WEB-U-056).
describe("WEB-U-056 the route form's Paste key", () => {
  it("creates the secret without a version and answers its reference", async () => {
    const { routes, sent } = setup(
      Response.json(
        {
          ref: "secrets/local-key",
          updated_at: "2026-09-27T08:00:00Z",
          version: 1,
          seed: null,
          last_set_by: "admin",
          routes: [],
          rotated_at: "2026-09-25T09:00:00Z",
          rotate_after_days: null,
          rotation_due: false,
          consumers: [],
        },
        { status: 201 },
      ),
    );
    expect(
      await routes.createSecret("local-key", "fixture-value", "key-1"),
    ).toBe("secrets/local-key");
    expect(sent(0)).toEqual({
      url: "/api/v1/admin/secrets/local-key",
      method: "PUT",
      key: "key-1",
      body: { value: "fixture-value" },
    });
  });
});

// The Limits page on the admin limits API (slice 5, ADR-0028; WEB-U-041).
describe("WEB-U-041 the limits admin client", () => {
  const item = {
    key: "admission.slots",
    value: 8,
    default: 8,
    min: 1,
    max: 64,
    unit: "sandboxes",
    applies: "next_admission",
    version: 1,
    seed: "current",
    last_set_by: "seed",
    updated_at: "2026-09-26T08:00:00Z",
    clamped_from: null,
    below_floor: false,
  } as const;

  it("lists limits, sets one with the version read and resets another", async () => {
    const { limits, sent } = setup(
      Response.json({ items: [item], generation: 2, ...SIZES }),
      Response.json({ ...item, value: 6, version: 2 }),
      Response.json({ ...item, version: 3 }),
    );
    expect((await limits.limits()).items).toEqual([item]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/limits",
      method: "GET",
    });
    await limits.putLimit("admission.slots", 6, 1, "key-1");
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/limits/admission.slots",
      method: "PUT",
      key: "key-1",
      body: { value: 6, version: 1 },
    });
    await limits.resetLimit("admission.slots", 2, "key-2");
    expect(sent(2)).toMatchObject({
      url: "/api/v1/admin/limits/admission.slots?version=2",
      method: "DELETE",
      key: "key-2",
    });
  });

  it("refuses a limit answer that does not match the contract", async () => {
    const { limits } = setup(
      Response.json({
        items: [{ ...item, value: -1 }],
        generation: 2,
        ...SIZES,
      }),
    );
    await expect(limits.limits()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});

// ADR-0034 S6: the helpers' two settings that aren't numbers, read and written with the version read.
describe("WEB-U-227 the helpers' settings client", () => {
  it("reads the settings and writes both with the version read", async () => {
    const stored = { member_route: null, acceptance_check: null, version: 1 };
    const { limits, sent } = setup(
      Response.json(stored),
      Response.json({
        member_route: "local",
        acceptance_check: "cargo test",
        version: 2,
      }),
    );
    expect(await limits.agentTeams()).toEqual(stored);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/agent-teams",
      method: "GET",
    });
    expect(
      await limits.putAgentTeams(
        { member_route: "local", acceptance_check: "cargo test", version: 1 },
        "key-1",
      ),
    ).toMatchObject({ member_route: "local", version: 2 });
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/agent-teams",
      method: "PUT",
      key: "key-1",
      body: {
        member_route: "local",
        acceptance_check: "cargo test",
        version: 1,
      },
    });
  });

  it("refuses an answer that does not match the contract", async () => {
    const { limits } = setup(Response.json({ member_route: 3, version: 1 }));
    await expect(limits.agentTeams()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});

// The Models page's route check (WEB-U-039): one POST with an Idempotency-Key; a failed check is a result.
describe("WEB-U-039 the route check client", () => {
  it("posts a check with its key and returns the observation, refusing a malformed answer", async () => {
    const failed = probeResult("failed", {
      ok: false,
      failure: { code: "unreachable", status: null, excerpt: null },
    });
    const { routes, sent } = setup(
      Response.json({ probe: failed }),
      Response.json({ probe: { state: "unknown" } }),
    );
    expect(await routes.probe("local", "key-9")).toEqual(failed);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/routes/local/probe",
      method: "POST",
      key: "key-9",
    });
    await expect(routes.probe("local", "key-10")).rejects.toMatchObject({
      code: "invalid_response",
    });
  });
});

// The Connectors page on the gateway's registry API (ADR-0031; WEB-U-043).
describe("WEB-U-043 the connectors admin client", () => {
  it("lists, creates without a version and replaces with the version read", async () => {
    const stored = connector("tracker");
    const { connectors, sent } = setup(
      Response.json({ items: [stored], generation: 2 }),
      Response.json(stored, { status: 201 }),
      Response.json({ ...stored, version: 2 }),
      Response.json({ items: [{ name: "tracker" }], generation: 2 }),
    );
    expect((await connectors.connectors()).items).toEqual([stored]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/connectors",
      method: "GET",
    });
    const fields = { url: stored.url, class: "read" as const };
    await connectors.putConnector("tracker", fields, undefined, "key-1");
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/connectors/tracker",
      method: "PUT",
      key: "key-1",
      body: fields,
    });
    await connectors.putConnector("tracker", fields, 1, "key-2");
    expect(sent(2)).toMatchObject({ body: { ...fields, version: 1 } });
    await expect(connectors.connectors()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("deletes with the version read and offers the shared secrets' refs", async () => {
    const { connectors, sent } = setup(
      new Response(null, { status: 204 }),
      Response.json({
        limit: 256,
        items: [
          {
            ref: "secrets/tracker",
            updated_at: "2026-09-26T08:00:00Z",
            version: 1,
            seed: null,
            last_set_by: "admin",
            routes: [],
            rotated_at: "2026-09-25T09:00:00Z",
            rotate_after_days: null,
            rotation_due: false,
            consumers: [],
          },
        ],
      }),
    );
    await connectors.deleteConnector("tracker", 3, "key-3");
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/connectors/tracker?version=3",
      method: "DELETE",
      key: "key-3",
    });
    expect(await connectors.secretRefs()).toEqual(["secrets/tracker"]);
  });
});

// The Decision engines page on the gateway's engines admin API (ADR-0013 point 6; WEB-U-074).
describe("WEB-U-074 the decision engines admin client", () => {
  it("lists, creates without a version, replaces with it, deletes with it, and allows a secret for an engine", async () => {
    const stored = engine("router");
    const secret = {
      ref: "secrets/engine-key",
      updated_at: "2026-09-29T00:00:00Z",
      version: 2,
      seed: null,
      last_set_by: "admin",
      routes: [],
      rotated_at: "2026-09-29T00:00:00Z",
      rotate_after_days: null,
      rotation_due: false,
      consumers: [],
    };
    const { engines, sent } = setup(
      Response.json({ items: [stored], generation: 5 }),
      Response.json(stored, { status: 201 }),
      Response.json({ ...stored, version: 2 }),
      new Response(null, { status: 204 }),
      Response.json({ items: [secret], limit: 256 }),
      Response.json({ items: [secret], limit: 256 }),
      Response.json({ ...secret, version: 3 }),
    );
    expect((await engines.engines()).items).toEqual([stored]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/decision/engines",
      method: "GET",
    });
    const fields = { url: stored.url, exposure: "calls" as const };
    await engines.putEngine("router", fields, undefined, "key-1");
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/decision/engines/router",
      method: "PUT",
      key: "key-1",
      body: fields,
    });
    await engines.putEngine("router", fields, 1, "key-2");
    expect(sent(2)).toMatchObject({ body: { ...fields, version: 1 } });
    await engines.deleteEngine("router", 2, "key-3");
    expect(sent(3)).toMatchObject({
      url: "/api/v1/admin/decision/engines/router?version=2",
      method: "DELETE",
      key: "key-3",
    });
    expect(await engines.secretRefs()).toEqual(["secrets/engine-key"]);
    await engines.allowSecret("secrets/engine-key", "router");
    expect(sent(6).body).toEqual({
      consumers: [{ kind: "engine", name: "router" }],
      version: 2,
    });
  });
});

// The Environments page on the gateway's templates admin API (slice 4, ADR-0032; WEB-U-047).
describe("WEB-U-047 the templates admin client", () => {
  it("lists, creates without a version, replaces with it, and refuses a malformed list", async () => {
    const stored = environment("data");
    const { templates, sent } = setup(
      Response.json({ items: [stored], generation: 3, ...SIZES }),
      Response.json(stored, { status: 201 }),
      Response.json({ ...stored, version: 2 }),
      Response.json({ items: [{ name: "data" }], generation: 3, ...SIZES }),
    );
    expect((await templates.environments()).items).toEqual([stored]);
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/templates",
      method: "GET",
    });
    const fields = {
      display_name: "Data",
      image: stored.image,
      capabilities: ["python" as const],
    };
    await templates.putEnvironment("data", fields, undefined, "key-1");
    expect(sent(1)).toEqual({
      url: "/api/v1/admin/templates/data",
      method: "PUT",
      key: "key-1",
      body: fields,
    });
    await templates.putEnvironment("data", fields, 1, "key-2");
    expect(sent(2)).toMatchObject({ body: { ...fields, version: 1 } });
    await expect(templates.environments()).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  it("deletes with the version read, moves the default, and asks for a check", async () => {
    const { templates, sent } = setup(
      new Response(null, { status: 204 }),
      Response.json(environment("minimal", { is_default: true, version: 4 })),
      new Response(null, { status: 202 }),
    );
    await templates.deleteEnvironment("data", 3, "key-3");
    expect(sent(0)).toMatchObject({
      url: "/api/v1/admin/templates/data?version=3",
      method: "DELETE",
      key: "key-3",
    });
    expect(
      (await templates.makeDefault("minimal", 3, "key-4")).is_default,
    ).toBe(true);
    expect(sent(1)).toMatchObject({
      url: "/api/v1/admin/templates/minimal/default",
      method: "POST",
      key: "key-4",
      body: { version: 3 },
    });
    await templates.check("minimal");
    expect(sent(2)).toMatchObject({
      url: "/api/v1/admin/templates/minimal/check",
      method: "POST",
    });
  });
});
