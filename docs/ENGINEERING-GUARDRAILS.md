# TensorQuay Agent: Engineering Guardrails

Status: **draft v0.1 for Product Owner review** (22 Sep 2026). This document is the engineering practice standard for
the repository: universal principles, size limits, the testing standard, and language-specific rules for Rust, Go,
TypeScript and React, SQL, shell and containers. [DEV-GUIDELINES](DEV-GUIDELINES.md) holds the project-specific design
rules and the CI gates; [AGENTS.md](../AGENTS.md) holds the git and content guardrails. Where this document says
**must**, CI or review enforces it; **should** is the default that a reviewer may waive with a written reason in the PR.

## 0. How to read this document
| Word | Meaning | Enforcement |
|---|---|---|
| must | A rule. A change that breaks it is not merged | CI gate where possible, otherwise review checklist |
| should | The default practice | Review; deviation needs a one-line reason in the PR |
| may | An option | – |

## 1. Universal principles
### 1.1 Single responsibility
- A crate, package, module, file, type and function each do **one thing** that can be named in one short sentence.
  If the name needs "and", split it.
- A file holds one primary type or one cohesive group of free functions. Helper types that exist only for that file
  may live with it.
- A function does one step of one algorithm at one level of abstraction. Mixing I/O, parsing and business rules in one
  function is a review failure.

### 1.2 Modularity and dependency direction
- Dependencies point **downward only** across layers (ARCHITECTURE §3, gate G-07). Lower layers never know about
  higher ones; shared types live in the lowest layer that needs them.
- Boundaries are **interfaces** (traits, Go interfaces, TypeScript types), owned by the consumer, small (one to five
  methods), and mockable with a fake.
- No global mutable state. Configuration and dependencies are passed in at construction time.
- No "util", "common", "misc" or "helpers" modules. Name the responsibility.

### 1.3 Explicit over implicit
- Parse and validate at the boundary once, into typed values; the interior takes only validated types.
- No magic numbers or strings: named constants with a comment saying where the value comes from.
- Defaults are written down in one place (the configuration schema), not scattered in code.
- Behaviour that depends on the environment (model route, backend, host OS) is a declared property, never detected
  by sniffing.

### 1.4 Robustness
- **Fail closed.** On doubt, deny the action, reject the input, refuse to start.
- **Bounded everything:** input size, output size, queue length, retries, concurrency, recursion depth, timeouts.
  Every bound is a named configuration value with a documented default.
- **Every I/O has a timeout** and observes cancellation.
- **Idempotent operations** where a retry is possible (create, destroy, append with a sequence, migrations).
- **Retries** use exponential back-off with jitter and a maximum; only retryable errors are retried.
- **Partial failure is designed, not discovered:** what happens when the database, the model server, the sandbox or
  the client disappears is written in the contract and tested (fault injection).
- **Crash safety:** durable state is written before it is acknowledged; recovery is by replay, never by guessing.

### 1.5 Errors
- Errors are **typed** per crate or package, carry a **stable code** that can appear on the wire, and are documented.
- Errors are never swallowed. Either handle, or return with context, or log at the boundary once. Never both log and
  return the same error at every level.
- No panics, `unwrap`, `expect`, `panic!`, `unreachable!` or uncaught exceptions in library code. Binaries may fail
  fast at start-up on invalid configuration.
- Error messages say what was attempted, what was expected and what was found; they never include secrets.

### 1.6 Size limits
| Unit | Limit | Kind |
|---|---|---|
| Source file | ≤ 1000 lines (gate G-06) | must |
| React component file | ≤ 400 lines | should |
| Function or method | ≤ 80 lines; ≤ 5 parameters (use a struct or options object beyond that) | should |
| Cyclomatic complexity per function | ≤ 15 | should (lint where available) |
| Nesting depth | ≤ 4 | should |
| Public API surface of a module | ≤ 20 exported items; otherwise split | should |
| Pull request | ≤ 600 changed lines excluding generated files and fixtures; one concern | should |
| Test function | One behaviour; ≤ 60 lines | should |

### 1.7 Naming
- Names say what a thing is or does, in full words; no abbreviations except universally known ones (`id`, `url`, `db`).
- Booleans read as predicates (`is_ready`, `has_secret`); functions read as verbs; types as nouns.
- Units in names when a number has a unit (`timeout_ms`, `max_bytes`).
- One name per concept across the code, the protocol, the database and the documents.

