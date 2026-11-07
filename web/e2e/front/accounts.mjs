import { randomBytes, randomUUID } from "node:crypto";

import { user } from "./fixtures.mjs";
import { seededMap } from "./state.mjs";
import { teamsOn } from "./teams.mjs";

// Team accounts for the fake front (task 067): each sign-in starts a team with its administrator and one member, who
// owns two tasks the administrator may see only as metadata. Invitation links let a second browser join that team.
const DAY_MS = 86_400_000;
const invitationLinks = seededMap();
const colleagueEmail = "colleague@example.invalid";

const now = () => new Date().toISOString();
const later = (days) => new Date(Date.now() + days * DAY_MS).toISOString();
/** The installation's upload and preview limits, as sign-in and me carry them (GW-I-142, GW-I-144): the gateway's defaults. */
export const UPLOAD_LIMITS = {
  upload_bytes: 25 * 1024 * 1024,
  uploads_bytes: 1024 * 1024 * 1024,
  preview_html_bytes: 2 * 1024 * 1024,
  preview_pdf_bytes: 20 * 1024 * 1024,
  preview_sheet_bytes: 10 * 1024 * 1024,
};
const publicAccount = ({ id, email, role }) => ({
  id,
  email,
  role,
  internet: "none",
  limits: UPLOAD_LIMITS,
  // ADR-0034 S6: the install switch, on only in the "agent-teams" scenario.
  features: { agent_teams: teamsOn() },
});
const listed = ({ id, email, role, disabled, created_at }) => ({
  id,
  email,
  role,
  disabled,
  created_at,
  last_active_at: null,
});
const listedToken = ({
  id,
  name,
  hint,
  created_at,
  expires_at,
  last_used_at,
}) => ({
  id,
  name,
  hint,
  created_at,
  expires_at,
  last_used_at,
});
const task = (owner) => {
  const id = randomUUID();
  return {
    id,
    workspace_id: id,
    owner_id: owner,
    model_route: "local",
    template: "workstation",
    pinned: false,
    title_source: "message",
    backend: "docker",
    state: "active",
    created_at: "2026-01-02T09:00:00Z",
    updated_at: "2026-01-02T09:00:00Z",
    last_activity_at: "2026-01-03T15:30:00Z",
    mcp_servers: [],
    connectors: [],
    team: null,
  };
};

/** A new team for one sign-in; signing in as the seeded member makes the session that member. */
export function membership(email) {
  const admin = {
    ...user,
    disabled: false,
    created_at: "2026-01-01T08:00:00Z",
  };
  const colleague = {
    id: randomUUID(),
    email: colleagueEmail,
    role: "member",
    disabled: false,
    created_at: "2026-01-02T08:00:00Z",
  };
  const team = {
    accounts: [admin, colleague],
    invitations: [],
    tokens: new Map(),
    tasks: [task(colleague.id), task(colleague.id)],
  };
  return { team, account: email === colleagueEmail ? colleague : admin };
}

export const sessionUser = (member) => publicAccount(member.account);

/** Another member's task, visible to administrators as metadata only (no title, no events). */
export function otherTask(member, id) {
  return member.team.tasks.find((value) => value.id === id);
}
export const teamTasks = (member) => member.team.tasks;

/** Public invitation routes; returns the membership to sign in with after an accepted invitation. */
export async function invitationRoute(path, method, read, reply, refuse) {
  if (method !== "POST") return false;
  if (path === "/api/v1/invitations/lookup") {
    const { token } = await read();
    const link = usable(token);
    if (!link) refuse(400, "invitation_invalid", "Invalid invitation.");
    else
      reply(200, {
        email: link.invitation.email,
        role: link.invitation.role,
        expires_at: link.invitation.expires_at,
      });
    return true;
  }
  if (path === "/api/v1/invitations/accept") {
    const { token, password } = await read();
    const link = usable(token);
    if (!link) {
      refuse(400, "invitation_invalid", "Invalid invitation.");
      return true;
    }
    if (typeof password !== "string" || Array.from(password).length < 12) {
      refuse(400, "bad_request", "The password is too short.");
      return true;
    }
    const account = {
      id: randomUUID(),
      email: link.invitation.email,
      role: link.invitation.role,
      disabled: false,
      created_at: now(),
    };
    link.team.accounts.push(account);
    link.invitation.status = "accepted";
    invitationLinks.delete(token);
    return { team: link.team, account };
  }
  return false;
}
function usable(token) {
  const link = typeof token === "string" ? invitationLinks.get(token) : null;
  return link &&
    link.invitation.status === "pending" &&
    Date.parse(link.invitation.expires_at) > Date.now()
    ? link
    : null;
}

