# TensorQuay Agent: Development Guidelines

Status: **draft v0.1 for Product Owner review** (22 Sep 2026). These rules apply to every change. Most are enforced in
CI (§6); the rest are checked in review (§7). The language-specific practice standard (Rust, Go, TypeScript and
React, SQL, shell, containers), the size limits and the testing standard are in
[ENGINEERING-GUARDRAILS](ENGINEERING-GUARDRAILS.md). Architecture is in [ARCHITECTURE](ARCHITECTURE.md). Related:
[PRD](PRD.md), [DEV-PLAN](DEV-PLAN.md), [TEST-PLAN](TEST-PLAN.md), [AGENTS.md](../AGENTS.md).

## 1. Design rules
### 1.1 Structure
1. Crates, responsibilities and the **downward-only dependency direction** are in ARCHITECTURE §3 (gate G-07). A new
   crate needs a decision record in `docs/adr/`.
2. **Pure crates** (`tqa-types`, `tqa-llm`, `tqa-context`, `tqa-tools`, `tqa-prompt`, `tqa-core`) have no Docker,
   Firecracker or database dependency, so they build and test on macOS and on a plain Linux CI runner.
3. **The agent loop is host-agnostic.** `tqa-core` receives an `EventSink`, a `ToolRegistry`, a `ModelProvider` and a
   `PermissionBroker`; it never opens sockets, spawns sandboxes or reads host configuration.
4. **Parse, don't validate.** Wire frames, registry files and configuration are parsed into validated types once at
   the boundary (`Frame::parse`, `Registry::load`); the rest of the code takes only validated types.
5. **Every subsystem has a fake** for the layer above: `FakeProvider` (scripted responses and faults),
   `FakeSandboxBackend` (in `tqa-sandbox`), `FakeGateway` (the runtime link and the model relay), `FakeClock`. Tests of
   a layer use fakes for the layers below it.
6. **Files ≤ 1000 lines** (gate G-06). Split by responsibility, not by line count.

### 1.2 Runtime
7. **No secrets in the sandbox.** Nothing that runs inside a sandbox receives a provider key, a database URL or a
   long-lived credential. The one credential a sandbox holds, its per-run token, travels as a read-only mounted
   file, never as an environment value, so it never enters a container-create request, an inspect output or a
   client library's debug log (ADR-0018). Reviewers check every new environment variable and mount against this rule.
8. **Every state change is an event** with a per-conversation `seq`. Persistence, the UI, resume and metering consume
   events; none of them has a private side channel.
9. **Write the journal before emitting.** The runtime appends an event to the journal, then sends it. A crash between
   the two is recovered by replay, never by guessing.
10. **Bounded everything.** Tool output, frame size, event payloads, journal records, HTTP bodies, directory listings,
    retries and queues all have caps that are configuration values with documented defaults.
11. **Cancellation is cooperative and prompt.** Every await in the loop and in tools observes the run's cancel token;
    a cancel is reflected in events within 200 ms (CORE-U-005).
12. **Configuration only in validated TOML.** Environment variables carry secrets and the config path only. Unknown
    keys are errors.
13. **Observability:** `tracing` spans at run, step and tool boundaries; Prometheus metrics; no `println!` in libraries;
    prompts and tool output are not logged by default.

### 1.3 Security
14. Paths from a model are canonicalised with symlinks resolved and checked against the workspace root before any
    file operation. Shell commands run in the sandbox. The loop never runs outside a sandbox; tests that need a
    real runtime start it in a Docker container.
15. Approvals bind to a prepared action (canonical JSON hash of tool, arguments and call context). The gateway records
    the binding when it stores the `permission.request`, accepts a client decision only against that record, and
    consumes it once; the runtime checks the hash again and consumes it once. There is no model-visible "approved"
    argument on any tool, and approval never widens builtin file confinement.
16. New network egress from a sandbox needs an allow-list rule and a test in SEC.
17. Dependencies: the licence policy of ADR-0017 (permissive by default; MPL-2.0 only unmodified; CC-BY-4.0 and
    CDLA-Permissive-2.0 for data; never GPL, LGPL, AGPL, SSPL or BUSL); `cargo deny` and the
    web licence check enforce it from `deny.toml` and `web/licenses.json`; new dependencies are justified in the PR.
