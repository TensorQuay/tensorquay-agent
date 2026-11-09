import { randomUUID } from "node:crypto";

// Workspaces for the fake front: each task's own workspace, workspaces made on the page, forks that are
// copied for a moment, and two workspaces another person shares with the session, behind the gateway's routes and
// rules (owner-only share, rename and delete; share by exact email; no delete while in use or with others' tasks).
const GiB = 1024 ** 3;
const QUOTA = 10 * GiB;
const COPY_MS = 1500;
const now = () => new Date().toISOString();
const ana = {
  id: "0195d8c0-0000-7000-8000-00000000a0a1",
  email: "ana@example.invalid",
};
/** Active accounts share-by-email can find; any other address is unknown. */
const accounts = [
  ana,
  { id: "0195d8c0-0000-7000-8000-00000000a0a2", email: "li@example.invalid" },
  {
    id: "0195d8c0-0000-7000-8000-00000000a0a3",
    email: "sam@example.invalid",
  },
];
const stores = new WeakMap();

function seed() {
  const shared = (name, access, size, lease) => ({
    id: randomUUID(),
    owner: ana,
    name,
    access,
    state: "ready",
    created_at: "2026-01-01T08:00:00Z",
    last_used_at: "2026-01-02T08:00:00Z",
    size_bytes: size,
    lease,
    shares: new Map(),
  });
  return {
    made: new Map(),
    shared: [
      shared("Team handbook", "write", Math.round(1.2 * GiB), {
        conversation_id: randomUUID(),
        title: null,
        running: false,
        pinned: false,
      }),
      shared("Q3 planning", "read", Math.round(0.2 * GiB), null),
    ],
  };
}

function store(conversations) {
  let value = stores.get(conversations);
  if (!value) {
    value = seed();
    stores.set(conversations, value);
  }
  return value;
}

/** Every workspace the session can see: made on the page, each task's own, and the two shared with it. */
function all(member, conversations) {
  const state = store(conversations);
  const own = [...conversations.values()]
    .filter((task) => task.workspace_id === task.id)
    .map((task) => {
      if (!state.made.has(task.id))
        state.made.set(task.id, {
          id: task.id,
          // As the gateway names it (workspaces.rs): the title's first 100 characters, the most a name may hold.
          name: [...(task.title ?? "Untitled task")].slice(0, 100).join(""),
          state: "ready",
          created_at: task.created_at,
          size_bytes: 64 * 1024,
          shares: new Map(),
          derived: true,
        });
      return state.made.get(task.id);
    });
  const made = [...state.made.values()].filter(
    (workspace) => !workspace.derived,
  );
  return [
    ...made.map((workspace) => ({
      ...workspace,
      owner: member.account,
      access: "owner",
    })),
    ...own.map((workspace) => ({
      ...workspace,
      owner: member.account,
      access: "owner",
    })),
    ...state.shared,
  ];
}

function bound(conversations, id) {
  return [...conversations.values()].filter(
    (task) => task.workspace_id === id && task.state === "active",
  );
}

function view(workspace, conversations) {
  const tasks = bound(conversations, workspace.id);
  const running = tasks.find((task) => task.running);
  const holder = running ?? tasks.find((task) => task.leased);
  const lease =
    workspace.lease ??
    (holder
      ? {
          conversation_id: holder.id,
          title: holder.title ?? null,
          running: Boolean(running),
          // The holder's keep-alive pin, as the gateway reports it beside running.
          pinned: Boolean(holder.pinned),
        }
      : null);
  const owner = workspace.access === "owner";
  const last = tasks
    .map((task) => task.last_activity_at)
    .sort()
    .at(-1);
  return {
    id: workspace.id,
    owner_id: workspace.owner.id,
    name: workspace.name,
    // How it was made, as the gateway says (GW-I-215): this fake makes no helper copies of its own.
    kind: workspace.kind ?? "standard",
    state: workspace.state,
    access: workspace.access,
    created_at: workspace.created_at,
    updated_at: workspace.created_at,
    last_used_at: last ?? workspace.last_used_at ?? null,
    size_bytes: workspace.state === "ready" ? workspace.size_bytes : null,
    size_measured_at: workspace.state === "ready" ? workspace.created_at : null,
    ...(workspace.fork ? { fork: workspace.fork } : {}),
    owner: { id: workspace.owner.id, email: workspace.owner.email },
    quota_bytes: QUOTA,
    lease,
    share_count: owner ? workspace.shares.size : null,
    task_count: owner ? tasks.length : null,
  };
}

const validName = (value) =>
  typeof value === "string" &&
  value.trim() !== "" &&
  Array.from(value.trim()).length <= 100;

/** The workspace a new task may bind: ready, and owned or shared for writing. */
export function bindable(member, conversations, id) {
  const workspace = all(member, conversations).find((item) => item.id === id);
  if (!workspace) return [404, "not_found"];
  if (workspace.access === "read") return [403, "forbidden"];
  if (workspace.state !== "ready") return [409, "workspace_not_ready"];
  return null;
}

/**
 * The list as the gateway pages it (GW-I-117, GW-I-122): `q` in the name, `filter` by lease and access, `sort` recent or
 * by name, and `limit` with a cursor (here an offset); without `limit`, the whole list.
 */
