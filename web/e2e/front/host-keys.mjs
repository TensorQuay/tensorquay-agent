// Host keys on the fake front (ADR-0037 §1, §9), as the gateway serves them at /api/v1/host-keys and
// /api/v1/admin/host-keys: a team's members' own keys and its shared keys, metadata only (a value is taken and dropped),
// versions, 400 details and 409s from the gateway's closed lists, like src/test/host-keys.ts for the unit tests.
import { randomUUID } from "node:crypto";

import { egressAllows } from "./egress.mjs";
import { onReset } from "./state.mjs";

const NAME = /^[A-Z][A-Z0-9_]{0,63}$/;
const RESERVED = new Set([
  "PATH",
  "HOME",
  "USER",
  "LOGNAME",
  "SHELL",
  "PWD",
  "OLDPWD",
  "TMPDIR",
  "LANG",
  "LANGUAGE",
  "TERM",
  "HOSTNAME",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
  "ALL_PROXY",
  "SSL_CERT_FILE",
  "SSL_CERT_DIR",
  "REQUESTS_CA_BUNDLE",
  "CURL_CA_BUNDLE",
  "PIP_CERT",
  "NODE_EXTRA_CA_CERTS",
  "NODE_OPTIONS",
  "PYTHONPATH",
  "PYTHONHOME",
  "PYTHONSTARTUP",
]);
const RESERVED_PREFIXES = ["LC_", "LD_", "GIT_", "TQA_"];
const HOST =
  /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*$/;
const PRINTABLE = /^[\x21-\x7e]{1,4096}$/;
const SCHEMES = ["bearer", "basic"];

// The scenarios a test asks for at reset: they apply to every team made after it.
let scenarios = [];
let generation = 0;
onReset((names) => {
  scenarios = names;
  generation += 1;
});

/** Each team's keys: its members' own (by account id) and its shared ones, made again after every reset. */
const teams = new WeakMap();
function storeOf(team) {
  const current = teams.get(team);
  if (current?.generation === generation) return current;
  const made = { generation, own: new Map(), shared: new Map() };
  const admin = team.accounts.find((account) => account.role === "admin");
  if (scenarios.includes("shared-host-keys"))
    for (const users of ["all", "member"]) {
      const item = key(
        {
          name: users === "all" ? "GITHUB_TEAM" : "JIRA_TEAM",
          host: users === "all" ? "github.com" : "jira.acme.example",
          port: 443,
          path_prefix: users === "all" ? "/acme" : null,
          scheme: "bearer",
        },
        { shared: true },
      );
      made.shared.set(item.id, {
        item,
        users:
          users === "all"
            ? "all"
            : team.accounts
                .filter((account) => account.role === "member")
                .map((account) => account.id),
        owner: admin?.id,
      });
    }
  if (scenarios.includes("host-key-refused") && admin) {
    const item = key(
      {
        name: "GITHUB_WORK",
        host: "github.com",
        port: 443,
        path_prefix: "/acme",
        scheme: "bearer",
      },
      {},
    );
    item.last_use = {
      at: new Date(Date.now() - 3_600_000).toISOString(),
      count: 1,
      outcome: "foreign_run",
      conversation_id: null,
      task_title: null,
      user: null,
    };
    made.own.set(admin.id, new Map([[item.id, item]]));
  }
  teams.set(team, made);
  return made;
}

/** A key as the gateway lists it: metadata only. */
function key(fields, { shared = false } = {}) {
  return {
    id: randomUUID(),
    name: fields.name,
    host: fields.host,
    port: fields.port,
    path_prefix: fields.path_prefix,
    scheme: fields.scheme,
    user_name: fields.user_name ?? null,
    header_name: null,
    shared,
    users: null,
    set_at: new Date().toISOString(),
    last_use: null,
    allowed: true,
    check_name: false,
    version: 1,
  };
}

/** An IPv6 address in the one spelling the gateway stores, or null. */
function ipv6(text) {
  if (!/^[0-9a-fA-F:.]+$/.test(text) || !text.includes(":")) return null;
  try {
    const address = new URL(`https://[${text}]/`).hostname.slice(1, -1);
    const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(address);
    if (!mapped) return address;
    const high = parseInt(mapped[1], 16);
    const low = parseInt(mapped[2], 16);
    return `::ffff:${[high >> 8, high & 255, low >> 8, low & 255].join(".")}`;
  } catch {
    return null;
  }
}