18. `unsafe` is forbidden outside a short list of audited modules named in `deny.toml` comments; each block has a
    `SAFETY:` comment.

### 1.4 Local-model behaviour
19. Anything that depends on a model's behaviour (native tool calling, reasoning fields, cache hints, token limits)
    is a **route property in the registry**, not a code path keyed on a vendor name.
20. The system prompt and the tool list are byte-stable across the steps of one run so prefix caches hit
    (PROMPT-U-003).

### 1.5 Extension discipline
21. **Batteries are external.** No connector-specific or service-specific code in the core. A capability is added
    through an extension point (ARCHITECTURE §15): skills, MCP servers, agent profiles, sandbox templates, model
    routes. A new builtin tool needs an ADR that explains why an MCP server is not enough.
22. **Extensions never widen the trust boundary.** An extension runs either inside the sandbox without secrets, or
    outside it behind the gateway with credentials in the vault. A change that creates a third place is rejected.
23. **Profiles are configuration.** Anything a profile can express (tools, connectors, policy, template, budgets) is
    validated data with a schema, never code.

## 2. Code style
### 2.1 Rust
- Edition 2024, stable toolchain pinned in `rust-toolchain.toml`; `cargo fmt` and `cargo clippy -D warnings` clean.
- Errors: `thiserror` enums per crate with stable codes that appear on the wire (`error.code`); `anyhow` only in
  binaries.
- Async: `tokio`; no blocking calls in async contexts (use `spawn_blocking`); every spawned task is owned and joined
  or cancelled.
- Public types are documented; every `pub` item has a doc comment saying what it guarantees.
- Tests live next to the code for units and in `tests/` for contracts and integration; property tests use
  `proptest`; deterministic seeds recorded on failure.

### 2.2 TypeScript and the web
- Strict TypeScript; ESLint and the formatter clean; no `any` outside generated files.
- Protocol types are generated from `tqa-types` JSON Schemas (gate G-08); hand-written duplicates are a review
  failure.
- Components are small and pure where possible; state for the live transcript lives in one store keyed by `seq`.
- Accessibility: keyboard operation and labels on every interactive element; automated checks in WEB-U-007.
- Visual consistency: every page follows [UI-GUIDELINES](UI-GUIDELINES.md); components use design tokens only, and
  a lint rule rejects colour literals outside the token file.

### 2.3 Documents
- Plain professional English; short sentences; tables for lists of requirements and tests.
- Stable IDs for goals, requirements, metrics, decisions and tests; never renumber, only append or mark superseded.
- Follow the public-repository content rules in AGENTS.md: no local paths, private links, personal names or
  references to other organisations' internal work.

### 2.4 Provenance and citations
- Cite public sources by name and public URL in design documents where a design decision depends on them.
- No code, prompt text, schema or document is copied from any other code base. Reading another project's public
  source to understand an idea is fine; reproducing it is not (see §2.5).

### 2.5 Implementation provenance
Learn from public designs and papers, then write TensorQuay Agent's code independently. Never copy or line-by-line port
another project's code, prompts, schemas or tests. Third-party software enters only as a declared dependency under a
permissive licence, with its notice kept in `THIRD-PARTY-NOTICES.md`. Reviewers reject changes that look ported.

## 3. Git and review
- Rules and hooks: AGENTS.md (identity, Conventional Commits, forbidden content, no bypass).
- Branches: `feat/…`, `fix/…`, `docs/…`; `main` is protected; squash-merge with a conventional title.
- Every PR states what changed, why, and how it was verified, and links the requirement or test IDs it affects.
- Review: [`REVIEW.md`](../REVIEW.md) sets the layers, the risk tiers, what each area's reviewer looks for and the
  three-round cap.
- Milestones need independent acceptance by the technical lead and an evaluation report in `docs/evals/`.

## 4. Tests and coverage
- **Tests before code.** A contract (types, invariants, error codes) and its tests are approved before implementation.
- **Coverage ≥ 90 % lines on the pure crates**, measured with `cargo-llvm-cov` (gate G-04); the runtime, sandbox and
  gateway crates report coverage and must not regress.
