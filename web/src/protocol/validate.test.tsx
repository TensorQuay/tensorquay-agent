import { readFileSync } from "node:fs";
import { join } from "node:path";

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId, transcriptFrames } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { TranscriptView } from "../transcript/TranscriptView";

import adminSchema from "./generated/admin.schema.json";
import apiSchema from "./generated/api.schema.json";
import protocolSchema from "./generated/protocol.schema.json";
import { problemKinds } from "./generated/validators.core";
import { knownFormat, parseApi, parseFrame } from "./validate";
import { parseAdminApi } from "./validate-admin";

describe("protocol frame validation", () => {
  it("rejects malformed, unknown, incomplete and unsafe frames", () => {
    for (const source of [
      "{",
      "{}",
      '{"type":"event","seq":1}',
      JSON.stringify({
        type: "error",
        code: "busy",
        message: "wait",
        retryable: true,
        seq: Number.MAX_SAFE_INTEGER + 1,
      }),
      " ".repeat(262145),
    ]) {
      expect(() => parseFrame(source)).toThrow();
    }
  });

  // TOOL-U-036's tolerant decode, on the web: a runtime newer than this page may fail a run with a code the page's schema
  // doesn't list yet. Dropping the frame left the page on "Working…" forever (a cached page against an upgraded
  // gateway would look hung), so a known frame whose only fault is a newer code is accepted with that code, and the
  // page says "The run stopped" with the code after it. Anything else wrong is still refused.
  describe("a failure code newer than this page", () => {
    // Built as the wire has it: the typed helper only takes codes this page knows.
    const failed = (code: unknown, extra: object = {}) =>
      JSON.stringify({
        type: "event",
        seq: 3,
        run_id: runId,
        step: 1,
        ts: "2026-01-01T00:00:00.300Z",
        event: {
          type: "run.failed",
          run_id: runId,
          reason: "diagnostic",
          code,
          ...extra,
        },
      });
    it("accepts it in run.failed and an error frame, keeping the code", () => {
      const parsed = parseFrame(failed("brand_new_code"));
      expect(parsed.type === "event" && parsed.event).toMatchObject({
        type: "run.failed",
        code: "brand_new_code",
      });
      const error = parseFrame(
        JSON.stringify({
          type: "error",
          code: "brand_new_code",
          message: "wait",
          retryable: true,
        }),
      );
      expect(error).toMatchObject({ type: "error", code: "brand_new_code" });
    });

    it("still refuses a code that isn't a bare token, or any other fault", () => {
      for (const source of [
        failed("Not A Code"),
        failed("x".repeat(65)),
        failed(42),
        failed("brand_new_code", { run_id: 7 }),
      ])
        expect(() => parseFrame(source)).toThrow();
    });

    it("ends the run on the page: The run stopped, what to do, the code after", () => {
      const transcript = [
        frame(1, { type: "message.user", text: "Do it", attachments: [] }),
        frame(2, { type: "run.started", run_id: runId }),
        parseFrame(failed("brand_new_code")),
      ].reduce(
        (state, value) =>
          value.type === "event" ? apply(state, value) : state,
        emptyTranscript(),
      );
      expect(transcript.run.status).toBe("failed");
      const { container } = render(<TranscriptView transcript={transcript} />);
      expect(container.querySelector(".turn-outcome")).toHaveTextContent(
        "The run stopped. Send a new message to retry after checking the failed activity. Code: brand_new_code",
      );
      expect(container).not.toHaveTextContent("diagnostic");
    });
  });

  // WEB-U-417: the same for a retry's problem. A runtime newer than this page may name a problem kind the page's
  // schema doesn't list yet (the runtime reads an unknown kind as `other`, PROTO-U-044). A tab still running the old
  // app after an upgrade must not throw on it, so the retry is read with the problem as `other`. Anything else wrong is
  // still refused.
  describe("WEB-U-417 a retry problem newer than this page", () => {
    const retry = (fallback_problem: unknown, extra: object = {}) =>
      JSON.stringify({
        type: "event",
        seq: 4,
        run_id: runId,
        step: 1,
        ts: "2026-01-01T00:00:00.400Z",
        event: {
          type: "provider.retry",
          attempt: 1,
          delay_ms: 0,
          reason: "fallback_parse: malformed",
          fallback_problem,
          ...extra,
        },
      });

    it("reads an unknown kind as other and keeps the retry", () => {
      const parsed = parseFrame(retry({ kind: "brand_new_problem", depth: 3 }));
      expect(parsed.type === "event" && parsed.event).toEqual({
        type: "provider.retry",
        attempt: 1,
        delay_ms: 0,
        reason: "fallback_parse: malformed",
        fallback_problem: { kind: "other" },
      });
    });

    it("still refuses a known kind with bad fields, a kind that isn't a bare token, or any other fault", () => {
      for (const source of [
        retry({ kind: "wrong_type", tool: 5 }),
        retry({ kind: "Not A Kind" }),
        retry({ kind: "x".repeat(65) }),
        retry({ kind: 7 }),
        retry({ depth: 3 }),
        retry({ kind: "brand_new_problem" }, { attempt: -1 }),
      ])
        expect(() => parseFrame(source)).toThrow();
    });

    it("knows exactly the schema's problem kinds", () => {
      const schemaKinds = (
        protocolSchema.definitions.FallbackProblem.oneOf as {
          properties: { kind: { const: string } };
        }[]
      )
        .map((branch) => branch.properties.kind.const)
        .sort();
      expect([...problemKinds]).toEqual(schemaKinds);
    });
  });

  it("validates every browser fixture against the canonical protocol schema", () => {
    for (const frame of transcriptFrames)
      expect(parseFrame(JSON.stringify(frame))).toEqual(frame);
  });

  it("preserves allowed additive fields without turning them into events", () => {
    expect(
      parseFrame(
        JSON.stringify({
          type: "error",
          code: "busy",
          message: "wait",
          retryable: true,
          future: "ignored",
        }),
      ),
    ).toMatchObject({ type: "error", message: "wait" });
  });

  it("WEB-U-003 validates a structured destination without accepting URL syntax", () => {
    const error = {
      type: "error",
      code: "egress_denied",
      message: "Internal detail",
      retryable: false,
    };
    for (const host of ["example.invalid", "192.0.2.1", "2001:db8::1"])
      expect(
        parseFrame(JSON.stringify({ ...error, target: { host, port: 443 } })),
      ).toMatchObject({ target: { host, port: 443 } });
    for (const target of [
      { host: "2001:db8::1::2", port: 443 },
      { host: "[2001:db8::1]", port: 443 },
      { host: "example.invalid:443", port: 443 },
      { host: "example.invalid", port: 0 },
      { host: "example.invalid", port: 65536 },
    ])
      expect(() => parseFrame(JSON.stringify({ ...error, target }))).toThrow();
  });
});

