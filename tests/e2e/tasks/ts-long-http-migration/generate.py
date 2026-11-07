"""Regenerates this task's seed/, check/expected.json and dry-run/ deterministically: python3 generate.py.

The task is long on purpose: thirty-two TypeScript modules of about seventy-five lines call a deprecated helper in
three forms, and the new client takes one options object, so each call site changes shape. That is sized to need
compaction on a 32k window.
"""
import json
import pathlib
import random
import shutil

HERE = pathlib.Path(__file__).resolve().parent
MODULES = 32
FORMS = ("retries", "default", "constant")
LEGACY = '''/** Deprecated HTTP helper; use net/client.ts instead (docs/MIGRATION.md). */
export const DEFAULT_RETRIES = 3;

export interface RequestOptions {
  retries?: number;
}

export function request(url: string, options: RequestOptions = {}): { url: string; tries: number } {
  process.emitWarning("legacy/http.ts is deprecated; use net/client.ts", "DeprecationWarning");
  return { url, tries: options.retries ?? DEFAULT_RETRIES };
}
'''
CLIENT = '''/** The HTTP client that replaces legacy/http.ts. */
export const DEFAULT_ATTEMPTS = 3;

export interface SendOptions {
  url: string;
  attempts?: number;
}

export function send(options: SendOptions): { url: string; tries: number } {
  return { url: options.url, tries: options.attempts ?? DEFAULT_ATTEMPTS };
}
'''
MIGRATION = """# Migrating from legacy/http.ts to net/client.ts

`legacy/http.ts` is deprecated and will be removed. Every caller moves to `net/client.ts`:

| Before | After |
|---|---|
| `import { request } from "../legacy/http.ts";` | `import { send } from "../net/client.ts";` |
| `request(url)` | `send({ url })` |
| `request(url, { retries: n })` | `send({ url, attempts: n })` |
| `DEFAULT_RETRIES` | `DEFAULT_ATTEMPTS` |

The responses are identical, so behaviour must not change.
"""


def module(number, rng):
    form = FORMS[number % len(FORMS)]
    url = f"https://svc.example/e{number:02d}/orders"
    tries = {"retries": rng.randrange(1, 6), "default": 3, "constant": 3}[form]
    call = {"retries": f'request("{url}", {{ retries: {tries} }})', "default": f'request("{url}")',
            "constant": f'request("{url}", {{ retries: DEFAULT_RETRIES }})'}[form]
    imports = "request, DEFAULT_RETRIES" if form == "constant" else "request"
    fields = rng.sample(["id", "total", "customer", "region", "status", "placed", "items", "channel"], 4)
    scale = rng.randrange(2, 9)
    lines = [f"/** Endpoint e{number:02d}: reads /e{number:02d}/orders and summarises it for the weekly report. */",
             f'import {{ {imports} }} from "../legacy/http.ts";', "",
             f"export const FIELDS = {json.dumps(fields)};", f"const SCALE = {scale};", ""]
    for helper in range(6):
        verb = rng.choice(["filter", "normalise", "rank", "bucket", "clip", "weight"])
        lines += ["/**", f" * Step {helper + 1} of the summary: {verb} the values.", " *",
                  f" * The report for /e{number:02d} shows {', '.join(fields[:2])}; this step keeps the order of the input",
                  " * and drops missing values, so a partial response still yields a line.", " */",
                  f"export function helper{helper}(values: Array<number | null>): number[] {{",
                  f"  return values.filter((value) => value !== null).map((value) => (value as number) * SCALE + {helper});",
                  "}", ""]
    lines += ["export function describe(): string {", '  return FIELDS.join(", ");', "}", "",
              "export function run(): string {", f"  const response = {call};",
              "  const total = helper2(helper1(helper0([1, 2, null, 3]))).reduce((sum, value) => sum + value, 0);",
              "  return `${response.url}#${response.tries}:${total}:${describe()}`;", "}", ""]
    values = [((v * scale) * scale + 1) * scale + 2 for v in (1, 2, 3)]
    return "\n".join(lines), f"{url}#{tries}:{sum(values)}:{', '.join(fields)}"


def main():
    rng = random.Random(2026092603)
    seed = HERE / "seed"
    shutil.rmtree(seed, ignore_errors=True)
    for directory in ("src/legacy", "src/net", "src/app", "docs", "test"):
        (seed / directory).mkdir(parents=True)
    (seed / "package.json").write_text('{\n  "name": "report",\n  "private": true,\n  "type": "module"\n}\n')
    (seed / "src/legacy/http.ts").write_text(LEGACY)
    (seed / "src/net/client.ts").write_text(CLIENT)
    (seed / "docs/MIGRATION.md").write_text(MIGRATION)
    (seed / "README.md").write_text("# report\n\nThirty-two endpoint summaries under `src/app/`; HTTP helpers under `src/`.\n")
    expected = {}
    for number in range(MODULES):
        text, expected[f"e{number:02d}"] = module(number, rng)
        (seed / "src/app" / f"e{number:02d}.ts").write_text(text)
    (seed / "test/smoke.test.ts").write_text(
        'import assert from "node:assert/strict";\nimport { test } from "node:test";\n'
        'import { run } from "../src/app/e00.ts";\n\n'
        'test("the first endpoint names itself", () => {\n'
        '  assert.ok(run().startsWith("https://svc.example/e00/"));\n});\n')
    (HERE / "check" / "expected.json").write_text(json.dumps(expected, indent=1, sort_keys=True) + "\n")
    rewrite = ("sed -E -i "
               "-e 's#import \\{ request \\} from \"\\.\\./legacy/http\\.ts\";#import { send } from \"../net/client.ts\";#' "
               "-e 's#import \\{ request, DEFAULT_RETRIES \\} from \"\\.\\./legacy/http\\.ts\";#import { send, DEFAULT_ATTEMPTS } from \"../net/client.ts\";#' "
               "-e 's#request\\(\"([^\"]+)\", \\{ retries: DEFAULT_RETRIES \\}\\)#send({ url: \"\\1\", attempts: DEFAULT_ATTEMPTS })#' "
               "-e 's#request\\(\"([^\"]+)\", \\{ retries: ([0-9]+) \\}\\)#send({ url: \"\\1\", attempts: \\2 })#' "
               "-e 's#request\\(\"([^\"]+)\"\\)#send({ url: \"\\1\" })#' src/app/*.ts")
    rename = ("sed -E -i -e 's#import \\{ request(, DEFAULT_RETRIES)? \\} from \"\\.\\./legacy/http\\.ts\";#import { send, DEFAULT_ATTEMPTS } from \"../net/client.ts\";#' "
              "-e 's#request\\(#send(#' -e 's#DEFAULT_RETRIES#DEFAULT_ATTEMPTS#' src/app/*.ts")
    for name, command, final in (("solve", rewrite, "Moved every endpoint to net/client.ts with the options object."),
                                 ("wrong", rename, "Renamed request to send in every endpoint.")):
        scenario = {"model": "fixture", "responses": [
            {"events": [{"type": "tool_call", "name": "grep", "arguments": {"pattern": "legacy/http", "path": "src/app", "output_mode": "count"}, "split_into": 1}],
             "usage": {"prompt_tokens": 700, "completion_tokens": 40}},
            {"events": [{"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 2}],
             "usage": {"prompt_tokens": 2000, "completion_tokens": 160}},
            {"events": [{"type": "text", "text": final}], "usage": {"prompt_tokens": 2300, "completion_tokens": 20}}]}
        (HERE / "dry-run" / f"{name}.json").write_text(json.dumps(scenario, indent=1) + "\n")


if __name__ == "__main__":
    main()