### 1.8 Comments and documentation
- Comments explain **why**, constraints and invariants, not what the code does.
- Every public item has a doc comment stating its guarantee and its errors.
- Each crate or package has a README with: responsibility, the contract it implements, how to test it.
- A decision that a reader could reasonably question gets an ADR (`docs/adr/`).

### 1.9 Immutability and purity
- Prefer immutable values and pure functions; isolate side effects at the edges.
- Derived data is computed, not stored, unless measured to be a bottleneck.

### 1.10 Concurrency
- Every spawned task, goroutine or worker has an owner that joins or cancels it. No fire-and-forget.
- Channels and queues are bounded; back-pressure is explicit.
- Shared mutable state needs a written reason; prefer message passing or ownership transfer.
- Locks are never held across an await, an I/O call or a callback.

### 1.11 Security by default
- Least privilege for every process, container, token and database role.
- Secrets never in code, logs, error messages, URLs, events or test fixtures; redaction is on by default.
- Inputs from a model, a user, a repository or the network are untrusted; validate, bound and canonicalise.
- New egress, new mounts and new privileges need a test in the security suite.

### 1.12 Observability
- Structured logs with a stable set of fields (`conversation_id`, `run_id`, `step`, `tool`, `route`).
- Spans at every boundary; metrics for every queue, retry and failure path.
- Log levels: `error` needs action; `warn` is unexpected but handled; `info` is a state change; `debug` is for
  developers and is never on by default in production.

### 1.13 Configuration
- One validated schema per component; unknown keys are errors; defaults documented; secrets from the environment only.
- No feature detection at runtime that changes behaviour silently; feature flags are explicit and listed.

### 1.14 Performance discipline
- Measure before optimising; keep the measurement as a performance test (TEST-PLAN §15).
- No allocation or I/O in hot loops without a measurement that shows it is fine.
- Regressions above 10 % on a tracked measurement block the merge.

### 1.15 Dependencies
- Licences per ADR-0017 (gate G-05 and the web licence check): permissive by default, MPL-2.0 only as an unmodified
  dependency, CC-BY-4.0 and CDLA-Permissive-2.0 for data, never GPL, LGPL, AGPL, SSPL or BUSL. The policy covers code
  the product links, bundles or vendors, not infrastructure it talks to over a socket. A new
  dependency needs: a one-line reason, a maintained upstream, no overlapping existing dependency, and a check of its
  transitive footprint.
- Pinned versions with lockfiles; upgrades in their own PR with the changelog read.
- Vendoring is not allowed; forks are a last resort with an ADR and an upstream plan.

## 2. Testing standard
### 2.1 Levels and pyramid
Unit and contract tests are the bulk; integration tests prove real wiring once; end-to-end tests cover the critical
user paths; performance, soak and security suites run on a schedule and before releases (TEST-PLAN §2).

### 2.2 Coverage thresholds
| Scope | Threshold | Kind |
|---|---|---|
| Pure Rust crates (`tqa-types`, `tqa-llm`, `tqa-context`, `tqa-tools`, `tqa-prompt`, `tqa-core`) | ≥ 90 % lines (gate G-04); ≥ 80 % branches target | must / should |
| Parsers, validators, protocol codecs, permission rules, path confinement | 100 % lines and branches | must |
| Runtime, sandbox and gateway crates | Reported; no regression; ≥ 80 % target | should |
| Web stores, reducers, utilities | ≥ 80 % statements | should |
| Web components | Behaviour tests for every interactive component | must |
| Critical user paths | One end-to-end test each (TEST-PLAN §13) | must |

Measuring the 100 % class on the pinned stable toolchain: `cargo llvm-cov` branch counts need a nightly compiler, so
lines **and regions** must be 100 %, and `cargo mutants` on the touched module (§2.7) stands in for branches: every
surviving mutant is killed by a new test or documented as equivalent. No nightly toolchain is added for this.

Coverage is a floor, not a goal: a test that executes code without asserting behaviour does not count in review.

### 2.3 Test design
- **Arrange, act, assert**; one behaviour per test; the name states the behaviour and the condition
  (`cancel_during_tool_kills_process_group`).
- **Table-driven** tests for input matrices; **property-based** tests for parsers, serialisers and invariants;
  **fault injection** for every failure path in the contract.
- **Deterministic:** fake clock, seeded generators, no real network, no sleeps, no dependence on test order; parallel
  by default. Real time is allowed only where a real process must be observed (a process group being killed), with a
  bound and a comment saying why.
