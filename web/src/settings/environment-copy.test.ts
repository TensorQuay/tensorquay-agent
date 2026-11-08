import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { DIGEST, environment } from "../test/templates-admin";

import {
  CHECK_REASONS,
  checkState,
  environmentFailure,
  environmentId,
  environmentProblems,
  fromDraft,
  inUseParts,
  reasonKey,
  toDraft,
} from "./environment-copy";

/** The closed refusal codes of the image check (ADR-0032). */
const CONTRACT_REASONS = [
  "image_not_found",
  "not_derived",
  "image_user",
  "image_entrypoint",
  "image_volumes",
  "image_env_unset",
  "image_secret_file",
  "image_scan_limit",
  "runtime_label",
  "runtime_too_old",
  "runtime_missing",
  "runtime_modified",
  "runtime_unreadable",
  "rootfs_missing",
  "rootfs_outdated",
  "rootfs_mismatch",
  "rootfs_foreign",
  "rootfs_ambiguous",
];
const refusal = (
  status: number,
  code: string,
  details: { field: string; reason: string }[] = [],
) => new ApiError(status, code, "x", undefined, details);

// The Environments form and list rules: what an administrator types, and what each check result means (WEB-U-047).
describe("WEB-U-047 environment rules", () => {
  it("round-trips the fields and sends empty resources as the install's limit", () => {
    const item = environment("data", {
      display_name: "Data science",
      capabilities: ["shell", "python"],
      cpus: 1.5,
      memory_mib: 8192,
      pids: 512,
      workspace_mib: 20480,
      enabled: false,
    });
    // Memory and disk are read and typed in GiB, CPUs in halves.
    expect(toDraft(item)).toMatchObject({
      cpus: "1.5",
      memory: "8",
      pids: "512",
      disk: "20",
    });
    expect(fromDraft(toDraft(item), false)).toEqual({
      problems: {},
      fields: {
        display_name: "Data science",
        description: null,
        image: item.image,
        capabilities: ["shell", "python"],
        cpus: 1.5,
        memory_mib: 8192,
        pids: 512,
        workspace_mib: 20480,
        enabled: false,
      },
    });
    const plain = fromDraft(
      // Any name but the reserved "new" (GW-U-093).
      { ...toDraft(null), name: "plain", displayName: "Plain", image: DIGEST },
      true,
    );
    expect(plain.problems).toEqual({});
    expect(plain.fields).toMatchObject({
      capabilities: [],
      cpus: null,
      memory_mib: null,
      pids: null,
      workspace_mib: null,
      enabled: true,
    });
  });

  it("refuses at the field what the gateway would refuse", () => {
    const check = (change: Partial<ReturnType<typeof toDraft>>) =>
      fromDraft({ ...toDraft(environment("data")), ...change }, true).problems;
    for (const name of ["Data", "1data", "", "a".repeat(65)])
      expect(check({ name })).toEqual({ name: "environments.nameInvalid" });
    for (const image of [
      "tqa-runtime:latest",
      "sha256:abc",
      `name@sha256:${"a".repeat(63)}`,
      `x${"y".repeat(512)}@${DIGEST}`,
    ])
      expect(check({ image })).toEqual({
        image: "environments.imageNotPinned",
      });
    expect(check({ displayName: "" })).toEqual({
      displayName: "environments.displayNameInvalid",
    });
    expect(check({ displayName: "Two\nlines" })).toEqual({
      displayName: "environments.displayNameInvalid",
    });
    // CPUs in halves from 0.5; memory in GiB from 0.25 (256 MiB); processes whole from 64; disk in GiB from 16 MiB.
    for (const cpus of ["0", "0.3", "1.25", "65", "x"])
      expect(check({ cpus })).toEqual({ cpus: "environments.cpusInvalid" });
    for (const cpus of ["0.5", "2.5", "64"])
      expect(check({ cpus })).toEqual({});
    for (const memory of ["0.2", "1025", "x"])
      expect(check({ memory })).toEqual({
        memory: "environments.memoryInvalid",
      });
    for (const memory of ["0.25", "1.5"]) expect(check({ memory })).toEqual({});
    for (const pids of ["63", "65537", "1.5"])
      expect(check({ pids })).toEqual({ pids: "environments.pidsInvalid" });
    for (const disk of ["0.01", "1025"])
      expect(check({ disk })).toEqual({ disk: "environments.diskInvalid" });
    // Never above the installation's ceiling.
    const ceiling = {
      cpus: 8,
      memory_mib: 16384,
      pids: 4096,
      workspace_mib: 102400,
    };
    const capped = (change: Partial<ReturnType<typeof toDraft>>) =>
      fromDraft({ ...toDraft(environment("data")), ...change }, true, ceiling)
        .problems;
    expect(capped({ cpus: "8.5" })).toEqual({
      cpus: "environments.cpusInvalid",
    });
    expect(capped({ memory: "17" })).toEqual({
      memory: "environments.memoryInvalid",
    });
    expect(capped({ pids: "4097" })).toEqual({
      pids: "environments.pidsInvalid",
    });
    expect(capped({ disk: "101" })).toEqual({
      disk: "environments.diskInvalid",
    });
    expect(
      capped({ cpus: "8", memory: "16", pids: "4096", disk: "100" }),
    ).toEqual({});
    expect(
      fromDraft({ ...toDraft(environment("data")), name: "Bad" }, false)
        .problems,
    ).toEqual({});
  });

  // The GiB text is rounded for reading, so an unchanged field sends the stored MiB exactly; otherwise saving any field
  // would change memory or disk by 1 MiB and send the environment back to its check.
  it("rounds typed GiB to whole MiB once: 320 MiB reads 0.313 and stays 320 unless typed", () => {
    const item = environment("data", { memory_mib: 320 });
    expect(toDraft(item).memory).toBe("0.313");
    expect(
      fromDraft(toDraft(item), false, undefined, item).fields.memory_mib,
    ).toBe(320);
    // Typed, the GiB rounds to whole MiB exactly once: 0.3125 GiB is 320 MiB, 0.3134 GiB rounds to 321 MiB.
    const typed = (memory: string) =>
      fromDraft({ ...toDraft(item), memory }, false, undefined, item).fields
        .memory_mib;
    expect(typed("0.3125")).toBe(320);
    expect(typed("0.3134")).toBe(321);
  });

  it("sends a stored size unchanged when its field wasn't edited, for every MiB value", () => {
    for (let mib = 256; mib <= 16384; mib += 1) {
      const item = environment("data", { memory_mib: mib, workspace_mib: mib });
      const { fields, problems } = fromDraft(
        toDraft(item),
        false,
        undefined,
        item,
      );
      expect(problems).toEqual({});
      expect([fields.memory_mib, fields.workspace_mib]).toEqual([mib, mib]);
    }
  });

  // A size the ceiling has since lowered is kept as stored when not edited, so the environment (even the default, whose
  // size is locked) can still be saved; an edited one must fit the ceiling.
  it("lets an unchanged size above the ceiling through, and refuses an edited one", () => {
    const ceiling = {
      cpus: 8,
      memory_mib: 16384,
      pids: 4096,
      workspace_mib: 102400,
    };
    const item = environment("big", {
      cpus: 12,
      memory_mib: 32768,
      pids: 8192,
      clamped: ["cpus", "memory_mib", "pids"],
    });
    const kept = fromDraft(toDraft(item), false, ceiling, item);
    expect(kept.problems).toEqual({});
    expect(kept.fields).toMatchObject({
      cpus: 12,
      memory_mib: 32768,
      pids: 8192,
    });
    expect(
      fromDraft({ ...toDraft(item), cpus: "11" }, false, ceiling, item)
        .problems,
    ).toEqual({ cpus: "environments.cpusInvalid" });
  });

  it("has a sentence for every check refusal the contract names, and a plain one for a new code", () => {
    expect(Object.keys(CHECK_REASONS).sort()).toEqual(
      [...CONTRACT_REASONS].sort(),
    );
    expect(new Set(Object.values(CHECK_REASONS)).size).toBe(
      CONTRACT_REASONS.length,
    );
    expect(reasonKey("renamed_later")).toBe("environments.reasonUnknown");
  });

  it("reads the check state as the list shows it", () => {
    expect(checkState(environment("a"))).toBe("verified");
    expect(
      checkState(
        environment("a", {
          check: {
            state: "refused",
            reason: "not_derived",
            checked_version: 1,
            secret_files: null,
            scan_limit: null,
          },
        }),
      ),
    ).toBe("refused");
    expect(
      checkState(
        environment("a", {
          check: {
            state: "unchecked",
            reason: null,
            checked_version: null,
            secret_files: null,
            scan_limit: null,
          },
        }),
      ),
    ).toBe("checking");
    expect(
      checkState(
        environment("a", {
          version: 3,
          check: {
            state: "unchecked",
            reason: null,
            checked_version: 3,
            secret_files: null,
            scan_limit: null,
          },
        }),
      ),
    ).toBe("unchecked");
  });

  it("names the gateway's refusals at their fields, and the rest in plain words", () => {
    expect(
      environmentProblems(
        refusal(400, "invalid_request", [
          { field: "image", reason: "image_not_pinned" },
        ]),
      ),
    ).toEqual({ image: "environments.imageNotPinned" });
    expect(
      environmentProblems(
        refusal(400, "invalid_request", [
          { field: "memory_mib", reason: "limit_out_of_range" },
        ]),
      ),
    ).toEqual({ memory: "environments.memoryInvalid" });
    // Above the installation's ceiling, the gateway names the field too.
    expect(
      environmentProblems(
        refusal(400, "invalid_request", [
          { field: "pids", reason: "limit_out_of_range" },
          { field: "workspace_mib", reason: "limit_out_of_range" },
        ]),
      ),
    ).toEqual({
      pids: "environments.pidsInvalid",
      disk: "environments.diskInvalid",
    });
    expect(
      environmentProblems(
        refusal(400, "invalid_request", [
          { field: "capabilities", reason: "renamed" },
        ]),
      ),
    ).toEqual({ capabilities: "environments.fieldInvalid" });
    for (const [code, key] of [
      ["conflict", "environments.conflict"],
      ["name_taken", "environments.nameTaken"],
      ["template_count", "environments.full"],
      ["template_unavailable", "environments.notOffered"],
      ["default_count", "environments.defaultCount"],
      ["template_is_default", "environments.isDefault"],
      ["unavailable", "environments.saveFailed"],
    ] as const)
      expect(environmentFailure(refusal(409, code))).toBe(key);
    expect(environmentFailure(refusal(400, "invalid_request"))).toBeNull();
    expect(environmentFailure(new Error("offline"))).toBe(
      "environments.saveFailed",
    );
  });

  it("says each thing that still uses an environment", () => {
    expect(
      inUseParts({
        default: true,
        conversations: 2,
        mcp_servers: ["files", "wiki"],
      }),
    ).toEqual([
      ["environments.inUseDefault", {}],
      ["environments.inUseTasks", { count: 2 }],
      ["environments.inUseServers", { names: "files, wiki" }],
    ]);
    expect(
      inUseParts({ default: false, conversations: 0, mcp_servers: [] }),
    ).toEqual([]);
  });
});