// Task 069 / ARCHITECTURE §15: consumers ignore unknown fields and stay strict about known ones. The gateway may add
// fields (as 066a added Conversation.workspace_id) without breaking a web built before them.
// Task 090: the admin usage report brought the first `format: date`, which the validator did not know, so every answer
// failed as invalid. Every definition must compile; data errors are the only way a valid schema may refuse (WEB-U-036).
describe("WEB-U-036 generated schema formats", () => {
  it("knows every format the generated schemas use", () => {
    const formats = new Set<string>();
    const collect = (node: unknown) => {
      if (Array.isArray(node)) node.forEach(collect);
      else if (node && typeof node === "object")
        for (const [key, value] of Object.entries(node))
          if (key === "format" && typeof value === "string") formats.add(value);
          else collect(value);
    };
    collect(apiSchema);
    collect(adminSchema);
    collect(protocolSchema);
    expect(formats.size).toBeGreaterThan(0);
    expect([...formats].filter((format) => !knownFormat(format))).toEqual([]);
  });

  // The e2e fake front validates its frames with its own Ajv (e2e/front/fixtures.mjs): it must know the same formats,
  // or every fake-backed browser test fails to start (revision 7's int64 did exactly that).
  it("gives the fake front's validator every format the generated schemas use", () => {
    const formats = new Set<string>();
    const collect = (node: unknown) => {
      if (Array.isArray(node)) node.forEach(collect);
      else if (node && typeof node === "object")
        for (const [key, value] of Object.entries(node))
          if (key === "format" && typeof value === "string") formats.add(value);
          else collect(value);
    };
    // The fake front compiles the protocol schema (its frames), not the API schema.
    collect(protocolSchema);
    const front = readFileSync(
      join(process.cwd(), "e2e", "front", "fixtures.mjs"),
      "utf8",
    );
    expect(
      [...formats].filter((format) => !front.includes(`"${format}"`)),
    ).toEqual([]);
  });

  it("accepts calendar dates in the usage report and refuses impossible ones", () => {
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
      from: "2026-02-01",
      to: "2026-02-28",
      group: "day",
      rows: [{ key: "2026-02-28", label: null, ...figures }],
      totals: figures,
      truncated: false,
    };
    expect(parseAdminApi(report, "AdminUsageReport")).toEqual(report);
    for (const to of ["2026-02-30", "2026-2-28", "2026-02-28T00:00:00Z"])
      expect(() =>
        parseAdminApi({ ...report, to }, "AdminUsageReport"),
      ).toThrow("Invalid API response");
  });

  // The refusal names the definition, so a contract failure can say what it was (the transport adds the route).
  it("names the definition it refused", () => {
    expect(() => parseApi({}, "User")).toThrow(
      "Invalid API response: User (urn:tqa:api)",
    );
  });

  it("accepts a connector's absolute URL and refuses one that is not a URI", () => {
    const connector = {
      name: "tracker",
      url: "https://mcp.example.com/v1/mcp?team=a",
      auth: { scheme: "none", required: false },
      class: "read",
      call_timeout_seconds: 60,
      enabled: true,
      enabled_by_default: false,
      display_name: null,
      key_missing: false,
      version: 1,
      seed: null,
      last_set_by: "admin",
      updated_at: "2026-01-01T00:00:00Z",
    };
    expect(parseAdminApi(connector, "AdminConnector")).toEqual(connector);
    for (const url of [
      "mcp.example.com/v1/mcp",
      "https://mcp.example .com/mcp",
      "https:",
      "",
      "https://mcp.example.com/\u0000",
    ])
      expect(() =>
        parseAdminApi({ ...connector, url }, "AdminConnector"),
      ).toThrow("Invalid API response");
  });
});

