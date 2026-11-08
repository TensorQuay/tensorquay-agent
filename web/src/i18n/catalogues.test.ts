import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { LANGUAGES } from "./generated/languages";
import { argumentNames, formatMessage } from "./message";
import { catalogue } from "./translate";

const LOCALES = join("src", "i18n", "locales");
const english = catalogue(
  JSON.parse(readFileSync(join(LOCALES, "en.json"), "utf8")),
);
const argumentsOf = argumentNames;
const sample = (pattern: string) =>
  Object.fromEntries(
    Array.from(argumentsOf(pattern), (name) => [
      name,
      name === "count" ? 2 : "x",
    ]),
  );

// WEB-U-019: every shipped catalogue is valid ICU and uses the same arguments as English.
describe("WEB-U-019 catalogues", () => {
  const files = readdirSync(LOCALES).filter((name) => name.endsWith(".json"));

  // A language is complete exactly when it translates every reference message, as `pnpm i18n:report` counts; the menu
  // marks the others partial (WEB-U-032).
  it("WEB-U-032 marks a language complete only when it has every reference message", () => {
    const reference = Object.keys(english.messages);
    for (const entry of LANGUAGES) {
      const messages = (
        JSON.parse(
          readFileSync(join(LOCALES, `${entry.code}.json`), "utf8"),
        ) as { messages: Record<string, unknown> }
      ).messages;
      const complete = reference.every(
        (key) => typeof messages[key] === "string",
      );
      expect({ code: entry.code, complete: entry.complete }).toEqual({
        code: entry.code,
        complete,
      });
    }
  });

  it("lists every catalogue file as a shipped language", () => {
    expect(LANGUAGES.map((entry) => `${entry.code}.json`).sort()).toEqual(
      [...files].sort(),
    );
  });

  // Landmarks that can share a page need distinct names in every language (axe landmark-unique).
  const LANDMARKS = [
    "shell.sidebar",
    "shell.navigation",
    "list.tasks",
    "live.region",
    "start.region",
    "bar.region",
    "live.transcript",
    "composer.region",
    "toast.region",
  ] as const;
  it.each(files)("%s names each landmark distinctly", (file) => {
    const source = catalogue(
      JSON.parse(readFileSync(join(LOCALES, file), "utf8")),
    );
    const names = LANDMARKS.map(
      (key) => source.messages[key] ?? english.messages[key],
    );
    expect(new Set(names).size).toBe(names.length);
  });

  // A catalogue no native speaker has reviewed says so, in one agreed wording the report and reviewers can find.
  it.each(files)("%s marks a draft only with the agreed wording", (file) => {
    const { status } = JSON.parse(
      readFileSync(join(LOCALES, file), "utf8"),
    ) as { status?: unknown };
    expect([undefined, "draft - native review welcome"]).toContain(status);
  });

  it.each(files)(
    "%s formats every message with English's arguments",
    (file) => {
      const source = catalogue(
        JSON.parse(readFileSync(join(LOCALES, file), "utf8")),
      );
      const code = file.replace(/\.json$/, "");
      for (const [key, pattern] of Object.entries(source.messages)) {
        const reference = english.messages[key];
        expect(reference, `${code}: unknown key ${key}`).toBeDefined();
        expect(argumentsOf(pattern), `${code}: ${key}`).toEqual(
          argumentsOf(reference ?? ""),
        );
        expect(() =>
          formatMessage(pattern, sample(pattern), code),
        ).not.toThrow();
      }
    },
  );
});

// WEB-U-215 (PRD §12 item 22, FR-19): nothing in the app is specific to git. A git host is an outside service with a key,
// like any other; the agent uses git inside its sandbox like curl or pip. No message names git, a repository, a project
// or a project link, in English or in the other catalogues, except where the page must still read what was stored:
const ALLOWED: Readonly<Record<string, string>> = {
  // The transcript still shows past git_push calls and their outcomes (ADR-0037 §7: git_push is retired from the
  // catalogue, but old tasks keep their history).
  "tool.pushing": "a past git_push call",
  "tool.pushed": "a past git_push call",
  "tool.push": "a past git_push call",
  "tool.fallbackProject": "a past git_push call",
  "summary.savedProjects": "a past git_push call",
  "activity.pushRejected": "a past git_push call",
  "activity.pushOutcomeUnknown": "a past git_push call",
  "activity.pushDenied": "a past git_push call",
  "activity.pushEgress": "a past git_push call",
  "activity.pushTooLarge": "a past git_push call",
  "activity.pushTimeout": "a past git_push call",
  // A real rule until the second release: the gateway still keeps people's stored Git credentials from shared routes
  // and connectors by this prefix (ADR-0037, "Two releases"); the message goes with the prefix.
  "secrets.reservedPrefix": "the git- prefix, retired in the second release",
  // Status counts the stored Git credentials that couldn't become keys, kept for this release (ADR-0037 Amendment 4).
  "health.gitKeptTitle": "stored Git credentials kept for one release",
  "health.gitKeptAddress": "stored Git credentials kept for one release",
  "health.gitKeptWaiting": "stored Git credentials kept for one release",
};
// 仓库 isn't among them: Chinese uses it for package and image registries too, which are no git feature.
const GIT_WORDS = [/\bgit\b/i, /repositor/i, /\bprojects?\b/i, /项目/];

describe("WEB-U-215 nothing in the app is specific to git", () => {
  it("names no git, repository, project or project link in any catalogue, outside the allowed stored history", () => {
    const named = readdirSync(LOCALES)
      .filter((name) => name.endsWith(".json"))
      .flatMap((file) => {
        const { messages } = JSON.parse(
          readFileSync(join(LOCALES, file), "utf8"),
        ) as { messages: Record<string, string> };
        return Object.entries(messages)
          .filter(
            ([key, text]) =>
              !(key in ALLOWED) && GIT_WORDS.some((word) => word.test(text)),
          )
          .map(([key]) => `${file} ${key}`);
      });
    expect(named).toEqual([]);
  });

  it("allows only messages the app still uses", () => {
    for (const key of Object.keys(ALLOWED))
      expect(english.messages).toHaveProperty([key]);
  });
});

// WEB-U-201 (a UI review before 17.0): a size reads the same everywhere. The app writes sizes as `formatSize` does, in
// binary steps named kB, MB and GB (WEB-U-185); a message naming KiB, MiB, GiB or TiB is another convention.
describe("WEB-U-201 one way to write a size", () => {
  it("names no binary unit in any catalogue", () => {
    const named = readdirSync(LOCALES)
      .filter((name) => name.endsWith(".json"))
      .flatMap((file) => {
        const { messages } = JSON.parse(
          readFileSync(join(LOCALES, file), "utf8"),
        ) as { messages: Record<string, string> };
        return Object.entries(messages)
          .filter(([, text]) => /\b[KMGT]iB\b/.test(text))
          .map(([key]) => `${file} ${key}`);
      });
    expect(named).toEqual([]);
  });
});