- **Never weaken an assertion to make a test pass.** A failing test is a bug in the code until proven otherwise.
- **Reproduce before fixing.** A bug fix starts with a failing test that shows the reported behaviour.
- Flaky tests are quarantined with an issue and a deadline, never deleted silently.

## 5. Test levels
| Level | What | Where |
|---|---|---|
| Unit | Pure functions and small types, with fakes | every change, macOS and Linux |
| Contract | Wire frames, registry files, provider request and response shapes, schema sync | every change |
| Integration | Runtime in a container, gateway with PostgreSQL, Docker backend | Linux CI with Docker; macOS locally |
| Firecracker | The sandbox contract suite on the microVM backend | Linux KVM runner |
| End to end | Browser through gateway to sandbox and a model server; agent waves | Docker mode in CI with a fake model; the TensorQuay box for real models |
| Performance, soak, security | TEST-PLAN §13–§15 | scheduled and before releases |

## 6. CI gates
Tier 1 runs on every PR; tier 2 needs Docker; tier 3 needs a KVM host; tier 4 is scheduled or pre-release.

| Gate | Tier | Check |
|---|---|---|
| G-01 | 1 | `cargo fmt --check` and the web formatter |
| G-02 | 1 | `cargo clippy --all-targets -D warnings`; ESLint and `tsc --noEmit` |
| G-03 | 1 | Unit and contract tests for all crates and the web app |
| G-04 | 1 | Coverage ≥ 90 % lines on the pure crates; no regression elsewhere |
| G-05 | 1 | `cargo deny` (licences, advisories, bans, sources), the npm licence and audit check, and G-05-images: every third-party image in a compose file is on `deploy/third-party-images.toml`, pinned, under an allowed licence (ADR-0017 point 8) |
| G-06 | 1 | No source file over 1000 lines |
| G-07 | 1 | Crate dependency direction matches ARCHITECTURE §3 |
| G-08 | 1 | Protocol JSON Schemas and the generated web types are in sync |
| G-09 | 1 | Documentation links resolve; test IDs referenced in code exist in TEST-PLAN; G-09-complete: every ADR, testkit fake, document and guide section the plans or the copy name exists, and the protocol reference lists exactly the schema's frames, events, codes and types |
| G-10 | 1 | `tools/git-guard.py audit` (including internal references in the tree) and a secrets scanner pass |
| G-11 | 2 | Runtime, gateway and Docker backend integration suites (RT, GW, SBX-C on docker, SBX-D) |
| G-12 | 2 | Web end-to-end suite in Docker mode with the fake model server |
| G-13 | 3 | Firecracker suite (SBX-C on firecracker, SBX-F) |
| G-14 | 4 | Performance, soak and security suites; results stored as JSON with the environment |
| G-15 | 4 | Release: images built by digest, SBOM produced, third-party notices current |

Status on 23 Sep 2026: G-01 to G-10 run in `tools/check.sh` and CI, with G-04's threshold on the pure crates and a
workspace report; the web gates run in `web.yml`; the Docker suite runs in `docker.yml`. G-09 checks local Markdown
links and exact test IDs, with a report-only list of unreferenced Gate 1 P0 cases. G-10 runs the guard audit and pinned
Gitleaks scans of tracked and new nonignored working files and full history; missing tools fail the gate.
G-11 to G-15 arrive with their phases. A gate listed here but not yet running is never reported as passed.

## 7. Review checklist
1. Does the change match an approved contract and its tests? Are new tests P0/P1 labelled in TEST-PLAN?
2. Are all new inputs bounded (§1.2 rule 10) and all new awaits cancellable (rule 11)?
3. Does anything new reach the sandbox that could be a secret (rule 7)? Any new egress (rule 16)?
4. Is model-specific behaviour a registry property, not a vendor branch (rule 19)?
5. Are events complete: does every new state change emit a typed event with a schema (rule 8)?
6. Is the dependency direction intact (G-07) and is every new dependency justified and permissively licensed?
7. Is the content public-repository safe (AGENTS.md rules)? Any paths, names, private links, other organisations'
   internal references?
8. Are documents updated in the same PR (protocol reference, admin guide, TEST-PLAN)?
