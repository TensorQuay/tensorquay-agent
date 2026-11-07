import { ApiError, type FieldProblem, type InUse } from "../api/error";
import {
  NAME,
  offered,
  PINNED,
  type Capability,
  type EnvironmentFields,
  type EnvironmentImage,
  type EnvironmentItem,
  type TemplatesApi,
} from "../settings/admin/templates";

import { SIZE_CEILING, SIZE_DEFAULTS } from "./limits";

const refuse = (
  status: number,
  code: string,
  details: FieldProblem[] = [],
  inUse?: InUse,
) => new ApiError(status, code, "fixture refusal", undefined, details, inUse);
export const DIGEST = `sha256:${"a".repeat(64)}`;

export function environment(
  name: string,
  extra: Partial<EnvironmentItem> = {},
): EnvironmentItem {
  return {
    name,
    display_name: name[0]?.toUpperCase() + name.slice(1),
    description: null,
    image: `tqa-runtime:${name}@${DIGEST}`,
    capabilities: ["shell"],
    cpus: null,
    memory_mib: null,
    pids: null,
    workspace_mib: null,
    clamped: [],
    clamped_from: null,
    builtin: null,
    customised: false,
    is_default: false,
    enabled: true,
    version: 1,
    last_set_by: "seed",
    seed: "current",
    check: {
      state: "verified",
      reason: null,
      checked_version: 1,
      secret_files: null,
      scan_limit: null,
    },
    base: null,
    image_id: DIGEST,
    bound_conversations: 0,
    updated_at: "2026-09-26T09:00:00Z",
    ...extra,
  };
}

