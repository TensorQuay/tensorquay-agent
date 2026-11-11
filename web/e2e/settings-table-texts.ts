import { timesShown } from "./settings-page";
import { expect, type Page } from "./test";

// The texts WEB-E2E-120 makes long, each at the limit the gateway checks, and how a test opens a Settings table page.

const MIXED = "AbcdEfghIjklMnopQrstUvwxYz0123456789";
const UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
export const LOWER = "abcdefghijklmnopqrstuvwxyz0123456789";

/** `start` and then `alphabet` over and over, `length` characters in all: no space, hyphen or slash to break at. */
export function fill(start: string, length: number, alphabet = MIXED) {
  let out = start;
  for (let at = 0; out.length < length; at++)
    out += alphabet[at % alphabet.length] ?? "";
  return out.slice(0, length);
}
/** Each long form starts from the value it replaces, so a name and every reference to it still agree. */
export const word = (value: string) => value.replace(/[^A-Za-z0-9]/g, "");
/** Each kind starts with its own letter, so a route and a model of the same name are two texts. */
const text = (first: string, length: number) => (value: string) =>
  fill(`${first}${word(value)}`, length);
const lower = (first: string, length: number) => (value: string) =>
  fill(`${first}${word(value).toLowerCase()}`, length, LOWER);
/** A host name of 253 bytes in labels of 63 and 61 letters: a dot is no place to break a line. */
const host = (value: string) =>
  [
    fill(`h${word(value).toLowerCase()}`, 63, LOWER),
    fill("b", 63, LOWER),
    fill("c", 63, LOWER),
    fill("d", 61, LOWER),
  ].join(".");
function hostOf(value: string) {
  try {
    return new URL(value).hostname;
  } catch {
    return value;
  }
}

export type Kind = {
  what: string;
  /** The longest the gateway takes, by its own check. */
  limit: string;
  make: (value: string) => string;
  /** The part of it a row shows, when not all of it. */
  shows?: (long: string) => string;
};

