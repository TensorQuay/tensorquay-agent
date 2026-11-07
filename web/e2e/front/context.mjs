// WEB-U-046: what a scripted run loaded into the agent's context, sent straight after run.started (FR-11).

/** The first run's report: a truncated file, skills from both tiers, a shadowed skill and a folder without SKILL.md. */
const FIRST = {
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

/** A later run's report, so the panel's update is visible. */
const LATER = {
  type: "context.loaded",
  instructions: [{ path: "AGENTS.md", truncated: false }],
  skills: [],
  skipped: [],
};

/**
 * Inserts a context report after the run's start for a message that asks for one, renumbering what follows.
 * @param {string} text the user's message
 * @param {Array<{seq: number, event: {type: string}}>} values the run's frames in order
 */
export function withContext(text, values) {
  if (!text.startsWith("Load the context")) return values;
  const started = values.findIndex(
    (value) => value.event.type === "run.started",
  );
  const event = text === "Load the context again" ? LATER : FIRST;
  const report = { ...values[started], seq: values[started].seq + 1, event };
  return [
    ...values.slice(0, started + 1),
    report,
    ...values
      .slice(started + 1)
      .map((value) => ({ ...value, seq: value.seq + 1 })),
  ];
}