/** The gateway's semantics (slice 4): versions, 409 codes, the offered-only default, in_use on delete, 400 details. */
export function fakeTemplates({
  environments = [],
  mcpServers = {},
  images = [],
}: {
  environments?: EnvironmentItem[];
  /** The tool servers that list each environment, for 409 template_in_use. */
  mcpServers?: Record<string, string[]>;
  /** The engine host's environment images, newest first; an error refuses the list. */
  images?: EnvironmentImage[] | ApiError;
} = {}) {
  const store = new Map(environments.map((item) => [item.name, { ...item }]));
  const replies = new Map<string, EnvironmentItem | null>();
  const writes: {
    name: string;
    fields: EnvironmentFields;
    version?: number;
  }[] = [];
  const checks: string[] = [];
  const resets: { name: string; version: number }[] = [];
  const settle = <T>(work: () => T): Promise<T> => {
    try {
      return Promise.resolve(work());
    } catch (error) {
      return Promise.reject(
        error instanceof Error ? error : new Error("fixture failure"),
      );
    }
  };
  const api: TemplatesApi = {
    environments: () =>
      settle(() => ({
        generation: 1,
        items: [...store.values()].sort((a, b) => a.name.localeCompare(b.name)),
        defaults: SIZE_DEFAULTS,
        ceiling: SIZE_CEILING,
      })),
    putEnvironment: (name, fields, version, key) =>
      settle(() => {
        writes.push({
          name,
          fields,
          ...(version === undefined ? {} : { version }),
        });
        const replay = replies.get(key);
        if (replay) return replay;
        if (!NAME.test(name))
          throw refuse(400, "invalid_request", [
            { field: "name", reason: "name_invalid" },
          ]);
        // The Add page's address (GW-U-093).
        if (name === "new")
          throw refuse(400, "invalid_request", [
            { field: "name", reason: "name_reserved" },
          ]);
        if (!PINNED.test(fields.image))
          throw refuse(400, "invalid_request", [
            { field: "image", reason: "image_not_pinned" },
          ]);
        const existing = store.get(name);
        if (!existing && version !== undefined) throw refuse(404, "not_found");
        if (existing && version === undefined) throw refuse(409, "name_taken");
        if (existing && version !== existing.version)
          throw refuse(409, "conflict");
        const item = environment(name, {
          ...existing,
          ...fields,
          cpus: fields.cpus ?? null,
          memory_mib: fields.memory_mib ?? null,
          enabled: fields.enabled ?? true,
          version: (existing?.version ?? 0) + 1,
          last_set_by: "admin",
          // An administrator's edit of a built-in makes it theirs (WEB-U-098).
          customised: existing?.builtin
            ? true
            : (existing?.customised ?? false),
          seed: existing?.seed ?? null,
          base: existing?.base ?? "workstation",
          check: {
            state: "unchecked",
            reason: null,
            checked_version: null,
            secret_files: null,
            scan_limit: null,
          },
        });
        store.set(name, item);
        replies.set(key, item);
        return { ...item };
      }),
    deleteEnvironment: (name, version, key) =>
      settle(() => {
        if (replies.has(key)) return;
        const existing = store.get(name);
        if (!existing) throw refuse(404, "not_found");
        if (existing.version !== version) throw refuse(409, "conflict");
        const servers = mcpServers[name] ?? [];
        if (
          existing.is_default ||
          existing.bound_conversations > 0 ||
          servers.length
        )
          throw refuse(409, "template_in_use", [], {
            default: existing.is_default,
            conversations: existing.bound_conversations,
            mcp_servers: servers,
          });
        store.delete(name);
        replies.set(key, null);
      }),
    makeDefault: (name, version, key) =>
      settle(() => {
        const replay = replies.get(key);
        if (replay) return replay;
        const existing = store.get(name);
        if (!existing) throw refuse(404, "not_found");
        if (existing.version !== version) throw refuse(409, "conflict");
        if (!offered(existing)) throw refuse(409, "template_unavailable");
        for (const [other, item] of store)
          store.set(other, { ...item, is_default: other === name });
        const moved = { ...existing, is_default: true };
        replies.set(key, moved);
        return moved;
      }),
    check: (name) =>
      settle(() => {
        const existing = store.get(name);
        if (!existing) throw refuse(404, "not_found");
        checks.push(name);
        store.set(name, {
          ...existing,
          check: {
            state: "unchecked",
            reason: null,
            checked_version: null,
            secret_files: null,
            scan_limit: null,
          },
        });
      }),
    images: () =>
      settle(() => {
        if (images instanceof ApiError) throw images;
        return { items: images.map((image) => ({ ...image })), limit: 200 };
      }),
    // The gateway's reset: the built-in's current build, its name and tools; size, description and Enabled stay.
    reset: (name, version, key) =>
      settle(() => {
        const replay = replies.get(key);
        if (replay) return replay;
        resets.push({ name, version });
        const existing = store.get(name);
        if (!existing) throw refuse(404, "not_found");
        if (existing.version !== version) throw refuse(409, "conflict");
        if (!existing.customised) throw refuse(409, "not_customised");
        const build = (images instanceof ApiError ? [] : images).find(
          (image) => image.built_in !== null && image.environment === name,
        );
        // What the default runs stays fixed while it is the default, as for an edit.
        if (
          existing.is_default &&
          build &&
          (build.id !== existing.image ||
            build.capabilities.join() !== existing.capabilities.join())
        )
          throw refuse(409, "template_is_default");
        const item: EnvironmentItem = {
          ...existing,
          image: build?.id ?? existing.image,
          image_id: build?.id ?? existing.image_id,
          display_name: build?.display_name ?? existing.display_name,
          capabilities: build?.capabilities ?? existing.capabilities,
          version: existing.version + 1,
          last_set_by: "builtin",
          customised: false,
          check: {
            state: "unchecked",
            reason: null,
            checked_version: null,
            secret_files: null,
            scan_limit: null,
          },
        };
        store.set(name, item);
        replies.set(key, item);
        return { ...item };
      }),
  };
  /** The gateway's checker answers: the environment is verified or refused with a reason. */
  const answer = (name: string, reason: string | null) => {
    const existing = store.get(name);
    if (existing)
      store.set(name, {
        ...existing,
        check: {
          state: reason ? "refused" : "verified",
          reason,
          checked_version: existing.version,
          secret_files: null,
          scan_limit: null,
        },
      });
  };
  const touch = (name: string) => {
    const existing = store.get(name);
    if (existing)
      store.set(name, { ...existing, version: existing.version + 1 });
  };
  return { api, writes, checks, resets, answer, touch };
}

/** A typical install: the two built-in environments (one the default), a refused, a pending and a disabled one. */
export const LISTED: EnvironmentItem[] = [
  environment("workstation", {
    display_name: "Workstation",
    is_default: true,
    bound_conversations: 2,
    capabilities: ["shell", "python", "node", "build", "browser"],
  }),
  environment("minimal", { display_name: "Minimal" }),
  environment("data", {
    display_name: "Data science",
    base: "workstation",
    seed: null,
    last_set_by: "admin",
    check: {
      state: "refused",
      reason: "not_derived",
      checked_version: 1,
      secret_files: null,
      scan_limit: null,
    },
  }),
  environment("gpu", {
    display_name: "GPU tools",
    base: "workstation",
    seed: null,
    last_set_by: "admin",
    version: 2,
    check: {
      state: "unchecked",
      reason: null,
      checked_version: null,
      secret_files: null,
      scan_limit: null,
    },
  }),
  environment("old", { display_name: "Old", enabled: false }),
];

