# Contributing

TensorQuay Agent is an open-source agent harness and web UI for teams that run AI on their own hardware. Start with a
concrete bug, a model that behaves badly with the harness, a measured bottleneck or a deployment problem. Prefer a
small change that can be checked independently.

## Getting started
1. Read [AGENTS.md](AGENTS.md), [the engineering rules](docs/DEV-GUIDELINES.md) and
   [the engineering guardrails](docs/ENGINEERING-GUARDRAILS.md).
2. Install the repository guardrails with `tools/install-guardrails.sh`: its hooks check what you commit and push
   for secrets, personal data and the message rules. Commit as yourself. Maintainers add `--maintainer`, which sets
   the TensorQuay identity, and keep the private local lists described in `tools/git-guard.py`.
3. Read the design documents: [PRD](docs/PRD.md), [ARCHITECTURE](docs/ARCHITECTURE.md), [DEV-PLAN](docs/DEV-PLAN.md),
   [TEST-PLAN](docs/TEST-PLAN.md).

For a new tool, provider, runtime backend or protocol change, agree on the scope with the maintainers before
implementing it, and write the contract and tests first. Small fixes and tests can use the existing contracts.

## Independent implementation
Learn from public designs and cite them by name, then write the implementation independently. Do not copy or
line-by-line port another project's code, prompts or schemas. Third-party software enters only as a declared
dependency under a permissive licence.

## Running the checks
Install Rust 1.97.1 and its pinned components from `rust-toolchain.toml`, then the gate tools:

```sh
rustup toolchain install
cargo install --locked cargo-nextest@0.9.145
cargo install --locked cargo-llvm-cov@0.9.1
cargo install --locked cargo-deny@0.20.2
./tools/install-gitleaks.sh && export PATH="$HOME/.local/bin:$PATH" # Gitleaks 8.30.1, checksum verified
```

Run all Rust tier-1 gates from the repository root with `./tools/check.sh`. It runs every gate, whatever failed
before it, prints a PASS or FAIL line for each, and ends with the list of those that failed. Before a hand-in,
`./tools/check.sh --member` runs every gate except the whole-workspace coverage report (`G-04-report`, the largest
build); G-04's 90 % threshold on the pure crates still runs, and the integration run keeps the full set. Environments
without the private guard denylist can add `--skip-audit` (alone or with `--member`), which skips only the guard audit
and still runs the secrets scan; CI runs the audit in a separate job: on pushes
to `main` it requires the `TQ_GUARD_DENYLIST` repository secret; on a pull request from a branch of this repository it
uses that list, and on one from a fork it runs without it (GitHub gives forks no secrets), so contributions from forks
are checked too.

The runner executes these commands in order:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo llvm-cov nextest -p tqa-types -p tqa-llm -p tqa-context -p tqa-tools -p tqa-prompt -p tqa-core --fail-under-lines 90
cargo llvm-cov nextest --workspace
cargo deny check
python3 tools/check-file-length.py
python3 tools/check-dep-direction.py
cargo run -q -p tqa-types --bin export-schema -- --check crates/tqa-types/schema/tqa-protocol-v1.json
python3 tools/check-docs.py
python3 tools/git-guard.py audit
python3 tools/check-secrets.py
```

Check the gate tools themselves with:

```sh
python3 tools/check-file-length.py --self-test
python3 tools/check-dep-direction.py --self-test
python3 tools/check-docs.py --self-test
python3 tools/check-secrets.py --self-test
shellcheck tools/check.sh tools/install-gitleaks.sh
```

### Web, Docker and database prerequisites
- Node.js `>=22.13.0` (`web/.nvmrc` records 24.21.0, as the web image uses) with Corepack (the web package enforces it at install); the web app pins its pnpm version. From `web/`, run
  `corepack pnpm install --frozen-lockfile`; its scripts are listed in [web/README.md](web/README.md).
- Docker Engine or Docker Desktop, for the sandbox backend, the gateway's database tests and the one-box deployment.
- SQLx CLI 0.9.0 (`cargo install --locked sqlx-cli@0.9.0 --no-default-features --features rustls,postgres`) when you
  change gateway queries or migrations; how to regenerate the offline query cache is in
  [crates/tqa-gateway/README.md](crates/tqa-gateway/README.md).

Gate definitions are in [DEV-GUIDELINES §6](docs/DEV-GUIDELINES.md#6-ci-gates). Integration tests
that need Docker and the Firecracker suite that needs a Linux KVM host are described in
[TEST-PLAN §1a](docs/TEST-PLAN.md#1a-where-tests-run).

## Add a language

The web app's text lives in one catalogue per language under `web/src/i18n/locales/`. English (`en.json`) is the
reference; every other file translates some or all of its keys.

1. Copy `web/src/i18n/locales/en.json` to `<code>.json`, where `<code>` is a BCP 47 tag with the script where it
   matters (for example `pt-BR`, `zh-Hant`, `sr-Latn`).
2. Set `"language"` to the language's name written in its own script (for example `"Português (Brasil)"`). This is
   what the language menu shows.
3. Add `"status": "draft - native review welcome"` until a native speaker has reviewed the whole catalogue; remove it
   in the review pull request.
4. Translate the values in `"messages"`. Keep every `{argument}` exactly as in English, keep ICU `plural` and
   `select` blocks and give them the plural categories your language uses (`one`, `few`, `many`, `other`, …;
   `other` is always required). Write apostrophes as `’` or `''`: a single `'` before `{` starts ICU quoting.
   Leave keys you cannot translate out; the app shows English for them.
