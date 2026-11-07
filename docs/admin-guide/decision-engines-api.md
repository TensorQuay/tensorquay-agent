# Decision engines: API reference

The endpoints for [decision engines](decision-engines.md). The full contract, with every field and bound, is
`crates/tqa-gateway/openapi.json` (operations `adminDecisionEngines`, `adminDecisionEnginePut`,
`adminDecisionEngineDelete` and `relayDecision`). Errors use the gateway's usual body:
`{"error": {"code", "message", "retryable", ...}}`.

## Administration: `/api/v1/admin/decision/engines`
For administrators, with a session or an administrator's API token. Writes take an optional `Idempotency-Key`, and a
retry with the same key replays the stored answer. Every write is audited without values.

### `GET /api/v1/admin/decision/engines`
Every stored engine, by name: `{"items": [...], "generation": <n>}`. Each item has `name`, `display_name` (when set),
`protocol` (`decision_v1` or `typed_questions_v1`), `url`, `auth` (`scheme`: `none`, `bearer` or `header`, with `header`
for the last), `key_ref` (a secret's name, never a value), `timeout_ms`, `exposure` (`none`, `calls` or `full`),
`enabled`, and `key_missing`, `version`, `seed`, `last_set_by` and `updated_at`. An engine may also have a `budget`
(`calls`, `tokens`, `period`: `total` or `month`), and then `budget_used` (`calls` and `tokens` over the current count)
and `budget_since` (where that count starts: when the budget was set or last changed for `total`, the month's first
instant in UTC for `month`). A `typed_questions_v1` engine also has `model`, and `min_confidence` and `route_hints` when
set.

### `PUT /api/v1/admin/decision/engines/{name}`
Creates the engine (201, no `version` in the body) or replaces the version read (200, with `version`). The body is
every field except `name`, which the path gives. `insecure_http` and `ca_file` are for `gateway.toml` only.

| Field | Kind | Rule |
|---|---|---|
| `protocol` | any | `decision_v1` (the default) or `typed_questions_v1` |
| `model` | `typed_questions_v1` only, required | The service's model identifier: letters, digits and `. _ : / -`, up to 128 |
| `min_confidence` | `typed_questions_v1` only | 0 to 1; 0.6 when unset |
| `route_hints` | `typed_questions_v1` only | Up to 16 `route: description` pairs, each description one line of 1 to 200 characters |
| `budget` | any | `calls` and `tokens` (tokens for `typed_questions_v1` only), at least one, each 1 or more; `period` `total` (the default) or `month` |

| Status | Code | When |
|---|---|---|
| 400 | `invalid_request` | A field breaks its rule; `details[]` names the field and reason, for example `name_invalid`, `display_name_invalid`, `base_url_invalid`, `auth_query_invalid`, `auth_header_invalid`, `auth_invalid` (a key without a way to send it), `key_ref_not_reference`, `name_key_shaped` (the secret's name in `key_ref` looks like a key itself: a key pasted where its name belongs), `secret_ref_reserved` (a person's own secret), `limit_out_of_range` (`timeout_ms` outside 1 to 10000, or `min_confidence` outside 0 to 1), `model_empty`, `parameter_invalid` (a bad `model` or `route_hints`, or a `typed_questions_v1` field on a `decision_v1` engine), `route_name_invalid`, `budget_invalid` or `file_only` |
| 403 | `forbidden` | Not an administrator |
| 409 | `conflict` | The version is stale, missing on a replace, or given on a create |
| 409 | `secret_not_allowed` | `key_ref` names a secret that doesn't allow this engine; allow it on the secret first |
| 409 | `decision_engine_limit` | The installation already has 16 engines |
| 422 | `idempotency_key_reused` | The key was used for a different request |

A change of `exposure` is also recorded as its own audit entry, action `exposure`, with the old and new values, and a
change of `budget` likewise, action `budget` (for example `none->calls3000_month`). Saving a new budget starts a new
`total` count; the same budget keeps it.

### `DELETE /api/v1/admin/decision/engines/{name}?version=<n>`
Deletes the version read (204). The engine's secret binding goes with it. A seeded engine returns at a later start
only if its `[[decision_engines]]` entry changes.

| Status | Code | When |
|---|---|---|
| 403 | `forbidden` | Not an administrator |
| 404 | `not_found` | No such engine |
| 409 | `conflict` | The version is stale or missing |
| 409 | `decision_engine_in_use` | A profile or an active task names it; `in_use` counts the tasks (`conversations`) and personal profiles and names the shared `profiles`. Turn it off instead |

## The runtime's relay: `POST /relay/d/{engine}/v1/{point}`
Called by a task's runtime with its own runtime token, never by people. `{point}` is `before_step` or
`after_response`, and the body is a `decision/v1` request. The gateway checks the request and the engine's budget,
adds the engine's key from the secret store, posts it to `<url>/v1/<point>` within the engine's `timeout_ms`, and
answers with the engine's reply, passing back only its content type.

For a `typed_questions_v1` engine, only `before_step` exists. The gateway posts one question to the engine's `url`
itself (not `<url>/v1/<point>`), with the engine's `model`, and answers with the `call_model` decision it builds from
the service's answer.

| Status | Code | When |
|---|---|---|
| 401 | `unauthorized` | No live runtime token |
| 403 | `forbidden` | The task's profile doesn't name this engine (binding) |
| 404 | `not_found` | An unknown or turned-off engine, or an unknown point |
| 413 | `body_too_large` | The request is over 64 KiB |
| 400 | `invalid_request` | The body isn't a `decision/v1` request, its point differs from the path, or it carries what the engine's exposure excludes (arguments below `calls`; the answer's text or the transcript below `full`) |
| 429 | `rate_limited` | The task owner's relay rate is spent; `Retry-After` says when |
| 409 | `decision_engine_budget_spent` | The engine's budget for the period is used up; nothing was sent |
| 503 | `secret_unavailable` | The engine's key has no value |
| 502 | `decision_engine_invalid` | A multiple-choice service's answer was unusable or it refused the question as invalid, or the question's state would be over 16 KiB (then nothing was sent) |
| 502 | `upstream_unavailable` | The engine failed, refused (for a multiple-choice service, also its key refused or the service overloaded), or answered with more than 64 KiB |
| 504 | `timeout` | The engine didn't answer within `timeout_ms` |

Each consultation that passes the checks is recorded as usage under `decision:<name>`; a typed question refused
before sending is not. The runtime treats any failure as the engine having no answer for that step, and the agent
decides alone. A 409 `decision_engine_budget_spent` is recorded as `decision.fallback` with reason `budget_spent`,
once, and the runtime doesn't consult the engine again in that run; the next run does.

## Related refusals elsewhere
| Where | Status | Code | When |
|---|---|---|---|
| `POST /api/v1/conversations/{id}/messages` | 409 | `decision_engine_unavailable` | The task's profile names an engine that is gone or turned off |
| The runtime link (`/runtime/link`) | 426 | `runtime_too_old` | The task's profile names an engine and its runtime announces a protocol revision below 9; `needs` is 9 |
| The task's event stream | notice | `decision_engine_budget_spent` | The task's engine used its budget; the agent decides on its own until the period ends. Once per task and period, not retryable |
| The task's event stream | notice | `decision_engine_unavailable` | The engine went after a message was accepted: the sandbox stopped, or queued work failed. Retryable once the engine is back |