/** Each text from the API that a table row shows, at its limit. */
export const KINDS = {
  keyName: {
    what: "a key's name",
    limit: "64 characters, ^[A-Z][A-Z0-9_]{0,63}$",
    make: (value) => fill(`K${word(value).toUpperCase()}`, 64, UPPER),
  },
  host: {
    what: "a host",
    limit: "253 bytes in labels of up to 63",
    make: host,
  },
  path: {
    what: "an address's path",
    limit: "512 bytes",
    make: (value) => `/${fill(`p${word(value)}`, 511)}`,
  },
  email: {
    what: "an email address",
    limit: "254 characters with one @",
    make: (value) =>
      `${fill(`u${word(value.split("@")[0] ?? value).toLowerCase()}`, 252, LOWER)}@x`,
  },
  url: {
    what: "a URL's host",
    limit: "253 bytes",
    make: (value) => `https://${host(hostOf(value))}/`,
    shows: (long) => long.slice("https://".length, -1),
  },
  reference: {
    what: "a secret's reference",
    limit: "secrets/ and 120 characters",
    make: (value) =>
      `secrets/${fill(`S${word(value.replace(/^secrets\//, ""))}`, 120)}`,
    shows: (long) => long.slice("secrets/".length),
  },
  routeName: {
    what: "a model route's name",
    limit: "128 bytes",
    make: text("R", 128),
  },
  routeLabel: {
    what: "a model route's display name",
    limit: "80 characters",
    make: text("L", 80),
  },
  model: { what: "a model", limit: "128 characters", make: text("M", 128) },
  environmentName: {
    what: "an environment's name",
    limit: "64 characters, ^[a-z][a-z0-9_-]{0,63}$",
    make: lower("e", 64),
  },
  environmentLabel: {
    what: "an environment's display name",
    limit: "64 characters",
    make: text("E", 64),
  },
  profileLabel: {
    what: "an agent profile's display name",
    limit: "64 characters",
    make: text("P", 64),
  },
  profileDescription: {
    what: "an agent profile's description",
    limit: "512 characters",
    make: text("B", 512),
  },
  description: {
    what: "a description",
    limit: "160 characters",
    make: text("D", 160),
  },
  connectorName: {
    what: "a connector's name",
    limit: "16 characters, ^[a-z][a-z0-9_]{0,15}$",
    make: lower("c", 16),
  },
  connectorLabel: {
    what: "a connector's display name",
    limit: "80 characters",
    make: text("C", 80),
  },
  engineName: {
    what: "a decision engine's name",
    limit: "64 characters, ^[a-z][a-z0-9_-]{0,63}$",
    make: lower("g", 64),
  },
  engineLabel: {
    what: "a decision engine's display name",
    limit: "80 characters",
    make: text("G", 80),
  },
  consumer: {
    what: "who read a key (a route's or connector's name)",
    limit: "128 bytes",
    make: text("W", 128),
  },
  tokenHint: {
    what: "an API token's last characters",
    limit: "4 characters",
    make: (value) => fill(`H${word(value)}`, 4, LOWER),
  },
  tokenName: {
    what: "an API token's name",
    limit: "100 characters",
    make: text("N", 100),
  },
  workspaceName: {
    what: "a workspace's name",
    limit: "100 characters",
    make: text("S", 100),
  },
  taskTitle: {
    what: "a task's title",
    limit: "200 characters",
    make: text("T", 200),
  },
} satisfies Record<string, Kind>;
export type KindName = keyof typeof KINDS;

/** Which field of which answer holds which text (a path as `items[].name`). */
export const ANSWERS: {
  api: RegExp;
  group?: string;
  fields: Record<string, KindName>;
}[] = [
  {
    api: /^\/api\/v1\/(admin\/)?host-keys(\/shared)?$/,
    fields: {
      "items[].name": "keyName",
      "items[].host": "host",
      "items[].path_prefix": "path",
      "items[].users[].email": "email",
      "items[].last_use.task_title": "taskTitle",
      "items[].last_use.user.email": "email",
      "personal[].host": "host",
      "personal[].path_prefix": "path",
      "personal[].owner.email": "email",
    },
  },
  {
    api: /^\/api\/v1\/admin\/secrets$/,
    fields: {
      "items[].ref": "reference",
      "items[].routes[]": "routeName",
      "items[].consumers[].name": "consumer",
    },
  },
  {
    api: /^\/api\/v1\/admin\/secrets\/[^/]+\/reads$/,
    fields: { "items[].consumer": "consumer" },
  },
  {
    api: /^\/api\/v1\/profiles$/,
    fields: {
      "items[].display_name": "profileLabel",
      "items[].description": "profileDescription",
      "items[].model_route": "routeName",
      "items[].template": "environmentName",
      "items[].decision_engine": "engineName",
      "items[].connectors[]": "connectorName",
    },
  },
  {
    api: /^\/api\/v1\/(users|invitations)$/,
    fields: { "items[].email": "email" },
  },
  {
    api: /^\/api\/v1\/admin\/routes$/,
    fields: {
      "items[].name": "routeName",
      "items[].display_name": "routeLabel",
      "items[].model": "model",
      "items[].key_ref": "reference",
      "items[].description": "description",
      "items[].fallbacks[]": "routeName",
      "items[].fallback_of[]": "routeName",
    },
  },
  {
    api: /^\/api\/v1\/models$/,
    fields: {
      "items[].name": "routeName",
      "items[].display_name": "routeLabel",
      "items[].model": "model",
      "items[].description": "description",
    },
  },
  {
    api: /^\/api\/v1\/(admin\/)?templates$/,
    fields: {
      "items[].name": "environmentName",
      "items[].display_name": "environmentLabel",
      "items[].description": "description",
      "items[].base": "environmentName",
    },
  },
  {
    api: /^\/api\/v1\/(admin\/)?connectors$/,
    fields: {
      "items[].name": "connectorName",
      "items[].display_name": "connectorLabel",
      "items[].url": "url",
      "items[].secret_ref": "reference",
    },
  },
  {
    api: /^\/api\/v1\/admin\/decision\/engines$/,
    fields: {
      "items[].name": "engineName",
      "items[].display_name": "engineLabel",
      "items[].url": "url",
      "items[].model": "model",
      "items[].key_ref": "reference",
    },
  },
  { api: /^\/api\/v1\/admin\/egress$/, fields: { "items[].host": "host" } },
  {
    api: /^\/api\/v1\/admin\/usage$/,
    group: "user",
    fields: { "rows[].label": "email" },
  },
  {
    api: /^\/api\/v1\/admin\/usage$/,
    group: "route",
    fields: { "rows[].key": "routeName", "rows[].label": "routeLabel" },
  },
  {
    api: /^\/api\/v1\/tokens$/,
    fields: { "items[].name": "tokenName", "items[].hint": "tokenHint" },
  },
  {
    api: /^\/api\/v1\/workspaces$/,
    fields: {
      "items[].name": "workspaceName",
      "items[].owner.email": "email",
      "items[].lease.title": "taskTitle",
    },
  },
  {
    api: /^\/api\/v1\/conversations$/,
    fields: {
      "items[].model_route": "routeName",
      "items[].template": "environmentName",
      "items[].title": "taskTitle",
    },
  },
];

export async function open(page: Page, href: string) {
  await page.goto(href);
  await expect(page.locator("table").first()).toBeVisible();
  await page.waitForLoadState("networkidle");
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
  await timesShown(page);
}
