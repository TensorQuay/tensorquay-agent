# ADR-0040: The PDF reader: one crate for the text by page and the pages as pictures

## Status

Accepted (the Product Owner's decision of 9 Nov 2026, PRD item 30; built for 18.0).

## Date

2026-11-15

## Context

Until 18.0 a PDF in the workspace read as `[binary file]`, and the agent had to convert it with a shell tool first,
which needs a converter in the image and a Python PDF library the Product Owner ruled out. PRD item 30 asks for a
reader built into the product, in Rust, on an Apache-compatible library, with no OCR where it can be avoided: every
model gets the text, and a route with `vision` gets the pages as pictures too.

The engine was chosen by a benchmark (`crates/tqa-pdf-bench`, never shipped): a corpus of our own documents in the ten
interface languages and four layouts through seven PDF producers, hard cases (encryption, passwords, forms, Type 3 and
unmapped fonts, object streams, incremental updates), damaged and hostile files, and 300- and 1,000-page documents,
scored for character error rate, search recall, page attribution, no-text detection, speed and robustness. oxidize-pdf
(MIT, pure Rust) was the only text engine within every accuracy bar; hayro (Apache-2.0 OR MIT, pure Rust, the standard
14 fonts built in) draws the pages. Both are pinned exactly and their licences re-checked on every change.

Two parts of the product need the reader: `read_file` in `tqa-tools` (the text) and the runner in `tqa-core` (the
pictures of the pages a result holds). ARCHITECTURE §3 lets dependencies point downward only (gate G-07), and a new
crate needs a record.

## Decision

1. **One crate, `tqa-pdf`, below `tqa-tools`.** It wraps the two engines and nothing else: `Document::open` on the
   file's bytes, `text(page)` cleaned and cut at 64 KiB, `render_pngs(pages)` drawing several pages from one open. It
   depends on no workspace crate; `tqa-tools` and `tqa-core` gain the edges `tqa-tools → tqa-pdf` and
   `tqa-core → tqa-pdf` (G-07's table), so the two readers are one implementation and the engines are named in one
   place. The benchmark's adapter for the product's reader is a binary of this crate, so every change to the reader is
   held to the engine's bars.
2. **Caps before the engines see the file.** What isn't a PDF, needs a password, has more than 2,000 pages, or nests
   arrays and dictionaries more than 256 levels deep is refused by name before parsing: the engine's parser recurses
   once per level, and a hostile file nested 100,000 deep would overflow the stack and abort the runtime process. A page
   is drawn at 144 DPI with its longest side capped at 2,000 px, so a poster or a hostile `MediaBox` never becomes a
   pixmap of gigabytes; the file itself is bounded by `read_file`'s 64 MiB. Both reads run off the async runtime.
3. **Text by page for every model.** `read_file` on a PDF answers `[PDF, N pages]`, then each page under
   `--- page N ---`, as many whole pages as fit the read bound, and a cursor for the next page; `start_line` and
   `end_line` are page numbers there. The result's summary and page marks are the contract the runner reads.
4. **Pictures only for a model that sees.** On a route with `vision`, the runner attaches the pages a PDF result holds
   as images, drawn once per run and kept by file version (path, length, modified time, page) within the images-per-run
   cap; a page that can't be drawn is left out, never an error to the run. A route without `vision` sees the text only.

## Consequences

- The agent reads PDFs on every environment with no converter, offline, inside the sandbox; the reader's accuracy and
  speed are the benchmark's bars, rerun on every engine change (`tqa-pdf-bench run --engine reader=...`).
- Two more exactly pinned engines in the product's dependency tree (THIRD-PARTY-NOTICES), with duplicate versions of a
  few small crates through them; `cargo deny` checks licences, bans and sources as for every dependency.
- The known limits are the engines': a form's field values and a font with no `ToUnicode` map read wrong or empty, as
  for every engine measured; a scan with no text layer reads as empty pages (no OCR, by the decision).
- `tqa-tools` and `tqa-core` build the two engines; `tqa-gateway` and `web/` are unchanged.

## Links

- PRD item 30; ARCHITECTURE §3 (the crate table, gate G-07); ENGINEERING-GUARDRAILS §2.2
- TEST-PLAN PDF-U-001, PDF-U-002, TOOL-U-145, CORE-U-092; the benchmark's bars in `crates/tqa-pdf-bench/README.md`
