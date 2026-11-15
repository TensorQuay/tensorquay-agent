import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { productSources, split } from "../../scripts/validators.mjs";

// WEB-U-472 (18.0 bundle room, WEB-U-014's headroom): the answers a task page's first load never decodes keep their
// validators out of the core module, each in an area the file that decodes it loads only when it is first asked for:
// a task's key uses, a desktop's screen notices, a run's changed files, a workspace folder's page, the accounts, one
// workspace (read, made, renamed or copied), an upload's answer, a failed request's error body, the task's queued
// messages and the models (both read once the page is shown, never with its first scripts).
const AREAS = [
  ["key-uses", "TaskKeyUses"],
  ["screen-notices", "ScreenNotices"],
  ["run-changes", "RunChanges"],
  ["file-pages", "FilePage"],
  ["accounts", "Accounts"],
  ["workspace", "Workspace"],
  ["upload", "Upload"],
  ["error", "Error"],
  ["queued-messages", "QueuedMessages"],
  ["models", "Models"],
] as const;

describe("WEB-U-472 page-only answers keep their validators off the first load", () => {
  it("puts each of them in its own area, not the core module, and the client's own file names none", async () => {
    const files = await productSources(process.cwd());
    const modules = split(files);
    const areaOf = new Map<string, string>();
    for (const [module, names] of Object.entries(modules))
      for (const name of names) areaOf.set(name, module);
    for (const [area, name] of AREAS) expect(areaOf.get(name), name).toBe(area);
    const client = readFileSync(
      join(process.cwd(), "src/api/client.ts"),
      "utf8",
    );
    for (const [, name] of AREAS)
      expect(client, `${name} decoded in client.ts`).not.toContain(`"${name}"`);
  });

  // WEB-U-470: a site pass is minted only by the files panel's page preview, which loads on demand, so its validator
  // travels with the previews' (src/api/previews.ts), never in the core module or decoded in the client's own file.
  it("keeps a site pass's validator with the previews, off the first load", async () => {
    const modules = split(await productSources(process.cwd()));
    const holding = Object.entries(modules)
      .filter(([, names]) => names.includes("SitePass"))
      .map(([area]) => area);
    expect(holding).toEqual(["previews"]);
    const client = readFileSync(
      join(process.cwd(), "src/api/client.ts"),
      "utf8",
    );
    expect(client).not.toContain('"SitePass"');
  });

  it("loads each area's validators only when its answer is first asked for (a dynamic import, never a static one)", () => {
    for (const [area] of AREAS) {
      const text = readFileSync(
        join(process.cwd(), `src/api/${area}.ts`),
        "utf8",
      );
      expect(text, area).toContain(
        `import("../protocol/generated/validators.${area}")`,
      );
      expect(text, area).not.toMatch(
        new RegExp(
          `^import "\\.\\./protocol/generated/validators\\.${area}"`,
          "m",
        ),
      );
    }
  });
});
