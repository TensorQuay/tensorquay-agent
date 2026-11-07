# Catalogue of tested extensions

This catalogue lists MCP servers and sandbox templates that passed this project's conformance checks
(`tools/ext-check`, ARCHITECTURE §15.1). It is documentation, not code: nothing here is installed or enabled
automatically. An administrator copies an entry's configuration into their own install.

An entry claims one thing: at the commit and time it names, the extension passed ext-check as a sandbox would run it.
It is not an endorsement or a security review, and it says nothing about later versions of the extension.

## Layout

| Path | Contents |
|---|---|
| `mcp/<name>.md` | One MCP server, with its report `mcp/<name>.ext-check.json` next to it |
| `templates/<name>.md` | One sandbox template, with its report `templates/<name>.ext-check.json` next to it |
| `routes/` | Presets for model routes (ARCHITECTURE §6); none yet |

## Adding an MCP server

1. Build or pull a template image that ships the server at an absolute path outside `/workspace`, `/run/tqa` and
   `/tmp`, then run it from a clean checkout:
   `tools/ext-check mcp stdio --name <name> --template <image> --json docs/catalog/mcp/<name>.ext-check.json -- <command> [args...]`.
   Add `--calls <file.json>` to include calls with known answers.
   `--template` runs the server in that image, as a sandbox would, with no network; the image must be pinned by digest
   or image ID. `--host` checks a server under development on this machine instead, and its report can't be a
   catalogue entry.
2. Write `mcp/<name>.md` in the format below, copying the commit, date, tool list and hash from the report.
3. Run `tools/ext-check catalog lint docs/catalog` (the G-09-catalog gate in `tools/check.sh`).

## Checking a template

`tools/ext-check template <image ID or digest> --gateway-config <gateway.toml>` runs the template checks the gateway
runs (ADR-0032), then creates a sandbox from the image through the Docker backend and checks its posture and that its
runtime dials the gateway. It needs the install's gateway file (its templates are the bases, its `[sandbox.env]` the
sandbox environment). On a rootless Docker host, run it through `tools/rootless-test.sh`, which gives it the ownership
rights the gateway's container has. Nothing is pulled, and everything it creates is removed.

## Entry format

An entry starts with a heading and a sentence, then a table with these fields:

| Field | Value |
|---|---|
| Name | The server's name; the file is named after it |
| Kind | `mcp` |
| Source | Where the server's code comes from |
| Version | The version or pin that was tested |
| Transport | `stdio` |
| Tested | `ext-check <commit>, <date>`, exactly as in the report |
| Report | The report's file name |
| Tools SHA-256 | The report's `tools_sha256` |

Then three sections:

- **Configuration:** one `toml` block with exactly the server's `[[mcp.servers]]` entry, which the gateway's own
  validator must accept.
- **Tools:** a `| Tool | Model name |` table matching the report's tools, in its order. There is no permission class
  (ADR-0035), and an entry that still names one is refused.
- **Notes:** limits, warnings from the report, and anything an administrator should know.

The lint refuses an entry whose report is missing, not `ext-check/1`, not passing, made with network or outside a
template image, or made at an unknown or uncommitted state, and an entry that differs from its report.

## Template entry format

A template entry has the same heading and sentence, then a table with `Name`, `Kind` (`template`), `Source`,
`Version`, `Base` (the file-provided template it derives from), `Image` (the pin that was checked), `Tested` and
`Report`, and a **Configuration** section with one `toml` block holding exactly its `[[templates]]` entry, with that
image. Its report must be an `ext-check/1` template report for that image and base whose `template_checks`, `posture`
and `boot_ready` all passed (a skip, such as the microVM rootfs check on a host without KVM, is noted, not a pass).

