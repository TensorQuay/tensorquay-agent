# Reference decision engine

A small `decision/v1` engine to read, run and copy (ADR-0013). It is one Python file, [engine.py](engine.py), using
only the standard library, under the Apache-2.0 licence.

## What it decides
- **Before each step** it chooses the model route. After a step whose calls all only read (`read_file`, `glob`,
  `grep`, `read_notes`, and any tool named with `--read-only`) and all succeeded, the next step goes to the cheaper
  route named by `--cheap-route`. Every other step stays on the task's own route: the first step, a step after a
  failed call, a write or a command, and any step whose allowed routes don't include the cheaper one.
- **After a response** it keeps every call the model made, the same as having no engine.
- It never narrows the tools, runs a call itself, compacts or stops a task.

It needs only the lowest exposure, **Tool names only** (`exposure = "none"`): it reads the names and statuses of the
previous step's calls, never their arguments or the conversation.

## Run it
```sh
python3 examples/decision-reference/engine.py --port 8700 --cheap-route cheap
```
- `--bearer-from-env ENGINE_KEY` requires that environment variable's value as a bearer token; a request without it
  gets 401.
- `--read-only mcp.docs.search` counts another tool as read-only; repeat it for more.
- It prints one line per decision, such as `step 3: call_model cheap`, and nothing from the task.
- `--self-test` runs its own tests.

## Check it
```sh
tools/ext-check decision http://127.0.0.1:8700
```
Every consultation should pass at every exposure level (see
[Test an engine first](../../docs/admin-guide/extensions.md#test-an-engine-first)).

## Use it
Add it in **Settings**, **Decision engines**, or in `gateway.toml`, and name it in the profiles that should use it.
The task's model route needs a declared fallback called `cheap` (or whatever `--cheap-route` names); a route the
task can't use is never chosen.
```toml
[[decision_engines]]
name = "reference"
display_name = "Cheaper routine steps"
url = "http://127.0.0.1:8700"
insecure_http = true            # a plain http address, for an engine on the gateway's own machine
auth = { scheme = "none" }
exposure = "none"
```
Anywhere else, put it behind an `https` proxy; `insecure_http` is meant for an engine on the gateway's own machine.

## Change it
The policy is `choose_route`, about ten lines. Everything else is the protocol: two paths, one JSON decision per
request, at most 64 KiB each way. The agent checks every answer: a route or tool outside the request is refused, and
the step goes on as if there were no engine.
