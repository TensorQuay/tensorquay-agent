import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { LANGUAGES } from "../i18n/generated/languages";
import { catalogue } from "../i18n/translate";

import { toolSummary } from "./tool-summary";
import type { ToolView } from "./types";

const read = (path: string, result: string | null, args = {}): ToolView => ({
  id: "read",
  name: "read_file",
  arguments: { path, ...args },
  status: result === null ? "running" : "ok",
  output: "",
  result,
  ran: true,
  startedAt: null,
  durationMs: 11,
  exitCode: null,
});

const LOCALES = join("src", "i18n", "locales");

const PAGES_1_2 =
  "[PDF, 300 pages]\n--- page 1 ---\nPart 1 of the long report\n12\tnot a line number\n" +
  "--- page 2 ---\nPart 2 of the long report\n" +
  "[partial: pages 1-2 of 300; continue with cursor c1 or start_line=3]";

// WEB-U-482: a PDF read's step names the pages it read, as the result's page marks say, never lines.
describe("WEB-U-482 a PDF read says pages", () => {
  it("names the pages a PDF result holds", () => {
    expect(toolSummary(read("/workspace/uploads/report.pdf", PAGES_1_2))).toBe(
      "Read uploads/report.pdf pages 1–2",
    );
    expect(
      toolSummary(
        read(
          "report.pdf",
          "[PDF, 300 pages]\n--- page 150 ---\nOn the ridge\n[end of document: 300 pages]",
        ),
      ),
    ).toBe("Read report.pdf page 150");
  });

  it("goes by the result, not the file's name", () => {
    // A PDF saved without the extension is still read as pages.
    expect(toolSummary(read("uploads/scan", PAGES_1_2))).toBe(
      "Read uploads/scan pages 1–2",
    );
    // A text file that happens to end in .pdf comes back line-numbered: lines.
    expect(
      toolSummary(read("notes.pdf", "     1\tfirst\n     2\tsecond\n")),
    ).toBe("Read notes.pdf lines 1–2");
  });

  it("says pages for a PDF read still running, from its arguments", () => {
    expect(
      toolSummary(
        read("uploads/report.PDF", null, { start_line: 4, end_line: 6 }),
      ),
    ).toBe("Read uploads/report.PDF pages 4–6");
    expect(
      toolSummary(read("main.py", null, { start_line: 4, end_line: 6 })),
    ).toBe("Read main.py lines 4–6");
  });

  it("has the page words in every catalogue", () => {
    for (const { code } of LANGUAGES) {
      const { messages } = catalogue(
        JSON.parse(
          readFileSync(join(LOCALES, `${code}.json`), "utf8"),
        ) as unknown,
      );
      expect(messages["tool.readPage"], code).toMatch(/\{first\}/);
      expect(messages["tool.readPages"], code).toMatch(/\{first\}.*\{last\}/);
      expect(messages["tool.readPages"], code).not.toBe(
        messages["tool.readLines"],
      );
    }
  });
});
