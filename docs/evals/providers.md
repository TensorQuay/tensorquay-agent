# Live provider suite (E2E-PROVIDER-014)

**State:** passing on the release candidate of 2026-09-28 (dev `545c3d0`): all four hosted routes verified and each
passed its task. This page is the repository's summary. It names no provider or model: each route is named by its
protocol and settings, and the mapping to providers is kept outside the repository.

## What the suite checks
Four hosted routes with the settings we ship: two Chat Completions, one Responses and one Messages. Per route:
1. **The route check** (GW-I-037), the administrator's "test route", through the relay's own shaping, credential and
   decoder:
   - one streamed request with one tool, which must produce a tool call;
   - the round trip with the tool's result, which must produce an answer;
   - on a route that sends strict tools, a two-line string, which must keep its line break (GW-U-064).
2. **One real task** (the smoke task), only if the route check verified: one attempt through the whole product (a
   conversation, a sandbox and the agent loop), under a cap of 30,000 tokens per route.

The keys reach the stack only through the gateway's secret seed, which the runner removes as soon as the stack is up.
The run's output shows no key, base URL or model name. Each run spends real quota, so it runs only with the technical
lead's approval, before a `main` update or a demo.

## Results: release candidate, 2026-09-28 09:21–09:23 +0800
Images of dev `545c3d0` (runtime protocol revision 8), the suite at `f88b325`.

| Route | Settings | Tool call | Round trip | Strict line breaks | First byte | Task | Tokens (prompt / completion) | Requests |
|---|---|---|---|---|---|---|---|---|
| Chat | defaults | passed | passed | passed | 144 ms | pass | 15,417 / 852 | 5 |
| Chat | schema keywords `basic`; reasoning `reasoning_effort`, `low` | passed | passed | not run (no strict tools) | 1,446 ms | pass | 14,052 / 321 | 6 |
| Responses | defaults | passed | passed | passed | 2,000 ms | pass | 15,486 / 452 | 8 |
| Messages | `max_output` 16,000; reasoning `thinking`, `medium`, budget 4,096 | passed | passed | not run (no strict tools) | 2,015 ms | pass | 15,556 / 588 | 4 |

About 62,000 tokens in all, against a cap of 128,000. Tokens are the cost measure: routes carry no prices.

## How the routes got there (2026-09-27 and 28)
The first live run (27 Sep 15:48) failed every route check. Each failure was a product defect, fixed test-first, and
none was fixed by a provider-specific branch:

| Route | What failed | The fix | Rows | Verified |
|---|---|---|---|---|
| Responses | The endpoint labelled its event stream `text/plain` | The relay recognises and meters a mislabelled event stream | GW-U-062 | 27 Sep 16:52 |
| Chat, reasoning `low` | The endpoint attaches an opaque state to each tool call and requires it back on the next request | The state is kept as reasoning state and sent back on its call (ADR-0027, amended) | LLM-U-042, LLM-U-043 | 27 Sep 18:17 (task 18:20) |
| Messages | The endpoint refused an empty system text block | The encoder leaves out empty and blank system texts | LLM-U-044 | 27 Sep 21:07 (task 23:43) |
| Chat, defaults | A model that reasons before its call ran out of the route check's 64-token output allowance mid-call, and the decoder called it "invalid arguments" | The check allows 256 tokens on a route without reasoning, and a call cut at the output limit is named as cut | GW-I-037, LLM-U-045, GW-U-067, GW-U-068 | 28 Sep 02:07 |

## Reproduce
- **Dry mode** (no key, no paid endpoint; TK-U-018): the same four routes, with the settings we ship, against one
  scripted fake per profile:

  `TQA_DOCKER_TESTS=1 python3 -B tests/e2e/live/providers.py`
- **Live mode** (spends real quota):

  `python3 -B tests/e2e/live/providers.py --live --routes-file <private routes file> --env-file <private key file> --yes`

  The routes file (mode 0600) holds each route's model, base URL and key variable. The key file (mode 0600) holds the
  keys. Without `--yes`, the runner prints its plan and spending bound and stops. The report goes to `--report-dir`,
  outside the repository.

## Limits
- One task attempt per route: a working check, not a benchmark. Completion rates on a task set are E2E-WAVE-001 and
  E2E-CMP-003 on a local model.
- Hosted endpoints change models and behaviour without notice, so a result holds for its date.
- The routes' settings are ours. Another endpoint behind the same protocol may need different settings; the route
  check names the setting a refusal points at.