- **Edge cases are listed before the happy path** when designing a test file: empty, one, many, maximum, over the
  maximum, malformed, partial, concurrent, cancelled, crashed.
- **Assert on outcomes**, not on implementation details; a refactor that keeps behaviour must not break tests.
- Every test references its TEST-PLAN ID in its name or a comment when it implements a designed case.

### 2.4 Fixtures and goldens
- Recorded from real systems once, scrubbed of secrets and personal data, committed, and versioned.
- A golden change is reviewed as a behaviour change with a written reason.
- Fixture generators are checked in; fixtures are regenerated by a command, never edited by hand.

### 2.5 Test doubles
- Prefer **fakes** (a small working implementation) to mocks (expectation scripts). Mocks are allowed for a single
  interaction check.
- Every boundary interface ships with its fake in the same crate or in `tqa-testkit`.

### 2.6 Flaky tests
- A flaky test is a bug. Quarantine with an issue and a deadline; a quarantined P0 test blocks the next gate.
- Retrying a test to make it pass is forbidden in CI configuration.

### 2.7 Mutation testing
- Parsers, validators and permission logic should be checked with mutation testing before a release; surviving
  mutants are either killed by a new test or documented as equivalent.

## 3. Rust
### 3.1 Toolchain and lints
- Stable toolchain pinned in `rust-toolchain.toml`; edition 2024.
- `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` with these lints enabled workspace-wide:
  `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::todo`, `clippy::unimplemented`,
  `clippy::dbg_macro`, `clippy::print_stdout`, `clippy::print_stderr` (binaries may allow the last two),
  `clippy::large_enum_variant`, `clippy::missing_errors_doc`, `clippy::missing_panics_doc`,
  `clippy::cast_possible_truncation`, `rust_2018_idioms`, `unused_must_use`, `missing_docs` for public items.
- `#![forbid(unsafe_code)]` in every crate except the audited modules listed in `deny.toml` comments, which use
  `#![deny(unsafe_code)]` with `#[allow]` per block and a `// SAFETY:` comment.

### 3.2 Crate and module structure
- One responsibility per crate (ARCHITECTURE §3). `lib.rs` re-exports the public API and nothing else.
- No `mod.rs` files; `foo.rs` plus `foo/` for children. One primary type per file.
- Unit tests in a `tests` submodule of the file; contract and integration tests in the crate's `tests/`.
- Public API is minimal: `pub(crate)` by default; `pub` only for the crate's contract.

### 3.3 Errors
- One `Error` enum per crate with `thiserror`; variants carry context, not strings; a `code()` method returns the
  stable wire code.
- `anyhow` only in binaries. `?` with `.context()` at the boundary where the operation is named.
- Never `unwrap`/`expect` outside tests; use `let ... else`, `ok_or`, or a typed error.

### 3.4 API design
- Newtypes for identifiers (`ConversationId`, `RunId`, `Seq`); never bare `String` or `u64` on a public API.
- Builders for constructors with more than four inputs; `#[must_use]` on builders and results.
- Accept `impl AsRef<Path>`, `impl Into<String>` and `&[T]` where it helps callers; return concrete types.
- `serde` types at boundaries use `#[serde(deny_unknown_fields)]` for configuration and `rename_all = "snake_case"`
  for the wire; wire enums are `#[non_exhaustive]` and versioned.
- Time is `std::time::Instant` for durations and a `Clock` trait (fakeable) for wall-clock reads.

### 3.5 Async
- `tokio` runtime; blocking work in `spawn_blocking`; no `std::thread::sleep` in async code.
- Every task is spawned through an owner (`JoinSet` or a supervisor) and cancelled through a `CancellationToken`.
- `tokio::select!` arms are cancellation-safe or documented as not; prefer `biased;` when order matters.
- Every network and file operation is wrapped in `tokio::time::timeout`.
- Channels are bounded; `mpsc::channel(n)` with `n` from configuration; senders handle `Closed`.
- Locks across `.await` are forbidden; use `tokio::sync` primitives or actor-style ownership.

### 3.6 Logging and metrics
- `tracing` with `#[instrument(skip(...))]` on boundary functions; fields are structured, never formatted strings.
- Metrics through one `metrics` facade per crate; names follow `tqa_<subject>_<unit>`.

### 3.7 Testing in Rust
- `cargo nextest` for execution; `proptest` for properties; `cargo llvm-cov` for coverage; `cargo mutants` for
  parsers before releases.
