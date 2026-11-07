# ADR-0010: Web protocol types generated from JSON Schema

## Status

Accepted. Recorded on 2026-10-12 from what ships: the generator was in the repository's first import (23 Sep 2026)
and has been gated since (G-08, PROTO-C-002), but its record was never written.

## Date

2026-10-12

## Context

The wire protocol's types live in Rust (`tqa-types`), and the gateway's HTTP API is described by its OpenAPI document.
The web app reads both. Types written a second time by hand in TypeScript drift from the Rust ones without anyone
noticing, and a browser that trusts what arrives would render a malformed or future frame as if it were known. A
browser-side schema compiler (ajv at run time) needs `new Function`, which a strict Content-Security-Policy refuses,
and ships the compiler and every schema to every page.

## Decision

1. **The Rust types are the source.** `tqa-types` exports the protocol's JSON Schema,
   `crates/tqa-types/schema/tqa-protocol-v1.json` (and the decision engine's, `decision-v1.json`), with the
   `export-schema` binary; the gateway's API is `crates/tqa-gateway/openapi.json`.
2. **The web's types and validators are generated from them, never written by hand.** `pnpm protocol:gen`
   (`web/scripts/protocol.mjs`) writes `web/src/protocol/generated/`: `protocol.ts` from the protocol schema, `api.ts`
   from the OpenAPI document, the runtime schemas, and precompiled validators (`web/scripts/validators.mjs`), split by
   area like the copy catalogue so a page loads only the ones it decodes. The browser validates each frame and answer
   with that generated code; nothing is compiled in the page.
3. **Drift fails the build.** G-08 checks that the exported schemas match the Rust types (`export-schema --check`),
   and `pnpm protocol:check`, part of the web check, regenerates and compares the generated files, including a hash of
   every schema constraint (PROTO-C-002).

## Consequences

- A protocol or API change is one change in Rust, then `pnpm protocol:gen`; a forgotten regeneration fails
  `protocol:check`.
- The transcript reducer must handle every generated event variant exhaustively
  (`web/src/protocol/generated/README.md`).
- The generated files are committed and must not be edited by hand.
- The browser treats a few closed lists as open where a new value must not break an older page (the field problem's
  `reason`, in `protocol.mjs`), and says so in the generator.

## Links

- ARCHITECTURE §9 and §11; ADR-0005 (event sequence and replay); [the protocol reference](../protocol-reference.md)
- TEST-PLAN PROTO-C-001, PROTO-C-002; gate G-08
