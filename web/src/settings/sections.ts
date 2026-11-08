/**
 * The settings menu (WEB-U-075, UI-GUIDELINES §7): three groups instead of a flat list. Your account is everyone's;
 * Administration and Operations are for administrators only. Each section has one address; the old addresses of the
 * sections that moved or merged send the reader on.
 */
export type SettingsGroup = "account" | "administration" | "operations";

export const GROUPS = [
  { id: "account", label: "settings.groupAccount" },
  { id: "administration", label: "settings.groupAdministration" },
  { id: "operations", label: "settings.groupOperations" },
] as const satisfies readonly { id: SettingsGroup; label: string }[];

export const SECTIONS = [
  {
    id: "account",
    group: "account",
    label: "settings.account",
    href: "/settings/account",
    admin: false,
  },
  {
    id: "tokens",
    group: "account",
    label: "settings.tokens",
    href: "/settings/tokens",
    admin: false,
  },
  {
    id: "keys",
    group: "account",
    label: "settings.keys",
    href: "/settings/keys",
    admin: false,
    // Its other view, the shared keys: the section itself, not a page inside it (17.3).
    views: ["/settings/keys/shared"],
  },
  {
    id: "profiles",
    group: "account",
    label: "settings.profiles",
    href: "/settings/agent-profiles",
    admin: false,
  },
  {
    id: "overview",
    group: "administration",
    label: "settings.overview",
    href: "/settings",
    admin: true,
  },
  {
    id: "people",
    group: "administration",
    label: "settings.users",
    href: "/settings/people",
    admin: true,
  },
  {
    id: "models",
    group: "administration",
    label: "settings.models",
    href: "/settings/models",
    admin: true,
  },
  {
    id: "environments",
    group: "administration",
    label: "settings.environments",
    href: "/settings/environments",
    admin: true,
  },
  {
    id: "connectors",
    group: "administration",
    label: "settings.connectors",
    href: "/settings/connectors",
    admin: true,
  },
  {
    id: "decisionEngines",
    group: "administration",
    label: "settings.decisionEngines",
    href: "/settings/decision-engines",
    admin: true,
  },
  {
    id: "internetAccess",
    group: "administration",
    label: "settings.egress",
    href: "/settings/internet-access",
    admin: true,
  },
  {
    id: "limits",
    group: "administration",
    label: "settings.limits",
    href: "/settings/limits",
    admin: true,
  },
  {
    id: "usage",
    group: "operations",
    label: "settings.usage",
    href: "/settings/usage",
    admin: true,
  },
  {
    id: "status",
    group: "operations",
    label: "settings.status",
    href: "/settings/status",
    admin: true,
  },
] as const satisfies readonly {
  id: string;
  group: SettingsGroup;
  label: string;
  href: string;
  admin: boolean;
  views?: readonly string[];
}[];

export type SettingsSection = (typeof SECTIONS)[number]["id"];

/** The addresses of sections that moved or merged, and where they are now. */
export const MOVED: Readonly<Record<string, string>> = {
  "/settings/git": "/settings/keys",
  "/settings/connector-credentials": "/settings/keys",
  "/settings/secrets": "/settings/keys/shared",
  "/settings/users": "/settings/people",
  "/settings/egress": "/settings/internet-access",
  "/settings/profiles": "/settings/agent-profiles",
};

/** Where an old address is now, a path below it kept; null when it didn't move. */
export function movedTo(path: string): string | null {
  for (const [from, to] of Object.entries(MOVED))
    if (path === from || path.startsWith(`${from}/`))
      return `${to}${path.slice(from.length)}`;
  return null;
}
