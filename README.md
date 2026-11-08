# TensorQuay Agent

**An open, free and efficient on-premise AI agent platform that is easy to use, for any company and any individual.**

We believe developers and the open-source community deserve to use AI with the best options available to them, not
locked into one vendor's platform, and that end users deserve every level of the harness: isolated sandboxes, team
management and, in time, agent swarms. We are building an environment and a communication system for agents, not for
humans: humans set goals and watch, and everything a browser can do a program can do too. TensorQuay Agent
is that platform. Sign in from a browser, give the agent a
task, and it works in an isolated sandbox on its own workspace: reading, editing, running and shipping code. Models run
locally on an OpenAI-compatible server (vLLM, SGLang or TensorQuay Engine) or, when you choose, through a cloud
provider.

**Why another harness?** The existing ones are a CLI on one developer's laptop, a hosted service, or a chat window
without a workspace. A team sharing one GPU workstation needs a fourth shape: a server on the local network that gives
every person a sandboxed agent, works well with open models, keeps secrets out of the sandbox, and can be installed by
one administrator. The full argument is in [PRD §1](docs/PRD.md#1-mission).

- **Multi-user on one box:** accounts, roles, persistent conversations, one sandbox per conversation, resume after
  disconnects and restarts.
- **Two runtime modes behind one interface:** `docker` (default; Docker Engine or Docker Desktop) and `firecracker`
  (KVM microVMs on a Linux host). Same image, same protocol, same tests.
- **Local-model first:** OpenAI-compatible protocols, prompt-cache-aware context, a tool-call fallback for models with
  weak native tool calling, budgets sized for smaller context windows.
- **Isolation by design:** inside its sandbox the agent works freely; secrets stay in the gateway's built-in encrypted
  secret store and never enter the sandbox, and the network allow-list is the guardrail: a refused connection names
  its host and port (ADR-0035).
- **Workspaces on your disk:** every workspace is an owned, isolated directory or block image on the host's local disk,
  mapped into the sandbox. A conversation binds to one workspace; conversations can reuse, fork and share workspaces.
  No object storage in the file path.
- **Minimal core, open extension points:** skills, MCP tools inside the sandbox, connectors as gateway-proxied MCP
  servers with a credential vault, customised agent profiles, sandbox templates and model routes. No in-process plugin
  API: MCP is the plugin protocol. The core ships no connectors; each is an MCP server the user registers. The
  decision engine is pluggable too: the model decides by default, an external engine can be configured.
- **Agents talk through channels:** a shared, ordered forum at the gateway where agents and humans post, propose,
  vote, decide and claim tasks. No broker, no point-to-point links. Agent swarms, a later phase, will build on it.
- **Lean Rust harness, TypeScript web UI, Apache-2.0.**

## Try it
The one-box Docker install is in [deploy/README.md](deploy/README.md#install): build the images with
`deploy/build-images.sh`, point it at a model server (a local OpenAI-compatible one, or a hosted provider; or
build the scripted test model with `--fake-model` to try it without one), start it, and open the page in a browser. It runs on Linux and on a Mac with Docker Desktop. Without a GPU, a
local model works but slowly, minutes a step, too slow for helpers or the desktop: see the user guide's [Running
without a GPU](docs/user-guide/tasks.md#running-without-a-gpu) section.

## Status
**Releases: 17.0 and its fix release 17.1** (see the [changelog](CHANGELOG.md)). The one-box install runs end to end:
accounts, tasks in isolated sandboxes, models through the gateway, workspaces, connectors and channels (the
[development plan](docs/DEV-PLAN.md) has what is done and what is next). There are no prebuilt images, so the images
are built locally, and interfaces will still change.

## How it fits together
```text
browser ── HTTPS: events streamed down, actions posted up ──► tqa-gateway ──► PostgreSQL (history, users, usage)
                                                                  │
                                           outbound WebSocket     │   model calls through the gateway relay
                                                                  ▼
                                         sandbox: tqa-runtime (agent loop and tools) on /workspace
```
The gateway is the only public address. Each conversation gets its own sandbox on its own network, which dials out
to the gateway and has no other network route: sandboxes cannot reach each other. Model credentials stay in the gateway. Details are in
[ARCHITECTURE](docs/ARCHITECTURE.md).

## Repository layout
| Path | Content |
|---|---|
| `crates/tqa-types` | The wire protocol: typed frames and events, exported as JSON Schema |
| `crates/tqa-llm` | Model providers, streaming, retries and the model-route registry |
| `crates/tqa-context` | Message history, projection, resume rules, context window and write-ahead log |
| `crates/tqa-tools` | The built-in tools (read, write, edit, search, glob, grep, shell, fetch_url, notes, and the channel and team tools) and workspace confinement |
| `crates/tqa-prompt` | The versioned system prompt and instruction files |
| `crates/tqa-core` | The agent loop: steps, tool batches, budgets, cancellation and resume |
| `crates/tqa-runtime` | The process inside each sandbox: gateway link, journal, run supervisor |
| `crates/tqa-sandbox` | Sandbox backends behind one interface (Docker, and Firecracker microVMs on a Linux KVM host) |
| `crates/tqa-gateway` | The control plane: accounts, conversations, event store, streams, sandbox manager, relay, secrets |
| `crates/tqa-testkit` | Fakes shared by the tests: model server, clock, provider, gateway |
| `crates/tqa-ext-check` | The extension catalogue's linter (`tools/check.sh` runs it) |
| `crates/tqa-vm-init` | The init process inside a Firecracker microVM |
| `web/` | The web app (Next.js, React, TypeScript) |
| `deploy/`, `images/` | The one-box Docker Compose deployment and the container images |
| `docs/` | Product, architecture, plans, guidelines and decision records |
| `tools/` | Repository gates and git guardrails |
| `brand/` | Logo and brand assets |

## Development
Setup, prerequisites and the checks every change must pass are in [CONTRIBUTING](CONTRIBUTING.md). The one-box
deployment is described in [deploy/README.md](deploy/README.md).

## Documents
| Document | Content |
|---|---|
| [docs/PRD.md](docs/PRD.md) | What we build and why; goals, users, success metrics, scope, requirements, risks, decisions |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Components and crates, runtime modes, agent loop, providers, tools, wire protocol, gateway, web, security |
| [docs/DEV-PLAN.md](docs/DEV-PLAN.md) | Phase-by-phase implementation plan with gates and estimates |
| [docs/TEST-PLAN.md](docs/TEST-PLAN.md) | Test principles, environments, the designed test cases, performance protocol, phase gates, traceability |
| [docs/DEV-GUIDELINES.md](docs/DEV-GUIDELINES.md) | Engineering rules: design, code style, provenance, git, tests, CI gates, review |
| [docs/UI-GUIDELINES.md](docs/UI-GUIDELINES.md) | One visual system: brand, colour and type tokens, layout, components, states, accessibility, copy |
| [docs/ENGINEERING-GUARDRAILS.md](docs/ENGINEERING-GUARDRAILS.md) | Practice standard: universal principles, size limits, testing standard, Rust, Go, TypeScript and React, SQL, shell, containers |
| [docs/release.md](docs/release.md) | Release artefacts: the SBOMs and package lists every release publishes, and how to produce them |
| [docs/admin-guide/](docs/admin-guide/README.md) | For administrators: files and settings pages: users, status, models, secrets, egress, limits, connectors, environments, usage, and everyone's API tokens, keys, connector credentials and profiles |
| [docs/user-guide/](docs/user-guide/README.md) | For everyone: tasks, files and changes, workspaces, sharing, keys, channels and helpers |
| [crates/tqa-gateway/openapi.json](crates/tqa-gateway/openapi.json) | The HTTP API reference (OpenAPI 3.1): every route, its fields and its error codes |
| [AGENTS.md](AGENTS.md) | Rules for anyone working in this repository, including the git guardrails |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute |

## Licence
Apache License 2.0. See [LICENSE](LICENSE).
