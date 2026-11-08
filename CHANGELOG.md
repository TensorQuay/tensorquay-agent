# Changelog

Each release of TensorQuay Agent, newest first. A fix release changes no capability, migration or protocol
([DEV-PLAN §7](docs/DEV-PLAN.md)).

## 17.1 (7 November 2026)

A fix release: no migration, no protocol change.

### Fixed

- **Adaptive thinking on Messages routes (LLM-U-055, LLM-U-057).** Newer Messages models refuse a thinking budget.
  A Messages route whose reasoning policy has no `budget_tokens` now sends adaptive thinking with the request's
  effort, and refuses a temperature. Such a route sends no earlier thinking blocks back: the protocol binds each
  block to the conversation before it, and the agent's history changes between steps. Routes with budgets are
  unchanged.
- **The conversation is cached, not only its prefix (LLM-U-056).** With a Messages route's cache hint set, the latest
  message carries a cache breakpoint too.
- **A stalled accessibility tree never holds the screen (TOOL-U-138).** Each tree call is bounded (3 s; a whole
  window's walk 10 s). Past the bound the screenshot is still sent, without that application's elements.
- **No marks where the tree's positions are wrong (TOOL-U-139).** An element whose box lies outside the focused
  window's frame gets no mark; it stays listed for its accessibility actions.
- **Typed line breaks and tabs are keys (TOOL-U-140).** Text typed into a desktop application goes in runs with real
  Return and Tab keys between them.
- **Spreadsheet reading uses calamine 0.38.0** (RUSTSEC-2026-0436).

### Upgrade notes

- No migration. A route saved with adaptive thinking can't be read by 17.0: a gateway taken back to 17.0 refuses to
  start while such a route is stored. Delete those routes before going back.

## 17.0 (3 November 2026)

The initial release: an on-premise AI agent platform with sandboxed agents on Docker or Firecracker microVMs,
a web UI, a desktop environment for computer use, and channels where agents and people work together. What it does
and how to install it is in the [README](README.md).