- Golden files under `tests/fixtures/`, checked with `insta` or plain comparison; a snapshot update is a reviewed
  change.
- Tests that need Docker, PostgreSQL or a KVM host are `#[ignore]`d with a reason naming the dependency, so a default
  run reports them as ignored instead of passed; their suites run with `--run-ignored all` where the dependency
  exists; a helper invoked without the dependency fails loudly rather than returning early. A test that returns
  success without exercising its subject is a defect.

### 3.8 Performance
- `criterion` benches for hot paths (protocol encode and decode, journal append, token counting), tracked in CI as
  informational and gated in TEST-PLAN §15.

## 4. Go (no Go code in v1.0; this section applies to any future Go component)
### 4.1 Toolchain and lints
- Pinned Go version in `go.mod`; `gofmt` and `goimports` clean; `go vet`; `staticcheck`; `golangci-lint` with
  `errcheck`, `govet`, `staticcheck`, `gosimple`, `unused`, `ineffassign`, `gocritic`, `gosec`, `revive`,
  `errorlint`, `bodyclose`, `contextcheck`, `gocyclo` (limit 15) and `lll` (limit 120).
- `go test -race` on CI always.

### 4.2 Package structure
- Packages by responsibility (`sessions`, `events`, `relay`), never `util` or `common`; `internal/` for everything
  not meant to be imported.
- `cmd/<binary>/main.go` wires dependencies and nothing else.
- Interfaces are declared where they are consumed, are small, and have a fake in a `<pkg>test` package.

### 4.3 Errors and context
- Every error is checked. Wrap with `fmt.Errorf("doing x: %w", err)`; sentinel and typed errors for callers that
  branch; `errors.Is` and `errors.As`, never string matching.
- `context.Context` is the first parameter of every function that does I/O or may block; deadlines and cancellation
  are honoured; contexts are never stored in structs.
- No `panic` outside `main` start-up; no `log.Fatal` in libraries.

### 4.4 Concurrency
- Goroutines are owned: `errgroup` or an explicit supervisor; every goroutine has a documented exit condition.
- Bounded channels; `select` with `ctx.Done()` in every blocking send or receive.
- `goleak` in tests for packages that spawn goroutines.

### 4.5 Data and I/O
- Database access through a query layer with compile-time checked queries and contexts; transactions are explicit
  and short.
- HTTP handlers are thin: decode, validate, call a service, encode; middleware for auth, limits and logging.
- Structured logging with `slog`; no `fmt.Println` outside `cmd`.

### 4.6 Testing in Go
- Table-driven tests with `t.Run` and `t.Parallel`; `testing/fstest` and `httptest` for fakes; golden files under
  `testdata/`.
- Coverage ≥ 85 % on packages with logic; handlers covered by contract tests against the OpenAPI file.

## 5. TypeScript, React and Next.js
### 5.1 Toolchain and lints
- Strict TypeScript (`strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`); no `any` outside generated
  code; `unknown` at boundaries with validation.
- ESLint with the type-checked TypeScript rules, `react-hooks`, `jsx-a11y` and import ordering; a formatter; both
  clean in CI.
- Protocol and API types are generated from the schemas (gate G-08); hand-written copies are a review failure.

### 5.2 Structure
- Folders by feature (`conversation/`, `approvals/`, `admin/`), each with components, hooks, store slices and tests
  together; shared primitives in `ui/`.
- Components are either **presentational** (props in, elements out, no data fetching) or **containers** (wire data
  and callbacks); a file holds one exported component.
- Hooks encapsulate one concern and start with `use`; effects are minimal and always list their dependencies.

### 5.3 State
- Server state through TanStack Query with typed query keys and stale times; never copied into local state.
- Live transcript state in one store keyed by `seq`; reducers are pure and unit-tested.
- Derived values are computed with memoised selectors, not stored.
- Props are passed at most two levels; beyond that use context or the store.

### 5.4 Robustness in the browser
- Every fetch has a timeout and a typed error; every socket has back-off and a resync path.
- Error boundaries around each major panel; a failed panel never blanks the page.
- Loading, empty, error and partial states are designed for every data view.
- No secrets in the client bundle; only `NEXT_PUBLIC_` values are public by construction.
- No `dangerouslySetInnerHTML` without a sanitiser; markdown rendering uses an allow-list.