/** The address as the gateway reads it (ADR-0037 §1, Amendment 2), or the reason it refuses it. */
function address(text) {
  let rest = String(text ?? "").trim();
  // As the gateway: the address is a URL, so anything but https:// (none at all included) is refused (WEB-U-192).
  const scheme = /^([a-z][a-z0-9+.-]*):\/\//i.exec(rest);
  if (!scheme || scheme[1].toLowerCase() !== "https")
    return { reason: "address_not_https" };
  rest = rest.slice(scheme[0].length);
  const slash = rest.indexOf("/");
  const authority = slash === -1 ? rest : rest.slice(0, slash);
  if (authority.includes("@") || /[?#]/.test(rest))
    return { reason: "address_not_plain" };
  if (!authority || rest.includes("*") || /\s/.test(rest))
    return { reason: "host_invalid" };
  let host;
  let port = "443";
  if (authority.startsWith("[")) {
    const close = authority.indexOf("]");
    host = close === -1 ? null : ipv6(authority.slice(1, close));
    const after = close === -1 ? "" : authority.slice(close + 1);
    if (after) {
      if (!after.startsWith(":")) return { reason: "host_invalid" };
      port = after.slice(1);
    }
  } else {
    const parts = authority.toLowerCase().split(":");
    if (parts.length > 2) return { reason: "host_invalid" };
    host = parts[0];
    if (parts[1] !== undefined) port = parts[1];
    if (!HOST.test(host)) host = null;
  }
  const number = Number(port);
  if (!host || !/^\d{1,5}$/.test(port) || number < 1 || number > 65535)
    return { reason: "host_invalid" };
  const path = slash === -1 ? "" : rest.slice(slash).replace(/\/+$/, "");
  return { host, port: number, path_prefix: path || null };
}

const reserved = (name) =>
  RESERVED.has(name) ||
  RESERVED_PREFIXES.some((prefix) => name.startsWith(prefix));

/** The body's field problems, in the gateway's closed reasons; the address as read when it has none. */
function check(input, adding) {
  const found = [];
  const where = address(input.address);
  if (where.reason) found.push({ field: "address", reason: where.reason });
  if (typeof input.name !== "string" || !NAME.test(input.name))
    found.push({ field: "name", reason: "name_invalid" });
  else if (reserved(input.name))
    found.push({ field: "name", reason: "name_reserved" });
  if (!SCHEMES.includes(input.scheme))
    found.push({ field: "scheme", reason: "scheme_not_supported" });
  if (input.scheme === "basic" && !input.user_name)
    found.push({ field: "user_name", reason: "user_name_required" });
  if (adding || input.value !== undefined) {
    if (typeof input.value !== "string" || input.value === "")
      found.push({ field: "value", reason: "value_empty" });
    else if (!PRINTABLE.test(input.value))
      found.push({ field: "value", reason: "value_invalid" });
  }
  return { found, where };
}

/** The keys a member's tasks may use: their own, then the shared keys meant for them. */
function sharedFor(store, account) {
  return [...store.shared.values()].filter(
    ({ users }) => users === "all" || users.includes(account.id),
  );
}

/** A key as one viewer sees it, with whether their tasks may reach its address now. */
/**
 * The key a stored use was made with, as the gateway joins its audit rows to keys (ADR-0037 §2): the sender's own key,
 * or a shared one, of that name and address; null once it is gone, and the row with it. `usable`: whether `viewer` may
 * use it now (their own, or shared with everyone or with them), which decides whether a row names it to its sender,
 * judged when read.
 */
export function keyOfUse(team, sender, viewer, { name, host, port }) {
  const store = storeOf(team);
  const fits = (item) =>
    item.name === name && item.host === host && item.port === port;
  if ([...(store.own.get(sender)?.values() ?? [])].some(fits))
    return { usable: viewer === sender };
  const shared = [...store.shared.values()].find(({ item }) => fits(item));
  if (!shared) return null;
  return { usable: shared.users === "all" || shared.users.includes(viewer) };
}

const shown = (item, users = null) => ({
  ...item,
  users,
  allowed: egressAllows(item.host, item.port),
});

const usersOf = (team, users) =>
  users === "all"
    ? "all"
    : users.map((id) => ({
        id,
        email: team.accounts.find((account) => account.id === id)?.email ?? "",
      }));

/** Answers /api/v1/host-keys* and /api/v1/admin/host-keys*; returns false for any other path. */
export async function hostKeysRoute(member, url, method, read, send) {
  const admin = url.pathname.startsWith("/api/v1/admin/host-keys");
  const match =
    /^\/api\/v1\/(?:admin\/)?host-keys(?:\/([^/]+))?(\/reads)?$/.exec(
      url.pathname,
    );
  if (!match) return false;
  const refuse = (status, code, details = []) =>
    send(status, {
      error: {
        code,
        message: "fixture refusal",
        retryable: false,
        ...(details.length ? { details } : {}),
      },
    });
  if (admin && member.account.role !== "admin") {
    refuse(403, "forbidden");
    return true;
  }
  const store = storeOf(member.team);
  if (!store.own.has(member.account.id))
    store.own.set(member.account.id, new Map());
  const own = store.own.get(member.account.id);
  const [, id, reads] = match;
  if (id === "shared" && !admin && method === "GET" && !reads) {
    send(200, {
      items: sharedFor(store, member.account).map(({ item }) => shown(item)),
    });
    return true;
  }
  if (!id && method === "GET") {
    if (admin)
      send(200, {
        items: [...store.shared.values()].map(({ item, users }) =>
          shown(item, usersOf(member.team, users)),
        ),
        personal: [...store.own.entries()]
          .filter(([owner]) => owner !== member.account.id)
          .flatMap(([owner, keys]) =>
            [...keys.values()].map((item) => ({
              id: item.id,
              host: item.host,
              port: item.port,
              path_prefix: item.path_prefix,
              owner: {
                id: owner,
                email:
                  member.team.accounts.find((account) => account.id === owner)
                    ?.email ?? "",
              },
              set_at: item.set_at,
              last_use: item.last_use ? { at: item.last_use.at } : null,
              version: item.version,
            })),
          ),
      });
    else send(200, { items: [...own.values()].map((item) => shown(item)) });
    return true;
  }
  const keys = admin
    ? new Map([...store.shared.entries()].map(([key, { item }]) => [key, item]))
    : own;
  if (!id && method === "POST") {
    write(await read(), null);
    return true;
  }
  const existing = id ? keys.get(id) : undefined;
  // An administrator may delete a member's own key too, never edit or use it.
  const personal =
    admin && id && !existing
      ? [...store.own.values()].find((owned) => owned.has(id))
      : undefined;
  if (reads && method === "GET") {
    if (!existing && !personal) refuse(404, "not_found");
    else
      send(200, { items: readsOf(existing ?? personal.get(id)), next: null });
    return true;
  }
  if (id && method === "PUT") {
    if (!existing) refuse(404, "not_found");
    else write(await read(), existing);
    return true;
  }
  if (id && method === "DELETE") {
    const item = existing ?? personal?.get(id);
    if (!item) refuse(404, "not_found");
    else if (String(item.version) !== url.searchParams.get("version"))
      refuse(409, "conflict");
    else {
      (existing ? (admin ? store.shared : own) : personal).delete(id);
      send(204);
    }
    return true;
  }
  return false;

  function write(input, current) {
    const { found, where } = check(input, current === null);
    if (admin && input.users === undefined)
      found.push({ field: "users", reason: "body_invalid" });
    if (found.length) return refuse(400, "invalid_request", found);
    if (current && input.version !== current.version)
      return refuse(409, "conflict");
    // A name is one key's among the person's own and the shared keys they may use.
    const taken = [
      ...(admin ? [] : [...own.values()]),
      ...(admin
        ? [...store.shared.values()].map(({ item }) => item)
        : sharedFor(store, member.account).map(({ item }) => item)),
    ].find((item) => item.name === input.name && item.id !== current?.id);
    if (taken)
      return refuse(409, "name_taken", [
        {
          field: "name",
          reason: "name_taken",
          taken_by: { id: taken.id, shared: taken.shared },
        },
      ]);
    const item = {
      ...(current ?? key({ ...where, ...input }, { shared: admin })),
      name: input.name,
      host: where.host,
      port: where.port,
      path_prefix: where.path_prefix,
      scheme: input.scheme,
      user_name: input.scheme === "basic" ? input.user_name : null,
      check_name: false,
      version: (current?.version ?? 0) + 1,
      ...(input.value === undefined
        ? {}
        : { set_at: new Date().toISOString() }),
    };
    if (admin)
      store.shared.set(item.id, {
        item,
        users: input.users,
        owner: member.account.id,
      });
    else own.set(item.id, item);
    send(
      current ? 200 : 201,
      shown(item, admin ? usersOf(member.team, input.users) : null),
    );
  }
}

/** A key's uses: its newest one as a read row, when it has one. */
function readsOf(item) {
  if (!item.last_use) return [];
  return [
    {
      id: randomUUID(),
      consumer_kind: "host",
      consumer: item.host,
      conversation_id: item.last_use.conversation_id ?? null,
      run_id: null,
      outcome: item.last_use.outcome ?? "ok",
      count: item.last_use.count ?? 1,
      first_at: item.last_use.at,
      last_at: item.last_use.at,
    },
  ];
}