const id = (hex: string) => `sha256:${hex.repeat(64)}`;
/** One image on the engine host, as the gateway lists it: usable, untagged and unused unless said otherwise. */
export function image(
  hex: string,
  extra: Partial<EnvironmentImage> = {},
): EnvironmentImage {
  return {
    id: id(hex),
    tags: [],
    created: "2026-09-28T09:00:00Z",
    size_bytes: 1_500_000_000,
    runtime: "current",
    environment: null,
    display_name: null,
    capabilities: [],
    capabilities_valid: true,
    usable: true,
    reason: null,
    based_on: null,
    used_by: [],
    sandboxes: 0,
    built_in: null,
    sizes: null,
    ...extra,
  };
}

/**
 * An install with built-ins from its own builds (the gateway registered Workstation and Minimal from their images),
 * Workstation customised by an administrator, and the host's images: the two built-in builds, one derived from
 * Workstation, one untagged, one too old and one without a runtime.
 */
export const BUILT_IN_IMAGES: EnvironmentImage[] = [
  image("1", {
    tags: ["tqa-runtime:workstation"],
    environment: "workstation",
    display_name: "Workstation",
    capabilities: ["shell", "python", "node", "build", "browser"],
    built_in: "registered",
    used_by: ["workstation"],
    sandboxes: 2,
    size_bytes: 3_200_000_000,
  }),
  image("2", {
    tags: ["tqa-runtime:base"],
    environment: "minimal",
    display_name: "Minimal",
    capabilities: ["shell", "python"],
    built_in: "registered",
    used_by: ["minimal"],
    size_bytes: 900_000_000,
  }),
  image("3", {
    tags: ["team/notebooks:1"],
    environment: "workstation",
    display_name: "Workstation",
    capabilities: ["shell", "python", "node", "build", "browser"],
    based_on: "workstation",
    created: "2026-09-29T15:30:00Z",
  }),
  image("4", { capabilities: ["shell"], based_on: "minimal" }),
  image("5", {
    tags: ["team/old-tools:2025"],
    runtime: "too_old",
    usable: false,
    reason: "too old for this version: rebuild it",
  }),
  image("6", {
    tags: ["postgres:16"],
    runtime: "none",
    usable: false,
    reason: "there is no TensorQuay runtime in it",
  }),
];

/** Every tool Desktop's label declares (images/runtime/Dockerfile), Workstation's and its own two. */
export const DESKTOP_TOOLS: Capability[] = [
  "shell",
  "python",
  "node",
  "build",
  "browser",
  "data",
  "desktop",
];

/**
 * Desktop after an administrator deleted it: its build is still on the host, listed as a built-in not added, with its
 * label's tools and its own size (WEB-U-385, WEB-U-386).
 */
export const DELETED_DESKTOP = image("d", {
  tags: ["tqa-runtime:desktop"],
  environment: "desktop",
  display_name: "Desktop",
  capabilities: DESKTOP_TOOLS,
  built_in: "not_added",
  reason: "deleted by an administrator",
  sizes: { cpus: 2, memory_mib: 4096, pids: 1024 },
  size_bytes: 3_500_000_000,
});

export const BUILT_INS: EnvironmentItem[] = [
  environment("workstation", {
    display_name: "Workstation",
    description: "Code, browsers and builds",
    is_default: true,
    image: id("9"),
    image_id: id("9"),
    capabilities: ["shell", "python", "node", "build", "browser"],
    builtin: { image_created: "2026-09-20T09:00:00Z", updated_at: null },
    customised: true,
    last_set_by: "admin",
    seed: null,
    version: 3,
  }),
  environment("minimal", {
    display_name: "Minimal",
    description: "A shell and Python",
    image: id("2"),
    image_id: id("2"),
    capabilities: ["shell", "python"],
    builtin: { image_created: "2026-09-28T09:00:00Z", updated_at: null },
    last_set_by: "builtin",
    seed: null,
  }),
];
