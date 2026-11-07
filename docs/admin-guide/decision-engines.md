# Decision engines

A decision engine is a service that a task consults at each step, to choose the model, narrow the tools or stop the
step. There are two kinds:
- **Your own engine**, speaking this product's `decision/v1` protocol (`protocol = "decision_v1"`, the
  default);
- **A multiple-choice service**, an outside service that answers multiple-choice questions with a confidence
  (`protocol = "typed_questions_v1"`). The gateway asks it one question before each step, which model route to use,
  and turns its answer into a decision.

The agent never holds the engine's key or address: the gateway keeps both and relays each question to the engine. An
engine adds no tools, and a task uses one only when its profile names it.

This page covers setting one up and what to expect. The fields are in [Settings, Decision engines](web-settings.md#decision-engines);
the endpoints are in the [API reference](decision-engines-api.md).

## Add an engine with your own key
Each installation brings its own engine and its own key, and pays for its own use of it.
1. Put the key in **Keys**, **Shared** (or the secrets seed file). The value is never shown again.
2. Add the engine in **Settings**, **Decision engines**, with **Add decision engine**. First choose its kind,
   **Your own engine** or **A multiple-choice service**, and **Continue**; then give it a display name, its
   `https://` address and its key, and choose what it sees. A multiple-choice service also needs its
   **Model**, the model identifier the service's documentation gives: the service refuses a question without one, so
   there is no default. You can also add it in `gateway.toml` under
   `[[decision_engines]]` or through the API.
3. If the save says the secret isn't allowed for the engine, choose **Allow it for …** and save again. A secret is
   used only by what it allows, and an engine from `gateway.toml` is allowed its secret automatically.
4. Name the engine in the profiles whose tasks should consult it: **Settings**, **Agent profiles**, **Decision engine**.
   Only administrators choose it, and a profile without one leaves the agent to decide alone.

Tasks that use an engine need a runtime that speaks protocol revision 9 or later: the runtime image from this release,
and environments rebuilt from it. On an older runtime, a task whose profile names an engine is refused, with a notice
that the environment's runtime is too old; it never runs without its engine.

## What the engine sees
The setting has three levels:

| Level | Value (`exposure` in `gateway.toml` and the API) | The engine sees |
|---|---|---|
| Tool names only (the default) | `none` | Which tools the agent calls, never their arguments or the conversation |
| Names and arguments | `calls` | Also each call's arguments, such as file paths and commands |
| The full conversation | `full` | Everything the model sees |

The gateway refuses a request that carries more than the level allows, even if one is sent to it directly. Every
change of the level is recorded in the administration audit with the old and new level. A task already running keeps
the level it started with until it next runs: after you narrow it, the gateway refuses that task's requests to the
engine (they carry more than the new level allows), and the agent decides alone until then.

## When the engine is asked
The engine is asked at two points: before each step's model call, and after the model answers with tool calls, to
choose which of those calls run. Which points an engine answers depends on its kind, not on a setting. An engine
speaking `decision/v1` answers both; a multiple-choice service answers before each step only. At a point an engine
doesn't answer, the agent isn't slowed down and nothing is recorded for it.

## A multiple-choice service
Before each step, the gateway asks the service one question, `route`: which of the task's model routes to use for
this step. Each option is a route, described by its line in **Route descriptions** (or by its name). The question
carries the step's state within what the engine sees (at most 16 KiB): the step and attempt, the task's budget use, the
previous step's calls by name and status, and the step's tool names.

The service's choice becomes the step's route, with every tool. When its confidence is below **Confidence needed to
switch routes** (`min_confidence`, 0.6 by default), the task keeps its own route for that step. The service's tokens
for each question are recorded with the consultation's usage.

```toml
[[decision_engines]]
name = "router"
display_name = "Route chooser"
protocol = "typed_questions_v1"
url = "https://questions.example/v1/evaluate"
key_ref = "secrets/questions"
model = "the-model-id-from-the-service-docs"
min_confidence = 0.6
route_hints = { cheap = "routine steps: reading, looking something up, small changes" }
budget = { calls = 3000, tokens = 30000000, period = "month" }
```

## Cost and limits
What bounds an engine's use:
- An optional budget per engine (**Set a budget** in its form), off by default: a number of calls, and for a
  multiple-choice service a number of tokens (input and output together, as the service reports them), either or
  both, per **In total** (`total`) or
  per **Each month (UTC)** (`month`). In total counts from when the budget was set or last changed, so saving a
  new budget starts a new count, while saving the engine with the same budget keeps it. A calendar month counts the
  whole month even after a change in it: raising the budget mid-month only raises the ceiling. The engine's row in
  **Settings**, **Decision engines** shows the use so far, and every change of a budget is recorded in the
  administration audit with the old and new budget. The gateway counts the engine's usage, so the count survives
  restarts, and checks it before each consultation. A question that is refused before it is sent (for a multiple-choice
  service, one over 16 KiB) is not counted, since the budget follows what the service charges.

  Once the budget is used up, a consultation is refused before anything is sent. The task's transcript records it once
  (the engine's budget is used up), the agent decides alone for the rest of that task's run, and the engine isn't
  asked again in it. The next run of the task asks again, so a budget you raise takes effect from then. The task's
  owner is also told while watching. Consultations running at the same moment can pass the last unit together, so a
  budget can be exceeded by at most the consultations in flight when it ran out.
- Each consultation counts against the relay rate of the person whose task it is (`[relay]` in `gateway.toml`), the
  same rate their model requests use.
- Each consultation is recorded as usage under `decision:<name>`, so **Settings**, **Usage** shows how often each
  engine was asked.
- Each question and each answer is at most 64 KiB, and the engine has its time to answer (2 seconds by default, at
  most 10) before the agent decides alone for that step.
- Turning the engine off stops all use at once.

## When it fails
An engine that is unreachable, answers late, answers with something that isn't a decision, or has used its budget
never stops a task.
The agent decides alone for that step and records why. After three failures in a row in one task, the engine isn't
asked again in that task; a used-up budget stops the asking at once, for the rest of the run.

An engine turned off or deleted affects the tasks that name it:
- A message to such a task is refused until the engine is on again.
- A task waiting to start is told, and its sandbox stops.
- Work queued for it fails once, with a notice.
Once the engine is back on, a message continues the task. An engine that profiles or open tasks use can't be deleted;
turn it off instead.
