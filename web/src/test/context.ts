import type { Event } from "../protocol/generated/protocol";

type ContextLoaded = Extract<Event, { type: "context.loaded" }>;

/** A run's context report with every kind of entry: a truncated file, both tiers and a shadowed skill (WEB-U-046). */
export const contextLoaded: ContextLoaded = {
  type: "context.loaded",
  instructions: [
    { path: "AGENTS.md", truncated: false },
    { path: "services/billing/AGENTS.md", truncated: true },
  ],
  skills: [
    {
      name: "release-notes",
      tier: "install",
      description: "Drafts release notes from the merged changes.",
    },
    {
      name: "pdf-report",
      tier: "user",
      description: "Builds a PDF report from Markdown files.",
    },
  ],
  skipped: [
    { folder: "pdf-report", tier: "install", reason: "shadowed" },
    { folder: "scratch", tier: "user", reason: "missing_manifest" },
  ],
};

/** A later run's smaller report, so a test can tell the two apart. */
export const laterContext: ContextLoaded = {
  type: "context.loaded",
  instructions: [{ path: "AGENTS.md", truncated: false }],
  skills: [],
  skipped: [],
};