function listed(views, url) {
  const q = (url.searchParams.get("q") ?? "").trim().toLowerCase();
  const filter = url.searchParams.get("filter") ?? "all";
  const keep = (item) =>
    (!q || (item.name ?? "").toLowerCase().includes(q)) &&
    (filter === "all" ||
      (filter === "in_use" && Boolean(item.lease)) ||
      (filter === "idle" && !item.lease) ||
      (filter === "shared_with_me" && item.access !== "owner"));
  const used = (item) => Date.parse(item.last_used_at ?? item.created_at);
  const found = views.filter(keep);
  if (url.searchParams.get("sort") === "name")
    found.sort((a, b) =>
      (a.name ?? "").toLowerCase().localeCompare((b.name ?? "").toLowerCase()),
    );
  else if (url.searchParams.get("sort") === "recent")
    found.sort((a, b) => used(b) - used(a));
  const limit = Number(url.searchParams.get("limit") ?? 0);
  if (!limit) return { items: found };
  const start = Number(url.searchParams.get("cursor") ?? 0);
  const next = start + limit;
  return {
    items: found.slice(start, next),
    ...(next < found.length ? { next_cursor: String(next) } : {}),
  };
}

/** Answers the workspace routes; returns false for any other path. */
export async function workspaceRoute(
  member,
  conversations,
  url,
  method,
  read,
  reply,
  refuse,
  close,
) {
  const path = url.pathname;
  if (!path.startsWith("/api/v1/workspaces")) return false;
  const state = store(conversations);
  const items = all(member, conversations);
  if (path === "/api/v1/workspaces") {
    if (method === "GET") {
      reply(
        200,
        listed(
          items.map((workspace) => view(workspace, conversations)),
          url,
        ),
      );
      return true;
    }
    if (method === "POST") {
      const input = await read();
      if (!validName(input.name)) {
        refuse(400, "bad_request", "Invalid name.");
        return true;
      }
      const workspace = {
        id: randomUUID(),
        name: input.name.trim(),
        state: "ready",
        created_at: now(),
        size_bytes: 0,
        shares: new Map(),
      };
      state.made.set(workspace.id, workspace);
      reply(
        201,
        view(
          { ...workspace, owner: member.account, access: "owner" },
          conversations,
        ),
      );
      return true;
    }
  }
  const match =
    /^\/api\/v1\/workspaces\/([^/]+)(\/fork|\/shares(?:\/([^/]+))?)?$/.exec(
      path,
    );
  const workspace = match && items.find((item) => item.id === match[1]);
  if (!workspace) {
    refuse(404, "not_found", "Not found.");
    return true;
  }
  const record = state.made.get(workspace.id);
  const owner = workspace.access === "owner";
  const [, , action = "", member_id] = match;
  if (action === "" && method === "GET") {
    reply(200, view(workspace, conversations));
    return true;
  }
  if (action === "/fork" && method === "POST") {
    const input = await read();
    if (!validName(input.name)) {
      refuse(400, "bad_request", "Invalid name.");
      return true;
    }
    if (workspace.state !== "ready") {
      refuse(409, "workspace_not_ready", "Not ready.");
      return true;
    }
    const copy = {
      id: randomUUID(),
      name: input.name.trim(),
      state: "forking",
      created_at: now(),
      size_bytes: workspace.size_bytes,
      fork: { from: workspace.id, consistent: true },
      shares: new Map(),
    };
    state.made.set(copy.id, copy);
    setTimeout(() => {
      copy.state = "ready";
    }, COPY_MS).unref();
    reply(
      202,
      view({ ...copy, owner: member.account, access: "owner" }, conversations),
    );
    return true;
  }
  if (!owner || !record) {
    refuse(403, "forbidden", "Only the owner may do this.");
    return true;
  }
  if (action === "" && method === "PATCH") {
    const input = await read();
    if (input.name !== undefined && !validName(input.name)) {
      refuse(400, "bad_request", "Invalid name.");
      return true;
    }
    if (input.name !== undefined) record.name = input.name.trim();
    reply(200, view({ ...workspace, ...record }, conversations));
    return true;
  }
  if (action === "" && method === "DELETE") {
    const tasks = bound(conversations, workspace.id);
    const withTasks = url.searchParams.get("with_conversations") === "true";
    if (
      workspace.state === "forking" ||
      // A going run or a pinned holder refuses; a kept, idle environment is retired, as the gateway does.
      tasks.some((task) => task.running || (task.leased && task.pinned)) ||
      (tasks.length > 0 && !withTasks)
    ) {
      refuse(409, "workspace_in_use", "The workspace is in use.");
      return true;
    }
    for (const task of tasks) {
      close(task);
      conversations.delete(task.id);
    }
    state.made.delete(workspace.id);
    reply(204);
    return true;
  }
  if (action === "/shares" && method === "GET") {
    reply(200, { items: [...record.shares.values()] });
    return true;
  }
  if (action === "/shares" && method === "PUT") {
    const input = await read();
    const email = String(input.email ?? "")
      .trim()
      .toLowerCase();
    const account = accounts.find((item) => item.email === email);
    if (!account) {
      refuse(404, "user_not_found", "No account uses that email.");
      return true;
    }
    const share = {
      user_id: account.id,
      email: account.email,
      access: input.access === "read" ? "read" : "write",
    };
    record.shares.set(account.id, share);
    reply(200, share);
    return true;
  }
  const share = member_id && record.shares.get(member_id);
  if (action.startsWith("/shares/") && share && method === "PUT") {
    const input = await read();
    share.access = input.access === "read" ? "read" : "write";
    reply(200, share);
    return true;
  }
  if (action.startsWith("/shares/") && share && method === "DELETE") {
    record.shares.delete(member_id);
    reply(204);
    return true;
  }
  refuse(404, "not_found", "Not found.");
  return true;
}
