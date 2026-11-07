// Agent profiles on the fake front (ADR-0012): each account's own, then the shared ones; members write only their
// own (403 on a shared one); versions and name_taken. One shared profile can't be used (its environment is gone).
import { seededMap } from "./state.mjs";
const NAME = /^[a-z][a-z0-9_-]{0,63}$/;
let next = 0;
const id = () =>
  `0195d8c0-0000-7000-8000-${String(700000000000 + (next += 1))}`;
const base = (name, extra) => ({
  id: id(),
  name,
  display_name: name[0].toUpperCase() + name.slice(1),
  shared: false,
  repositories: false,
  version: 1,
  updated_at: "2026-09-27T09:00:00Z",
  usable: true,
  skills: "all",
  mcp_servers: [],
  connectors: [],
  limits: {},
  ...extra,
});
const shared = seededMap(
  [
    base("team", {
      display_name: "Team default",
      shared: true,
      model_route: "local",
      // A field the gateway still accepts until its second release (PRD §12 item 22): no page offers it, and a
      // save passes it through untouched.
      repositories: true,
    }),
    base("legacy", {
      display_name: "Legacy tools",
      shared: true,
      template: "retired",
      usable: false,
      unusable_field: "template",
    }),
  ].map((item) => [item.id, item]),
);
const personal = seededMap();

/**
 * What a task keeps of the profile it was made from (ADR-0012; WEB-U-051): the profile's id, version, names and the
 * fields it sets. Null for an id the caller can't use.
 */
export function taskProfile(member, profileId) {
  const found =
    personal.get(member.account.id)?.get(profileId) ?? shared.get(profileId);
  if (!found?.usable) return null;
  const sets = [
    ["model_route", found.model_route],
    ["template", found.template],
    ["mcp_servers", found.mcp_servers?.length ? found.mcp_servers : null],
    ["connectors", found.connectors?.length ? found.connectors : null],
  ]
    .filter(([, value]) => value)
    .map(([field]) => field);
  return {
    profile: {
      id: found.id,
      version: found.version,
      name: found.name,
      display_name: found.display_name,
      sets,
    },
    model_route: found.model_route ?? null,
    template: found.template ?? null,
  };
}

/** Answers /api/v1/profiles*; returns false for any other path. */
export async function profileRoute(member, url, method, read, reply, refuse) {
  const match = /^\/api\/v1\/profiles(?:\/([^/]+))?$/.exec(url.pathname);
  if (!match) return false;
  const owner = member.account.id;
  if (!personal.has(owner)) personal.set(owner, new Map());
  const own = personal.get(owner);
  const admin = member.account.role === "admin";
  const target = match[1] ? decodeURIComponent(match[1]) : null;
  const found = target ? (own.get(target) ?? shared.get(target)) : null;
  if (!target && method === "GET") {
    reply(200, { items: [...own.values(), ...shared.values()] });
    return true;
  }
  if (target && !found) {
    refuse(404, "not_found", "Not found.");
    return true;
  }
  if (!target && method === "POST") {
    const input = await read();
    const store = input.shared ? shared : own;
    if (input.shared && !admin)
      refuse(403, "forbidden", "Administrators only.");
    else if (!NAME.test(input.name ?? ""))
      refuse(400, "invalid_request", "Invalid name.");
    else if ([...store.values()].some((item) => item.name === input.name))
      refuse(409, "name_taken", "Name taken.");
    else {
      const item = base(input.name, {
        ...input,
        shared: Boolean(input.shared),
      });
      store.set(item.id, item);
      reply(201, item);
    }
    return true;
  }
  if (found.shared && !admin && method !== "GET") {
    refuse(403, "forbidden", "Administrators only.");
    return true;
  }
  const store = found.shared ? shared : own;
  if (method === "GET") reply(200, found);
  else if (method === "PUT") {
    const input = await read();
    if (input.version !== found.version) refuse(409, "conflict", "Conflict.");
    else {
      const fields = { ...input };
      delete fields.version;
      const item = {
        ...found,
        ...fields,
        shared: found.shared,
        version: found.version + 1,
        usable: true,
      };
      delete item.unusable_field;
      store.set(found.id, item);
      reply(200, item);
    }
  } else if (method === "DELETE") {
    if (Number(url.searchParams.get("version")) !== found.version)
      refuse(409, "conflict", "Conflict.");
    else {
      store.delete(found.id);
      reply(204);
    }
  } else return false;
  return true;
}