5. From `web/`, run `corepack pnpm i18n:gen` to add the language to the menu, `corepack pnpm i18n:report` to list
   missing keys, and `corepack pnpm check` (the catalogue tests check that arguments match English and that every
   message formats).
6. If the language uses a script the brand faces do not cover, add system faces for it (macOS, Windows and a Linux
   Noto face; no downloaded fonts) as a `--fonts-<script>` list in `web/src/app/globals.css`, add that list to
   `--fonts-scripts`, and add the Linux package to the font step in `.github/workflows/web.yml`. The end-to-end test
   `e2e/fonts.spec.ts` checks, for every language, that each character is drawn by a listed face and that the longest
   strings keep the 1024 px layout.
7. Open a pull request. Say which parts you reviewed and whether you are a native speaker.

### Improve an existing language
Most shipped languages are drafts that translate part of the catalogue; the language menu marks them "(partial)".
From `web/`, `corepack pnpm i18n:report` prints, for every language, how many messages it translates and which keys
it is missing. To help:

- **Fill in missing keys.** Add them to the language's file with the English key; keep the order of `en.json` if you
  can, but order does not matter to the app. The menu drops "(partial)" once every key is translated (run
  `corepack pnpm i18n:gen`).
- **Review a draft.** Read the translation in the running app, not only the file: the length and the tone show there.
  When a native speaker has reviewed the whole catalogue, remove its `"status"` line in the same pull request.
- **Correct a translation.** A pull request that changes a few values is welcome. Say why, for example "the usual
  word on Thai keyboards is …".

### Words and tone
- **The product's name**, TensorQuay Agent, is never translated.
- **"Task"** is the unit of work people see ("New task", "Delete task"). Use your language's everyday word for a task
  or job, and the same word everywhere. Never translate it as "conversation" or "session".
- **Internal words stay out:** a translation never introduces "sandbox", "route" or "runtime" where the English says
  "environment" or "model". Settings pages for administrators are the exception, where the English itself uses
  them (for example **Model routes**).
- **Tone:** plain, short and in the present tense, as in the English (UI-GUIDELINES §10). No exclamation marks. Use
  your language's usual capitalisation for buttons and headings; the English uses sentence case.
- **Length:** the layout allows about 40 % more than the English. If a translation is much longer, prefer a shorter
  everyday word over an abbreviation.
- **What stays as it is:** every `{argument}`, a path or pattern with angle brackets in a message (such as
  `secrets/<name>`), and product and file names.
- **Never translated:** the agent's own answers and the tool output it shows. Only the product's own text is in
  the catalogues. The documentation in `docs/` is English.

The catalogue tests (WEB-U-019), the partial-language list (WEB-U-032) and the font check (`e2e/fonts.spec.ts`) run in
CI on every pull request that changes a catalogue.

## Pull requests
- One concern per PR; tests included; documentation updated in the same PR.
- Review follows [`REVIEW.md`](REVIEW.md): its risk tier decides who reviews and what evidence the PR carries.
- Conventional Commits under your own name (maintainers commit as TensorQuay); no attribution lines (AGENTS.md).
- Describe what changed, why, and how it was verified. The maintainers run independent acceptance for milestones.