// The conversation page carries no administration schema (WEB-U-014 budget); the admin pages load it on their own.
describe("WEB-U-014 administration schemas", () => {
  const refs = (node: unknown): string[] =>
    Array.isArray(node)
      ? node.flatMap(refs)
      : node && typeof node === "object"
        ? Object.entries(node).flatMap(([key, value]) =>
            key === "$ref" && typeof value === "string" ? [value] : refs(value),
          )
        : [];

  it("keeps every administration definition out of the shared API schema", () => {
    const admin = Object.keys(adminSchema.definitions);
    const shared = Object.keys(apiSchema.definitions);
    expect(admin).toEqual(
      expect.arrayContaining(["AdminRoutes", "AdminUsageReport", "RouteAuth"]),
    );
    expect(admin.filter((name) => shared.includes(name))).toEqual([]);
    expect(shared.filter((name) => name.startsWith("Admin"))).toEqual([]);
    const unresolved = refs(adminSchema).filter((ref) => {
      const [base, name] = ref.split("#/definitions/");
      return !(
        base === "" ? admin : base === "urn:tqa:api" ? shared : []
      ).includes(name ?? "");
    });
    expect(unresolved).toEqual([]);
    expect(
      refs(apiSchema).filter((ref) =>
        admin.some((name) => ref.endsWith(`/${name}`)),
      ),
    ).toEqual([]);
  });

  it("validates an administration answer only through the admin validator", () => {
    const secrets = { items: [], limit: 100 };
    expect(parseAdminApi(secrets, "AdminSecrets")).toEqual(secrets);
    expect(() =>
      parseAdminApi({ items: "none", limit: 100 }, "AdminSecrets"),
    ).toThrow("Invalid API response");
    // The shared validators don't carry the administration definitions at all.
    expect(() => parseApi(secrets, "AdminSecrets")).toThrow(
      "No validator for urn:tqa:api AdminSecrets",
    );
  });
});