// GW-U-093: `new` is the Add page's address, so no environment takes it; the page checks before the gateway does.
describe("GW-U-093 the reserved name, on the page", () => {
  it("refuses the name new before sending, and words the gateway's refusal", () => {
    const draft = { ...toDraft(null), name: "new", displayName: "New" };
    expect(fromDraft(draft, true).problems).toMatchObject({
      name: "environments.nameReserved",
    });
    expect(
      fromDraft({ ...draft, name: "new-tools" }, true).problems,
    ).not.toHaveProperty("name");
    expect(
      environmentProblems(
        refusal(400, "invalid_request", [
          { field: "name", reason: "name_reserved" },
        ]),
      ),
    ).toEqual({ name: "environments.nameReserved" });
  });
});

// WEB-U-097: the ID is made from the display name, so nobody has to know its rules to add an environment.
describe("WEB-U-097 an environment's ID from its display name", () => {
  const none = new Set<string>();
  it.each([
    ["Data science", "data-science"],
    ["  Data   Science!! Tools ", "data-science-tools"],
    ["Café Olé", "cafe-ole"],
    ["2026 reports", "reports"],
    ["team_tools", "team-tools"],
    ["x".repeat(80), "x".repeat(64)],
  ])("makes %j into %j", (display, id) => {
    expect(environmentId(display, none)).toBe(id);
  });

  it("gives nothing when the display name has no letters it can use", () => {
    expect(environmentId("数据", none)).toBe("");
    expect(environmentId("2026", none)).toBe("");
    expect(environmentId("", none)).toBe("");
  });

  it("never makes new, nor an ID already taken", () => {
    expect(environmentId("New", none)).toBe("new-environment");
    expect(environmentId("Minimal", new Set(["minimal"]))).toBe("minimal-2");
    expect(environmentId("Minimal", new Set(["minimal", "minimal-2"]))).toBe(
      "minimal-3",
    );
    const long = "y".repeat(64);
    expect(environmentId(long, new Set([long]))).toBe(`${"y".repeat(62)}-2`);
  });
});
