# Responsibility

System prompt assembly, instruction-file discovery, the skills directory and the tool-call fallback grammar.

# Contract

Implements [ARCHITECTURE](../../docs/ARCHITECTURE.md) sections 3, 7, 8 and 15.
The versioned system prompt contains stable workspace facts and an environment block with the template and
canonical capabilities. The image digest is excluded: repinning the same environment preserves the prompt bytes.
An environment with the `data` capability also gets a line saying Python's pandas, matplotlib and openpyxl are
installed, so the model uses them rather than installing (version 8, PROMPT-U-017).
The Network lines tell the model to explain a blocked site to the person in plain words and point to an
administrator (Settings > Internet access), never the proxy's status (version 9, PROMPT-U-018).
The open-web rule `*` reads as "any website on ports 80 and 443" (or its own ports), never as the pattern
(version 10, PROMPT-U-009).
A run with host keys (ADR-0037) gets Keys lines: each key's variable, address and scheme, and how to send it (the
Authorization header only; plain git for a git address), never its placeholder; a run without keys gets none
(version 11, PROMPT-U-019).
With the `browser` capability, a line says how to make a PDF from HTML with Chromium (version 12, PROMPT-U-020); with
the browser and the search page among the allowed sites (any website, `html.duckduckgo.com` or `*.duckduckgo.com`, on
443), another says how to search the web with curl, then Chromium, and to read the results with `fetch_url`; there is
no search tool (version 13, PROMPT-U-021). Version 14 puts Wikipedia's search API first for facts (its JSON: titles and
snippets) when `en.wikipedia.org` is allowed, gives Chromium the proxy without the URL's trailing slash (which it refuses), and says to report a
challenge page rather than retry.
Instruction discovery confines repository instruction files to the workspace and rejects symlink escapes.
Skills (PRD FR-11, PROMPT-U-002): `discover_skills` reads skill folders from the install and user tier roots; a
folder holds `SKILL.md`, whose front matter is exactly `name` (the folder name, `[a-z0-9-]`) and `description` (one
line, at most 280 characters). The user tier wins a name clash. Unusable folders are never fatal: each is recorded in
`SkillSet::skipped` with a stable `SkipReason` code (a missing or invalid manifest, a symlink anywhere, a manifest over
16 KiB or a folder over 1 MiB, non-UTF-8, shadowed, over the 64-skill limit, unreadable). `render_skill_index` gives
the byte-stable index for the instruction slot, one line per skill in name order, capped at 8 KiB with a marker.
`SkillTool` is the read-only `skill` builtin (ADR-0029, TOOL-U-024): it returns one discovered skill's instructions
and the absolute paths of up to 100 helper files, and refuses unknown names and folders changed since discovery.
`COMPACTION_PROMPT` is the fixed summary prompt for structured compaction. Its version,
`COMPACTION_PROMPT_VERSION`, equals the `version` of every `compaction` event, and any change to the text
needs a new version (ADR-0009).
The text protocol for tool calls (FR-2, ADR-0007): `FALLBACK_INSTRUCTIONS` (version `FALLBACK_PROMPT_VERSION`)
is added to the system prompt only for routes whose `native_tools` is false. A call is a `tqa_call` block with one
JSON object of exactly `name` and `arguments` (an object); `render_call` writes one on a single line. Blocks labelled
`tool_call`, which version 3 wrote, are read the same way (ADR-0007).
`parse_reply` splits a finished reply into prose and calls, using a caller-supplied `ToolCheck` (valid, invalid or
unknown), and applies five fixed repairs: trailing commas; bare keys and single quotes; a missing closing fence at
the end; a `json` or unlabelled block, or a whole reply, that is exactly `{name, arguments}` naming a known tool.
Ordinary JSON in an answer stays prose. Unreadable attempts are `FallbackFailure`s with stable codes; more than one
call is `too_many` unless the route allows parallel calls. `streamable_len` gives the prefix that can be shown before
the reply is complete.
On a native-tools route only the explicit forms count, plus a closed `json` fence that ends the reply and holds one
call to an offered tool whose arguments validate (F25, PROMPT-U-016); any other `json` fence is the answer.

# How to test

Run from the repository root:

```sh
cargo nextest run -p tqa-prompt
```