### 5.5 Design system
- One visual system, defined in [UI-GUIDELINES](UI-GUIDELINES.md): tokens in one CSS file, one implementation per
  component in `src/ui/`, brand assets from `brand/`, a `/design` reference page with a visual regression test.
- No colour, font or spacing literal outside the token file; no second copy of a brand asset; no third-party
  component library.

### 5.6 Accessibility and performance
- Every interactive element is keyboard-operable and labelled; focus is managed on dialogs and route changes;
  automated accessibility checks run in tests.
- Long lists are virtualised; expensive work is memoised only after a measurement; no heavy work in render.
- Bundle size is tracked per route; a regression above 10 % needs a reason.

### 5.7 Testing in TypeScript
- Unit tests for stores, reducers and utilities; component tests with a testing library that queries by role and
  text, never by implementation details; end-to-end tests for critical paths with Playwright.
- Tests are deterministic: fake timers, fake sockets, seeded data; no network.
- Snapshot tests only for stable markup and reviewed like goldens.

## 6. SQL and migrations
- Forward-only migrations; one logical change per file; idempotent where the engine allows; tested against the
  previous release's schema (GW-I-016).
- Destructive changes (drop, type narrowing) ship in two releases: first stop writing, then remove.
- Every foreign key has an index; every table has `created_at` and `updated_at` in UTC; identifiers are UUIDs.
- `jsonb` columns have size caps and are validated by the application; no logic in triggers.
- Queries are compile-time checked; no string-built SQL.

## 7. Shell, containers and CI
- Shell scripts: `#!/usr/bin/env bash`, `set -euo pipefail`, `shellcheck` clean, quoted variables, no `eval`,
  `trap` for cleanup; scripts over 100 lines become a real program.
- Dockerfiles: multi-stage; base images pinned by digest; non-root user; no secrets in any layer or build argument;
  `.dockerignore` present; one process per container; health checks defined.
- Images are reproducible from the repository at a commit; digests are recorded in release notes (gate G-15).
- CI: pinned action versions; no secrets echoed; every job reproducible locally with one command; tier 1 under 15
  minutes.

## 8. Protocol and API design
- Additive changes only within a version; removal or semantic change bumps the version and keeps the old one for one
  release.
- Every message has a schema; every field is documented; unknown fields are rejected at boundaries that must be exact
  and ignored where forward compatibility is intended, and the choice is written down per message.
- Errors carry a stable `code`, a human `message` and `retryable`.
- Lists are paginated; writes that may be retried carry an idempotency key or a sequence; every request has a size
  cap and a timeout; back-pressure is explicit.

## 9. Definition of done and review
A change is done when:
1. the contract and tests exist and pass, with new tests referencing their TEST-PLAN IDs;
2. lints, coverage, licence, size and dependency-direction gates pass;
3. documentation (protocol reference, admin guide, README of the crate) is updated in the same PR;
4. the PR describes what changed, why, and how it was verified;
5. a reviewer has checked the DEV-GUIDELINES §7 checklist and this document's size limits and robustness rules;
6. for a milestone, the technical lead has accepted it independently with an evaluation report.

## 10. Enforcement map
| Rule | Tool | Gate |
|---|---|---|
| Formatting, lints | `cargo fmt`, `clippy`, ESLint, `golangci-lint`, `shellcheck` | G-01, G-02 |
| Coverage | `cargo llvm-cov`, web coverage report | G-04 |
| Licences, advisories | `cargo deny`, npm audit and licence check; `tools/check-images.py` for third-party images in the compose files (SEC-030); `tools/check-zip-reads.py` while zip advisories are ignored (SEC-063) | G-05, G-05-images, G-05-zip |
| File size | `tools/check-file-length.py` | G-06 |
| Dependency direction | `tools/check-dep-direction.py` | G-07 |
| Schema sync | `export-schema --check` (Rust); web type generation diff | G-08 |
| Test ID references, doc links | `tools/check-docs.py` (offline links, exact IDs, Gate 1 P0 report) | G-09 |
| Named documents exist | `tools/check-docs-complete.py` (cited ADRs, testkit fakes, the documents PRD NFR-8 names, guide sections the copy names, the protocol reference against the schema) | G-09-complete |
| Secrets, personal data, attribution | `tools/git-guard.py audit`, `tools/check-secrets.py` (Gitleaks 8.30.1, working tree and full history) | G-10 |
| Function size, complexity, nesting | lints where available; otherwise review | review |
| Robustness, security, naming | review checklist | review |