describe("forward-compatible server input", () => {
  const conversation = {
    id: "0195d8c0-0000-7000-8000-000000000001",
    owner_id: "0195d8c0-0000-7000-8000-000000000002",
    title: "Workspace review",
    model_route: "local",
    template: "workstation",
    pinned: false,
    title_source: "message",
    backend: "docker",
    state: "active",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    last_activity_at: "2026-01-01T00:00:00Z",
    workspace_id: "0195d8c0-0000-7000-8000-000000000003",
    mcp_servers: [],
    connectors: [],
    team: null,
  };

  it("accepts an API response with a field it does not know", () => {
    const later = { ...conversation, archived_at: null, labels: ["x"] };
    expect(parseApi(later, "Conversation")).toMatchObject(conversation);
    const account = {
      id: conversation.owner_id,
      email: "member@example.invalid",
      role: "member",
      disabled: false,
      created_at: conversation.created_at,
      last_active_at: conversation.created_at,
      avatar_url: "https://example.invalid/a.png",
    };
    expect(parseApi({ items: [account] }, "Accounts").items[0]).toMatchObject({
      email: "member@example.invalid",
    });
    // Nested objects too: an item inside a page.
    expect(
      parseApi(
        { items: [later], next_cursor: null, total: 1 },
        "ConversationPage",
      ).items,
    ).toHaveLength(1);
  });

  it("still rejects a known field of the wrong type or out of bounds", () => {
    for (const broken of [
      { ...conversation, title: 5 },
      { ...conversation, pinned: "yes" },
      { ...conversation, backend: "podman" },
      { ...conversation, title: "" },
      { ...conversation, created_at: "yesterday" },
    ])
      expect(() => parseApi(broken, "Conversation")).toThrow();
  });

  it("still rejects a missing required field", () => {
    const missing: Partial<typeof conversation> = { ...conversation };
    delete missing.id;
    expect(() => parseApi(missing, "Conversation")).toThrow();
    expect(() =>
      parseApi({ items: [conversation] }, "ConversationPage"),
    ).toThrow();
  });

  it("accepts additive fields inside nested protocol objects", () => {
    const first = transcriptFrames.find((value) => value.type === "event");
    if (first?.type !== "event")
      throw new Error("The fixture starts with an event.");
    const frame = { ...first, event: { ...first.event, later: "ignored" } };
    expect(parseFrame(JSON.stringify(frame))).toMatchObject({
      type: "event",
      event: { type: first.event.type },
    });
  });
});

// Task 069b / ARCHITECTURE §15: a frame or event kind the web does not know is accepted as an opaque, sequenced marker;
// known kinds stay strictly validated.
describe("unknown frame and event kinds", () => {
  const envelope = {
    type: "event",
    seq: 7,
    run_id: "run-example",
    step: 2,
    ts: "2026-01-01T00:00:00Z",
  };

  it("keeps an unknown event kind as an ignored marker with its sequence", () => {
    expect(
      parseFrame(
        JSON.stringify({
          ...envelope,
          event: {
            type: "memory.pruned",
            summary: "Earlier steps",
            dropped: 12,
          },
        }),
      ),
    ).toEqual({ type: "ignored", seq: 7 });
  });

  it("keeps an unknown frame kind as an ignored marker, with a sequence only when it is valid", () => {
    expect(parseFrame('{"type":"future"}')).toEqual({ type: "ignored" });
    expect(parseFrame('{"type":"future","seq":4}')).toEqual({
      type: "ignored",
      seq: 4,
    });
    expect(() => parseFrame('{"type":"future","seq":-1}')).toThrow();
  });

  it("still rejects an unknown event whose envelope is broken", () => {
    for (const broken of [
      { ...envelope, seq: -1 },
      { ...envelope, seq: 1.5 },
      { ...envelope, ts: 5 },
      { ...envelope, event: { summary: "no type" } },
      { ...envelope, event: "memory.pruned" },
    ])
      expect(() =>
        parseFrame(
          JSON.stringify({
            event: { type: "memory.pruned" },
            ...broken,
          }),
        ),
      ).toThrow();
  });

  it("still rejects a known event kind with bad fields", () => {
    expect(() =>
      parseFrame(
        JSON.stringify({ ...envelope, event: { type: "text.delta", text: 5 } }),
      ),
    ).toThrow();
    expect(() =>
      parseFrame(JSON.stringify({ type: "heartbeat", run_id: 5 })),
    ).toThrow();
  });
  it("validates the compaction event as a known kind", () => {
    const summary = {
      goal: "Ship the report",
      done: [],
      pending: [],
      decisions: [],
      key_paths: [],
      open_errors: [],
    };
    const compaction = {
      type: "compaction",
      through_seq: 6,
      source: "fallback",
      version: 1,
      summary,
    };
    expect(
      parseFrame(JSON.stringify({ ...envelope, event: compaction })),
    ).toMatchObject({ seq: 7, event: { type: "compaction", through_seq: 6 } });
    for (const bad of [
      { ...compaction, source: "someone" },
      { ...compaction, summary: { ...summary, goal: 5 } },
      { ...compaction, through_seq: -1 },
    ])
      expect(() =>
        parseFrame(JSON.stringify({ ...envelope, event: bad })),
      ).toThrow();
  });
});