/** Signed-in account routes (users, invitations, tokens); returns true when it answered. */
export async function accountRoute(member, path, method, read, reply, refuse) {
  const { team, account } = member;
  const admin = account.role === "admin";
  const forbid = () => refuse(403, "forbidden", "Administrators only.");
  if (path === "/api/v1/tokens") {
    const tokens = team.tokens.get(account.id) ?? [];
    team.tokens.set(account.id, tokens);
    if (method === "GET") {
      reply(200, { items: tokens.map(listedToken) });
      return true;
    }
    if (method === "POST") {
      const { name, expires_in_days: days } = await read();
      if (tokens.some((value) => value.name === name))
        refuse(409, "token_name_taken", "Name taken.");
      else if (tokens.length >= 50) refuse(409, "token_limit", "Too many.");
      else {
        const secret = `tqa_pat_${randomBytes(32).toString("base64url")}`;
        const token = {
          id: randomUUID(),
          name,
          hint: secret.slice(-4),
          created_at: now(),
          expires_at: later(days),
          last_used_at: null,
        };
        tokens.unshift({ ...token, secret });
        reply(201, { ...token, token: secret });
      }
      return true;
    }
  }
  const token = /^\/api\/v1\/tokens\/([^/]+)$/.exec(path);
  if (token && method === "DELETE") {
    const tokens = team.tokens.get(account.id) ?? [];
    const index = tokens.findIndex((value) => value.id === token[1]);
    if (index < 0) refuse(404, "not_found", "No such token.");
    else {
      tokens.splice(index, 1);
      reply(204);
    }
    return true;
  }
  if (path === "/api/v1/users" && method === "GET") {
    if (!admin) forbid();
    else reply(200, { items: team.accounts.map(listed) });
    return true;
  }
  const change = /^\/api\/v1\/users\/([^/]+)$/.exec(path);
  if (change && method === "PATCH") {
    if (!admin) {
      forbid();
      return true;
    }
    const target = team.accounts.find((value) => value.id === change[1]);
    const input = await read();
    if (!target) {
      refuse(404, "not_found", "No such account.");
      return true;
    }
    const next = { ...target, ...input };
    const admins = team.accounts
      .map((value) => (value.id === target.id ? next : value))
      .filter((value) => value.role === "admin" && !value.disabled);
    if (admins.length === 0) {
      refuse(409, "last_admin", "The last administrator must stay.");
      return true;
    }
    Object.assign(target, input);
    reply(200, listed(target));
    return true;
  }
  if (path === "/api/v1/invitations") {
    if (!admin) {
      forbid();
      return true;
    }
    if (method === "GET") {
      reply(200, { items: team.invitations });
      return true;
    }
    if (method === "POST") {
      const { email, role } = await read();
      if (team.accounts.some((value) => value.email === email)) {
        refuse(409, "email_taken", "Email taken.");
        return true;
      }
      const invitation = {
        id: randomUUID(),
        email,
        role,
        status: "pending",
        created_by: account.id,
        created_at: now(),
        expires_at: later(7),
      };
      const secret = randomBytes(32).toString("base64url");
      team.invitations.unshift(invitation);
      invitationLinks.set(secret, { team, invitation });
      reply(201, { ...invitation, token: secret });
      return true;
    }
  }
  const invitation = /^\/api\/v1\/invitations\/([^/]+)$/.exec(path);
  if (invitation && method === "DELETE") {
    if (!admin) {
      forbid();
      return true;
    }
    const found = team.invitations.find((value) => value.id === invitation[1]);
    if (!found) refuse(404, "not_found", "No such invitation.");
    else if (found.status !== "pending")
      refuse(409, "invitation_closed", "Already closed.");
    else {
      found.status = "revoked";
      reply(204);
    }
    return true;
  }
  return false;
}
