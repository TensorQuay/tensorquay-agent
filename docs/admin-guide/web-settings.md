# Settings pages
<!-- Tests: WEB-U-131, WEB-U-134. -->

Open **Settings** from the Account menu. The sections are listed on the left in three groups (on a narrow screen,
**Section** above the page opens the same groups as a menu):

- **Your account**, for every person: **Account**, **API tokens**, **Keys** and **Agent profiles**;
- **Administration**, for administrators: **Overview**, **People**, **Models**, **Environments**, **Connectors**,
  **Decision engines**, **Internet access** and **Limits**; the installation's shared keys are on **Keys**, under **Shared**;
- **Operations**, for administrators: **Usage** and **Status**.

This chapter covers them all. Labels in **bold** are the words on the page. The pages' old addresses still work and
lead to the new ones.

Adding or editing something with several parts (a model route, an environment, an agent profile) opens a page of its
own, in one column: each field's label above it, and a list of choices (tools, connectors, capabilities, built-in
images) as a grid across the whole width. A choice among a few options (a model, a protocol, a unit) opens its list
below it; the chosen option shows in the field. **Save** and **Cancel** stay at the bottom, and leaving with unsaved
changes asks first. A refused save moves to the first field to fix, and the line by **Save** counts the fields left.

Each page loads its list when it opens. If loading fails, the page says so, for example "The model routes could not be
loaded.", and offers to try again. A refused save shows the reason next to the field it concerns, in the language the
person has chosen.

- [People](#people)
- [Status](#status)
- [Models](#models)
- [Shared keys](#shared-keys)
- [Shared keys for web services](#shared-keys-for-web-services)
- [Internet access](#internet-access)
- [Limits](#limits)
- [Connectors](#connectors)
- [Decision engines](#decision-engines)
- [Environments](#environments)
- [Usage](#usage)
- [Account](#account)
- [API tokens](#api-tokens)
- [Keys](#keys)
- [Agent profiles](#agent-profiles)

## People
<!-- Tests: WEB-U-023, WEB-U-075, WEB-U-117, WEB-U-156, WEB-E2E-020. -->
Accounts are created by invitation only. Nobody can sign up on their own.

### Invite someone
Choose **Invite**, enter the person's **Email** and choose a **Role**: **Member** or **Administrator**. The page shows
the **Invitation link**. Nothing is emailed: send the link to the person yourself. The link works once and expires on
the date shown. Opening it lets the person choose a password and join with the role you chose.

The **Invitations** list shows the invitations still open, **Pending**, and those **Expired** in the last week, so
one can be sent again. An accepted invitation's person is under **People**; revoked ones are no longer listed.
Revoking a pending invitation stops its link from working.

### Manage people
The **People** list shows each account's email, **Role**, when they **Joined**, how many **Tasks** they have, and
whether the account is **Active** or **Disabled**.

- **Role:** choose it in the list; it changes at once. Your own role can't be changed here ("You can't change your
  own role. Another administrator can."), so the last administrator never steps down by mistake; the gateway also
  keeps at least one administrator active.
- An email shows whole, on one line where there is room, and breaks only after its @ or a dot.
- **Disable:** asks first, naming the email; the person is signed out at once and their API tokens stop working. Their
  tasks stay. Your own row has no Disable.
  **Enable** lets them sign in again.
- **Tasks:** opens the list of that person's tasks. Administrators see a task's details (owner, dates, environment,
  model and state), never its content.

## Status
<!-- Tests: WEB-U-033, WEB-E2E-022, WEB-U-265, WEB-U-292, WEB-U-297, GW-I-229. -->
Status answers one question first: "New tasks can start." or "New tasks can't start: … problems below.", with the
time of the last check. **Check again** repeats the checks.

Only a failing check is explained, with what to do:

| Check | When it fails |
|---|---|
| **Database** | The gateway can't reach its database. Check that the database is running and the gateway's database address is right. |
| **Key store** | The gateway can't open its key store, so tasks can't get their keys. Check the store's files and permissions on the gateway host. |
| **Environment engine** | The gateway can't reach Docker (or the microVM engine), which runs environments. Check that it is running and reachable from the gateway. |
| **Agent image** | The agent's image is missing on the engine host (on microVMs, or its VM disk image isn't built). Build or load it there, then check again. |
| **Agent version** | The agent's image is older than this gateway. Rebuild it from this release, then check again. |

Passing checks are listed in one line under the answer, followed by how environments run (as Docker containers or as
microVMs). The **Default environment** section says whether new tasks can start in it, and why not, with
**Open Environments**; while a check above fails, it says "Ready once the problem above is fixed" instead of
**Available**.

**Before a restart** says how many tasks are working on a step: each of their steps would start over if the gateway
restarted now (see [Restarting the gateway](README.md#restarting-the-gateway)). It counts tasks whose environment is
connected and whose run is going, and never says which tasks or whose. The page reads it again every 30 seconds
while it's open, with **Checked at** updated. "No task is working on a step, so restarting now wouldn't start any
step over." is the moment to restart. A screen reader announces the count when it changes, never on each read. The
section is left out when the count can't be read. If one of those reads fails (say, while the gateway restarts), the
page keeps the last one and says "The latest read failed, so this shows the one before."; the next read clears it.

A **Git credentials** section appears only while some Git credentials from an earlier release couldn't become keys.
"N Git credentials couldn't become keys: their address isn't a web address" means a person saved one for an address
such as `git@github.com:team/repo.git`: they add the key under **Keys** by hand, with the repository's `https://`
address. "N Git credentials are waiting to become keys" means their owners already have the most keys: each becomes a
key at the next restart after its owner removes one. Neither stops tasks from starting.

## Models
<!-- Tests: GW-U-153, GW-U-154, WEB-U-039 (including the route check, 099b-2), WEB-U-181, WEB-U-095, GW-U-092, WEB-U-056, WEB-U-057, WEB-U-058, WEB-U-061, WEB-U-281, WEB-U-283..WEB-U-295, WEB-E2E-006, WEB-E2E-039, WEB-E2E-040. -->
A model route is one model on one server, with the credential and settings the gateway uses to call it. People pick a
route by its display name when they start or continue a task.

A change applies to new tasks, and to open tasks from their next message; a message already running keeps its route. A
rotated key or a changed server address applies to the next model request at once.

### The list
The **Model routes** table shows, for each route:

| Column | Meaning |
|---|---|
| **Route** | The display name (or the route name, when it has none) |
| **Model** | The model name. When the routes don't all use the same protocol, it is under each model: **Chat Completions**, **Responses** or **Messages**. A route's own page always names it, on its "Connects to" line and under **Advanced** › **Connection** |
| **Key** | The name of the shared key the route sends (as Keys › Shared names it; the full `secrets/<name>` reference on hover), **No key**, **Key missing** when the named secret does not exist yet (with a link to **Add it under Keys › Shared**), or **Key needed** when the route has no usable key (use **Paste key** in the route's menu) |
| **Source** | **Configuration file**, **No longer in the configuration file** or **Added here** (see [Files and the settings pages](README.md#files-and-the-settings-pages)). Shown once any route comes from the configuration file; when every route was added on the page, it is left out |

Each route has **Edit** and a menu (the three dots) with **Check**, **Paste key** when its key is needed, and **Delete**. A
**Delete** that isn't possible says why in the menu.

With no route yet, the page says "No model routes yet. Add one so people can start tasks.": nobody can start a task
until there is one.

**Download models.toml** saves every route in the file format the installation starts from, so the current
routes can be kept, reviewed or copied to another installation. The file names each route's key but never holds its
value, as the line under the button says.

### Check a route
**Check**, in a route's menu, opens a dialog that tests the stored route the way a task will use it: a short
streamed request with one tool call and its result. Nothing runs until you choose **Run check**, because a check uses
a few tokens; it can take up to a minute. A local model (on this computer or your network: a loopback address,
`localhost`, the container host, a single-label name such as a compose service like `ollama`, a `.local` or
`.internal` name, or a private, link-local or Tailscale address; or a route without a key) is checked by its
progress instead: each answer may take 2 minutes to start (`[relay] local_check_timeout` in `gateway.toml`, 1-600
seconds), and then the check waits as long as the model keeps answering, with no pause longer than `[relay]
idle_timeout` (60 seconds by default), for up to 10 minutes in all. A model on a computer without a suitable GPU can
take minutes: the dialog says "Checking… local models can take a minute or two.", and after the first minute how long
it has waited. A check that runs out of time says what to do next: for a local model, that it may be busy with another
task or slow on this computer, to try again when it's idle, and that setting; for another route, to try again in a
moment.

A local model that works but answers slowly (under 15 tokens a second) passes with its speed in place of the "…":
"Verified, but slowly: this model answers at about … tokens a second here, so a task's step can take minutes, and
helpers or a Desktop task far longer. A computer with a suitable GPU, or a hosted model, is much faster. See the user
guide's Running without a GPU section." The check's
details show the speed of every check that had usage. A local model that answered the tool call and the round trip
but ran out of time in the last step, the two-line test value, passes the same way, with that step's warning: it
works for tasks, slowly.

The result starts with a verdict, "Verified: this route works for tasks." or "The check failed." with the reason, and
the button then reads **Run again**. What was observed is folded under **Details**: **Streaming**, **Tool call**, **Tool result round trip**, **Strict tools keep line breaks**,
**Usage reported**, **Reasoning level accepted**, **First response**, **Total time** and **Tokens used**. When the
server answered with an error, its status and a short, redacted excerpt appear under **What the server answered**, as
plain text.

The route's row then shows the latest result: **Verified**, **Check failed**, or **Check out of date** once the route's
settings have changed since. A check describes the route's settings when it ran, not its key: after changing a key,
check again.

| Message | What to do |
|---|---|
| This route has no key yet. | Add the shared key the route names (the dialog links to **Keys**, **Shared**) |
| A check of this route is already running. | Wait for it to finish |
| Checked too recently. Try again in … s. | A route can be checked once every 30 seconds, and each administrator can run 10 checks a minute |
| The check ran out of time and stopped. Local models can take a minute or two to answer: try again once the model has loaded. | The server may be overloaded, unreachable or still loading the model; try again later |
| With strict tools the server dropped the line break from a tool’s text, and without strict it kept it; multi-line edits would fail. | Set **Strict tool schemas** to **Never** (see below) and check again |
| The model didn’t write the two-line test value, so whether strict tools keep line breaks wasn’t checked. | A warning, not a failure; check again, or try another model |
| The server held part of the answer back: its own tool-call parser takes calls the model writes in its native format. Tasks recover by asking again with the tools declared, but this route works better with Tool calls on. | A warning on a route with **Tool calls** off: turn **Tool calls** on for it, or serve the route without the server's own tool-call parsing (below) |

Some failures say what to try next:

- A route at `localhost` (or `127.x`, `::1`) that the check can't reach: inside the gateway's container, localhost
  is the container itself, never the computer a model server runs on. The result says so. Where the gateway can reach
  `host.docker.internal` (Docker Desktop on a Mac or Windows, or a host entry for it), it names the address that works,
  such as `http://host.docker.internal:11434/v1`. **Use host.docker.internal** then opens the route with that address,
  unsaved: save it, then check again. Where the name doesn't resolve (most Linux installs), it says to use the
  computer's network address with the same port, and offers no click.
- No endpoint for the route's protocol (a 404 or 405 on the check's own request, not a model the server doesn't
  know): "The server has no endpoint for this route's protocol at this address." It names the protocol the route
  speaks and the two others to try under **Advanced** › **Connection**, and says the address often ends in `/v1`. A model
  name the server doesn't know still reads "The server does not know this model or address."
- A server that refuses usage reporting: the rest of the check worked without it, and **Edit the route with this
  setting** opens the route with **Ask the server for token usage** unticked and focused, unsaved.

When the provider refused the route's tool settings, the result suggests a change, such as "The provider refused a
tool schema in strict mode." with "Try Strict tool schemas: Never", or "Try Schema keywords: Basic only".
**Edit the route with this setting** opens the route's form with that change made, unsaved: save it, then check again.

### Local models need room
A task's requests are large from the start: the agent's instructions and its tools come to about 2,300 tokens before
the person's first message, and each file the agent reads adds up to about 2,000 more. A model server that gives the
model less context than that doesn't refuse a request that's too long: Ollama, for one, quietly drops the oldest
messages, the person's request first, and the agent carries on without knowing what it was asked to do.

Ollama chooses the context itself unless told otherwise: 4,096 tokens when the GPU memory it detects is under 23 GiB,
too little for tasks; 32,768 from 23 GiB, and 262,144 from 47 GiB. Its log says which it chose at start ("vram-based
default context"). So when a local route's server is Ollama, the check also
asks it how much context the model it just answered with has (the CONTEXT column of `ollama ps`):

- below 16,384 tokens, the check fails, names both numbers and says how to raise the server's context; matching the
  route's **Context window** to it wouldn't make tasks work, so it isn't offered;
- at 16,384 or more, the check passes, with a note when the route's **Context window** is empty or says otherwise,
  and one click to set it to the server's number.

Raise Ollama's context to 32,768 tokens (more if the computer has the memory; a larger context takes more), then check
again:

- On a Mac with the Ollama app: run `launchctl setenv OLLAMA_CONTEXT_LENGTH 32768`, then quit and reopen Ollama;
  or set the context length in the Ollama app's settings.
- On Windows: quit Ollama, add the user environment variable `OLLAMA_CONTEXT_LENGTH` with the value `32768`, then open
  Ollama again; or set the context length in the Ollama app's settings.
- On Linux with Ollama as a service: run `systemctl edit ollama.service`, add `Environment="OLLAMA_CONTEXT_LENGTH=32768"`
  under `[Service]`, then `systemctl restart ollama`.
- In a container: start it with `-e OLLAMA_CONTEXT_LENGTH=32768`.

`ollama ps` then shows 32768 under CONTEXT once the model is loaded. Set the route's **Context window** to the same
number: left empty, a task plans with 32,768 tokens as a guess, and only three quarters of it. Other local servers
(LM Studio, llama.cpp) aren't asked; set their context and the route's **Context window** to the same number by hand.

### List a route's models (API)
`GET /api/v1/admin/routes/{name}/models`, with an administrator's session or token, lists the models the route's
server offers, using the route's stored key, which never leaves the gateway. Only ids and display names come back, at
most 500 (`truncated` says there were more). When the server refuses, `failure` names it as a check does, with a
redacted excerpt; many proxies have no model list and answer `not_found`. A route can be listed once every 30
seconds, and each listing is audited. The route's page offers it as **List models** (below).

### Add a route through the API
`PUT /api/v1/admin/routes/{name}` adds a route as the form does, but it doesn't bind the route to its key. A route may
read a shared key only once the key names it among its consumers. Until then, the PUT answers `409
secret_not_allowed`. So set the key's consumers first, with `PUT /api/v1/admin/secrets/{key name}/consumers`, its
list including `{"kind": "route", "name": "<the route>"}`, at the version read. This call needs an administrator's signed-in session, not an API token. The form's
**Key** and **Paste key** do this for you.

### Add or edit a route
Choose **Add route**, or **Edit** on a route: it opens on a page of its own, with **Models** at the top to go back
and **Save** and **Cancel** at the bottom; leaving with unsaved changes asks first. Adding starts with **Where does the model run?**: **A hosted service**
(its address and a key), **A server on your network** (its address, and a key only when **It needs a key** is ticked)
or **Something else** (every setting, with **Advanced** open). What most routes need comes first; the rest waits under
**Advanced**, closed until you open it (it opens by itself for a check's suggestion or a problem inside):

| Part | Fields |
|---|---|
| **Route** | **Display name** (required: it is what people pick), **Description** (optional: one line on what it is good for, shown under its name where people pick a model), **Address** (its hint gives an example that fits the answer to **Where does the model run?**, such as `https://api.openai.com/v1`, or `http://ollama:11434/v1` and "Not localhost"), **Model** (the name the server expects; when adding, its hint says **List models** comes once the route is saved) |
| **Key** | Adding: the **Key** itself, stored as a shared key named after the route when you save (numbered if the name is taken; a shared key is never overwritten), or **Use an existing shared key** for the **Shared key** field with **Paste key**. A route that sends no key says so |
| **Advanced** | **Route name** when adding (made from the display name until you type one; fixed once created, because tasks store it; `new` is reserved, in the page and in `models.toml`), **Connection** (**Protocol**; **Send the key**: **No key**, **As a bearer token**, **In a header** with a **Header name**, or **In a query parameter** with a **Parameter name**), **Limits**, **Capabilities**, **Fallbacks, in order**, **Reasoning**, and **Tool calls and instructions** with **Extra headers** and **Extra body** |

Saving a new route goes back to the list, which says it was saved and to check it before people pick it (a minute and
a few tokens), with **Check … now**, which opens its check.

When you edit a route, its connection reads as one line, "Connects to … · …", with **Change**, and its key as "Uses
the shared key …" with **Replace key**, which stores a new key for this route and never changes a shared key other
routes use, and **Use another shared key**.

Under **Limits**, **Context window** left empty makes a task plan with 32,768 tokens as a guess, three quarters of it
usable: set it to the model server's real context (see [Local models need room](#local-models-need-room)).
**Max output** left empty uses the server's own: with no Max output, a task's
requests name no output limit on Chat Completions and Responses, so the model server's own applies (the task plans its
context with 4096 tokens for the answer). Anthropic Messages requires a Max output, so the form asks for one there. A
set Max output is a cap: when the conversation leaves less room in the context window, a request asks for what's left,
down to 4096. When a model spends its whole limit thinking and writes nothing else, the task stops saying so; sending
the message again often gets an answer, and if it keeps happening, raise Max output above the limit it hit, or give
the model a thinking budget below Max output in **Extra body**, if its model server takes one. On vLLM (with a
reasoning parser, such as `--reasoning-parser qwen3`) that is `{"thinking_token_budget": 12288}` with a Max output of
16384: at the budget the server ends the thinking and the model answers in what is left. An empty
Max output is no fix: it lets a model that keeps thinking run to the context window. Some local models (qwen3 on
Ollama, for one) think before every answer and can't be told not to; **Check** says so when the route's Max output is
lower than 16384, and offers a button that sets Max output to 16384 (or to half a smaller Context window), filling it
in for you to save.
**Concurrency** and
**Requests per minute** left empty set no limit. **First response within** is how many seconds to wait for the model's
first answer (5 to 900). Left empty, it is 300 for a model on this computer (no key, or an address on this computer or
the container host), which may read a long prompt for minutes on CPU, and 60 otherwise; while a task waits past 10
seconds, the page says "Waiting for the model to start answering…". **Capabilities** are **Tool calls**, **Parallel tool calls**,
**Image input**, **Ask the server for token usage** and **Caching** (**None**, **Prefix** or **Cache control blocks**).
**Ask the server for token usage** is on by default: tasks ask the model server to report the tokens each reply used, which budgets and the
usage pages count. Turn it off only for a server that refuses the request: the route's check then fails and offers to
turn it off for you.
**Fallbacks, in order** are up to three other routes the gateway switches to when this one is unavailable, shown and
chosen by their display names; **Up**, **Down** and **Remove** reorder them. **Reasoning**, when **This route accepts a reasoning level** is on, sets the
**Request field**, the **Allowed levels**, the **Default level** and optional **Token budgets** per level.

**Tool calls and instructions** adjusts how the gateway sends tools and instructions, for providers that differ:

- **Strict tool schemas**: **Every tool**, **Only schemas that qualify** or **Never**. Some providers refuse strict
  schemas; if a check says so, choose **Never**.
- **Schema keywords**: **All**, or **Basic only**, which keeps the JSON Schema keywords most providers accept.
- **Instructions go as**: **The protocol's default**, **A system message**, **A developer message** or
  **The instructions field**. Messages routes always send them as the instructions field.

A route with **Tool calls** off has the model write its calls as text. Some model servers parse tool calls themselves
(an inference server with automatic tool choice turned on, for example): when the model writes a call in its own
native format, the server takes it, and on such a route, which declares no tools, the call never arrives. Serve these
routes without the server's own tool-call parsing, or turn **Tool calls** on for them. A task recovers either way: a
reply the server held back is asked again once with the tools declared, and the transcript says "The model server
hid a tool call". After that, the task declares the tools on every request. A route whose server refuses declared
tools stays text-only for that task.

**List models**, under **Model** when you edit a saved route, asks the route's server which models it offers, using
the route's saved address and key, and lists them with a **Filter models** box. Pick one to fill **Model**; nothing is
saved until **Save**. It works only on what is saved: after changing the address, protocol, key or headers,
save first ("Save the route first: the list uses its saved address and key."). It lists at most 500 models, says when
there are more, and names a refusal as the check does, with what the server answered. A route can be listed once
every 30 seconds; within that wait, the page shows the last list it got and says when you can list again.

Keys never go in the form's fields. Store the key as a shared key (see [Shared keys](#shared-keys)) and choose it under
**Shared key**, or use **Paste key**: it stores what you paste as a new secret, with a **Secret name** (the route's name and
`-key` by default) and the **Key**, then **Store key**. The secret is allowed for this route (if that step fails, saving
offers to allow it), and the form picks it; save the route to use it. A name already taken is refused, and nothing is
overwritten. A route whose key is deleted later, or one the configuration file gives a key not stored yet, is shown as
**Key missing**, and its model requests fail until the key is added; it can still be edited and saved. **Shared key** takes the name of a key listed there, as Keys › Shared shows it (or its `secrets/<name>` reference, as
configuration files write it); store a new key first, with **Paste key** or under Keys › Shared. Anything else, such
as a pasted key, is refused and never saved. A route whose key was pasted there by an earlier version has had that key
removed; it is shown as needing its key again and is not used until you enter one. Rotate such a key with its provider,
since it was stored in plain text.

The gateway checks a route the same way it checks `models.toml`, so the page refuses what the file would refuse. The
common refusals:

| Message | What to do |
|---|---|
| Use letters, digits, '.', '-' and '_', up to 64 characters. | Shorten or simplify the route name |
| Use the server's http:// or https:// address, without a name or password. | Remove any `user:password@` part; put the key in a secret |
| Choose the secret that holds the key, or send no key. | Pick a shared key, or choose **No key** |
| Paste the service's key, or use an existing shared key. | Adding a route with its key typed in: paste the key, or **Use an existing shared key** |
| Enter the name of a shared key, or use Paste key to store one. | **Shared key** holds something that isn't a listed key's name (a key pasted there, say): pick a listed key, or store the key with **Paste key** |
| Put credentials in a secret, not in a header. | Remove the header from **Extra headers**; use **Send the key** instead |
| That header is set by the gateway. / These body fields are set by the gateway and cannot be overridden. | Remove the field; the gateway sets it for every request |
| Fallbacks must be other routes, at most 3, without loops. | Remove the route itself, extra entries, or a fallback that falls back to this route |
| Name the request field, and make the default one of the allowed levels. | Complete the **Reasoning** part |

### Delete a route
**Delete** asks for confirmation. Tasks on a deleted route fail their next model request until the person picks
another model, so move people to another route first. A route that other routes use as a fallback cannot be deleted;
the page names those routes ("Other routes fall back to it: …"). Remove it from their fallbacks first. A route from the
configuration file stays deleted after a restart unless its entry in the file changes, and the confirmation says so.

## Shared keys
Open **Keys** and choose **Shared** (administrators only).

<!-- Tests: WEB-U-035, WEB-U-052, WEB-U-053, WEB-U-054, WEB-E2E-022. -->
Shared keys are the keys the gateway uses to reach model providers and connectors. The page shows each by its name; a
model route or a connector in a configuration file refers to it as `secrets/<name>` (in `models.toml`), which
**Who may use** states. A route or connector may use a key only once the key allows it (see
[Who may use a shared key](#who-may-use-a-shared-key)).

Values are write-only. The page shows a key's name, what uses it and when it last changed, never its value, and no
page or API returns it. Adding, replacing or deleting a shared key needs a signed-in browser session: an API token
cannot change them.

### The list
| Column | Meaning |
|---|---|
| **Name** | The key's name on one line (a long name is shortened; point at it for the whole name), and **Rotation due** when its reminder has passed. A key from the configuration file says so under its name (**Configuration file**, or **No longer in the configuration file**); a key added here says nothing |
| **Used by** | The models that send it (more than two read as a count, such as 3 models; point at it for the list), or **Not used**. Which routes, connectors and engines may use it is in **Who may use** |
| **Last changed** | When its value last changed, and its reminder |

Each row has **Replace** and a … menu with **Reads**, **Who may use**, **Reminder** and **Delete**.

### Add, replace or delete
- **Add a shared key** opens a dialog: a **Name** (letters, digits, `-` and `_`, up to 120 characters), a
  **Value**, and under **Use it for** the routes, connectors and engines that may use it, then **Add key**. Routes and
  connectors that already name the key may use it anyway. Names starting with `git-` or `connector-` are reserved for people's own keys: they
  are not listed here, cannot be replaced or deleted here, and no route or connector may use them. A name that looks
  like a key itself (a key pasted into **Name**) is refused: "This looks like a key, not a name. Put the key under
  Value and give it a short name, such as openai." A key added earlier under such a name can still be replaced or
  deleted, but no route, connector or engine may use it.
- **Replace:** enter the **New value**, then **Replace key**. For a key from the configuration file, the page notes
  that your value stays until the file's value for it changes.
- **Delete** (in the row's … menu): asks for confirmation (**Delete key**) and cannot be undone. A key that a route
  still uses cannot be deleted; the menu shows **Delete** as unavailable with "A model route uses it; change the
  route first." Change or delete the route first.

If the store is full, the page says how many keys it allows. Delete an unused key to add another.

A key can also be stored from a route's form, with **Paste key** (see [Add or edit a route](#add-or-edit-a-route)).

### Who may use a shared key
"A route or connector can use this key only when it is ticked here." **Who may use**, in the row's … menu, opens the
list of routes and connectors, grouped as **Model routes**, **Connectors** and **Decision engines**, with the key's
reference ("Configuration files refer to it as secrets/…"); tick the ones that may use the key and **Save**. The
ticked ones are the key's list; the Shared keys table doesn't repeat it. A route or connector that no longer exists is
marked "(no longer exists)" until you untick it.

A route or connector that names a key it isn't allowed to use is refused when saved, with
"… isn't allowed for this route. Allow it first." (or "for this connector"). The form offers **Allow it for …**,
which ticks it for you; then save again.

A key is also allowed without ticking it in two cases:

- A route in `models.toml` or a connector in `gateway.toml` is allowed the key it names when the file seeds it.
- A route or connector saved with a key that doesn't exist yet is allowed that key, so adding it later in
  **Keys**, **Shared** makes it work.

A new key otherwise allows nothing until you tick its users. People's own keys are never allowed for anything else;
only their owner's tasks use them.

### Rotation reminders
**Reminder**, in the row's … menu, sets how often the value should change: **No reminder**, a number of days from the
list, or **Other** (1 to 3650 **Days**), then **Save reminder**. When the value is older than that, the list shows
**Rotation due**. The reminder only tells you: the key keeps working after it. Replacing the value starts the count
again; changing the reminder alone does not.

### Reads
**Reads**, in the row's … menu, shows every time the gateway read the secret for a task, newest first, 50 at a time
(**Load older** shows more): **When**, the **Person** whose task it was, the **Task** (with **Open the task**, or
**No task**; the column shows only when a read has a task or an unknown run), what it was **Used by** (a route or a
connector), and the **Outcome**: **Read**, **No value at the time** or **Store unavailable**. A read that can't be tied to one of the task's runs says **Run unknown** under its task: the
call named no run (an older agent runtime, or the gateway's own call such as naming a task), or named a run that isn't
the task's. A connector's reads never belong to a run, so they don't say it. Values never appear. A person sees the same for their own keys on the
**Keys** page, without the **Person** column.

## Shared keys for web services
<!-- Tests: WEB-U-144, WEB-U-145, WEB-U-146. -->
Open **Keys** and choose **Shared**; **Shared keys for web services** follows the shared keys above. A key for a web
service lets tasks call that service, such as a package mirror or a team's bot account on a tracker, without the
agent ever holding it: each run gets a stand-in, and the gateway swaps in the value only at the key's web address,
for that run (ADR-0037). People add their own such keys under **Mine** (see the
[user guide](../user-guide/keys.md)); only administrators add shared ones.

A run gets the keys of the person whose message started it: their own keys, and the shared keys whose **Who may use
it** includes them, for the web addresses the task may reach. A key for an address that **Internet access** doesn't
allow is given to no run.

### The list
| Column | Meaning |
|---|---|
| **Name** | What tasks call the key (`$PYPI_MIRROR`); it opens the key's page |
| **Web address** | The host, with a path when the key is limited to part of it, and the port when it isn't 443 |
| **Who may use it** | **Everyone in this installation**, or how many people, such as 2 people: choose it to list them |
| **Last used** | When, and in which task; point at it for how many times and by whom |

### Add or change a shared key
**Add a shared key for a web service** opens **Add a shared key**, the same page as a person's own key (web address,
name, how the service expects it, and the key, write-only) with **Who may use it**:

- **Everyone in this installation** (the default), or **Chosen people**: **Add a person** offers the installation's
  people, and each chosen person shows with its × to remove them. A chosen person whose account was removed reads
  **Removed account**. With nobody chosen: "Choose at least one person, or Everyone."
- A shared key's name must differ from the other shared keys': "… is already the shared key for …. Pick another
  name."
- It must also differ from the names that the people who may use it gave their own keys, since a name means one key
  to a task.
  Saving is refused, at **Name**, when it doesn't: "Someone who may use this key already has their own key named ….
  Choose another name." The words name nobody on purpose: which member, and what they call their keys, is theirs.
  The same refusal comes when **Who may use it** grows to take such a person in.

Choose a key's name in the list to open its page: replace its value, change it, see **Show all uses** (every use and
refusal, and whose task it was), or **Delete…** it, as on a person's own key.

### Members' own keys
**Members' own keys for web services** lists every member's own key by **Person**, **Web address** and **Last used**,
never the key's name or its value: a member's key is exposed only to them (ADR-0037 Amendment 1). An administrator
can't use, change or share it, only delete it: **Delete…** in the row's ⋯ menu asks "Delete …'s key for …?", says
"Their tasks can't use … with it any more. Only … can add it again.", then **Delete key**. If the member changed the
key meanwhile: "Someone changed this key since you opened it. Delete again to delete it as it is now."

### What members see
Members get a read-only **Shared** tab too: **Shared keys you may use**, with each key's name, web address and their
own last use only. They never see who else may use a key, or anyone else's use of it. In a task, the step that used a
key names it only to the person whose run it is; anyone else who can see the task reads that a key was used, and why
one wasn't sent, without its name (see the [user guide](../user-guide/keys.md#what-a-task-shows)).

## Internet access
<!-- Tests: WEB-U-040, WEB-E2E-006, WEB-U-118, WEB-U-119, WEB-E2E-034, WEB-U-150, WEB-U-162. -->
Egress is the allow-list of hosts and ports that task environments may connect to. Everything not on the list is
refused, and the person sees a notice in the task, such as "Blocked network access to …: the network allow-list
doesn't allow it." An empty list means environments can reach nothing outside. A fresh installation starts that
way, and **New task** says so: "Tasks can't reach the internet: no sites are allowed yet." For an administrator the line
ends with **Allow sites**, which opens this page.

A change applies to each environment's next connection, including environments that are already running. A connection
that is already open is not cut; it ends at its usual idle and lifetime bounds.

If the page says "Egress is turned off in the gateway configuration", the installation is set up so that environments
are internal-only. No rule applies, and the list cannot be changed on the page; this is a setting in `gateway.toml`.

### Add, edit or delete a rule
To add a rule for any site, choose **Add a site** on the **Custom** card, under **Ready-made lists**. A link to this
page for one site (`/settings/internet-access?host=api.example.com`, with `&port=` for a port other than 443) opens
**Add a site** with it filled in, or its rule if it has one. The rules are listed under **Allowed sites**; each row has
**Edit**, and **Delete…** in its … menu, which asks first.

- **Host:** a host name or an address, such as `api.example.com` or `10.0.0.5`.
- **Ports:** 1 to 16 different ports from 1 to 65535, separated by commas, such as `443, 8443`.
- **Delete:** asks for confirmation. The next connection to that host from any environment is refused.

The list has a maximum size, which the page names when it is reached ("The allow-list is full …"). Delete a rule to
add another.

### Internal servers
A rule for a host name reaches only public addresses: a name that resolves to an address on your own network (such as
`git.corp.example` at `10.1.2.3`) is refused, so a task can't be pointed at the inside of your network by a name. For a
server you mean tasks to reach there, mark its rule as an internal server: in Settings › Internet access, tick
**Internal server** when you add or edit its rule; `internal = true` on its `[[egress.allow]]` in `gateway.toml`; or
`"internal": true` in the rule through the API. It's off unless you set it, and only for one
exact host name, never for a wildcard or an address (a rule for an exact address already reaches it).

An internal rule reaches private addresses (the RFC 1918 ranges that start 10., 172.16 to 172.31 and 192.168., the
shared range that starts 100.64 to 100.127, and IPv6 `fc00::/7`) and still never this machine's own ground: loopback,
link-local addresses (cloud metadata), the container engine's networks and their gateways, the task environments'
network pool, the gateway's own addresses, and this machine's own addresses. Each connection looks the name up once
and goes only to the addresses it checked, so a name re-pointed between the check and the connection can't move it,
and one forbidden address in an answer refuses it all.

The gateway finds the container engine's networks itself. It can't see this machine's own addresses from inside its
container, so internal rules stay off until you name them in `gateway.toml`:



```toml
[egress]
internal_never = ["192.168.1.1", "192.168.4.0/24"]
```

Until then, such a rule refuses every connection, the task's notice says internal servers are off here, the API lists
the rule with `internal_state` `internal_never_missing`, Internet access shows "Not in use yet: name this machine's own
addresses in gateway.toml ([egress] internal_never)." under it, and `tqa-gateway check` names the setting. A gateway that runs in the host's network namespace
finds this machine's addresses itself and doesn't need the setting. If the gateway can't read the container engine's
networks for a few minutes, internal rules refuse until it can again. Internet access then says "Not in use for now: the gateway can't read the
container engine's networks. It tries again every minute." under each internal rule.

In Internet access an internal rule is marked **Internal server** in the list. The choice isn't offered for a wildcard or
an address: "Only for one exact host name. A rule for an address already reaches it."

### Ready-made lists
**Ready-made lists** add what tasks usually need in one step. Each card says what it allows and how many sites it
holds, and whether it is **On**, **Partly on** or **Off**. **Add** puts its rules in the list, each marked with the list's name in the **Source** column; **Remove** takes
them out again. A host that already had a rule keeps it as it was, and the page names such hosts. A list whose rules
you have since edited or deleted says "Some of its sites were changed or removed." and offers **Add again**. **Custom**
adds any other site (see above).

**Open web: any website (ports 80 and 443)** asks first ("Allow any website?"), because the agent can then send data to any public site.

The lists are built-in presets, and their rules are fixed in the gateway:

- Package registries (`package_registries`): installing packages for Python, JavaScript, Rust and Go (`pypi.org`,
  `files.pythonhosted.org`, `registry.npmjs.org`, `crates.io`, `static.crates.io`, `index.crates.io`,
  `proxy.golang.org`, `sum.golang.org`), port 443.
- Code hosting (`code_hosting`): `github.com`, `codeload.github.com`, `raw.githubusercontent.com`, `objects.githubusercontent.com`,
  `gitlab.com`, port 443.
- Open web (`open_web`): any website on ports 80 and 443. It is not read-only: the agent can send data to any site. A name that
  resolves to a private or reserved address is still refused, as for every rule. This is the only way to allow any
  host; a typed rule can't.

The same through the API: `GET /api/v1/admin/egress/presets` lists them with how much of each is applied (`none`, `partial`, `all`).
`POST /api/v1/admin/egress/presets/{id}` adds the rules the list lacks, each marked with the preset, and keeps a host
that already has a rule as it is. `DELETE /api/v1/admin/egress/presets/{id}` removes the rules still marked; a rule you
edited since is yours and stays. Both take an Idempotency-Key and are audited per rule.

### Web search
There is no search tool. On the Workstation, the agent looks things up with what its environment already has:
- Facts come from Wikipedia's search first, with `curl` on `en.wikipedia.org`.
- The web is searched with `curl` on DuckDuckGo's plain search page (`html.duckduckgo.com`), or with headless
  Chromium on the same page if `curl` is refused or finds nothing.

It then reads the pages it picks. It is told about each only when that site is allowed. A search site may answer a
server's address with a page asking it to prove it's a person instead of results; the agent then says so and doesn't
retry. DuckDuckGo does this often from a server, Wikipedia hasn't so far.

To let tasks look things up, allow `en.wikipedia.org` and `html.duckduckgo.com` on port 443 with **Add a site** (the
link `/settings/internet-access?host=en.wikipedia.org` opens it filled in, and the same with the other host), or turn on
the open web. With only these allowed, the agent can read a result's page only on a site that is allowed too; with
the open web it can read any. The pages it reads come into the task marked as outside content, which the agent treats as
data, never as instructions.

## Limits
<!-- Tests: WEB-U-041 (including the connector call rate, G5). -->
Limits bound how many task environments run and for how long; what each may use is set per environment (see
[Environments](#environments)). The defaults come from `gateway.toml`; a value set on this page overrides the file's
value until you reset it. Under the table, **Largest environment** shows the most any environment may have, which only
`gateway.toml` sets (`[sandbox.ceiling]`).

| Limit | What it bounds | Applies to |
|---|---|---|
| **Tasks running at once** | Environments running at the same time, for everyone | Next task start |
| **Running tasks per person** | Environments running at the same time for one person; not more than **Tasks running at once** | Next task start |
| **Sleep after idle** | How long an idle environment stays awake | New environments |
| **Longest environment life** | The longest one environment runs; a busy run then continues in a fresh environment | New environments |
| **Connector calls per person per minute** | How many connector calls one person's tasks may make in a minute | Next connector call |
| **Desktop environments at once** | Desktop environments running at the same time, for everyone (default 2: each holds 4 GB); not more than **Tasks running at once**. Another Desktop task waits its turn | Next task start |
| **Keep action screenshots for** | How many days the screenshots a Desktop task takes stay viewable (default 30, at most 365) | Next clean-up |
| **Screenshots a run may send the model** | The most screenshots one run of a Desktop task sends the model (default 200, 1 to 1,000). Past it the agent is told to work from the page's text instead, so a looping model can't run up image tokens | Desktops that start after the change: a desktop already running keeps the cap it started with |
| **Desktop actions a run may take** | The most clicks, keystrokes and other desktop actions one run takes (default 1,000, 1 to 5,000). Past it every desktop action is refused for the rest of the run and the agent is told to wrap up; it can still look at the screen and use its other tools, and the next run starts at zero | Desktops that start after the change: a desktop already running keeps the cap it started with |

"Next task start" means the next time a task asks for an environment; "New environments" means environments created or
woken from now on; "Next connector call" means the next call a task makes to a connector; "Next clean-up" means the gateway's next removal of
old screenshots. Running environments keep the
limits they started with.

Below the table, **Largest environment** shows the ceiling no environment may exceed, set in the configuration file
(`[sandbox.ceiling]`): for example, "Environments can use up to 8 CPUs, 16 GB of memory, 4096 processes and 100 GB
of disk".

The table shows each limit and its value. A value set on this page says **Set here**; the others come from the
configuration file. **Edit** shows the range the gateway accepts, the **Default** (the file's value) and where the value
comes from, and refuses a value outside the range. A
duration as a number and a unit (seconds, minutes or hours), preset to the table's unit, with the range in that unit;
changing the unit converts the value (90 minutes becomes 1.5 hours). A line under a value explains it when it differs
from what you might expect: after a change to the configuration file left a value outside what the file now allows, the
gateway moved it to the nearest allowed value, and the line says what it was (for example, "Lowered from 12 tasks when
the configuration file changed; 8 tasks is the most allowed"); or the file sets a value below what can be set on this
page.

For a value set here, **Edit** also offers **Reset to the configuration file's value**, which asks once and returns the
limit to the file's value from the next task start or new environment. Each row has only **Edit**.

### Helpers (v1.1)
<!-- Tests: GW-I-106, GW-I-107, GW-I-111, GW-I-196 (v1.1). -->
From v1.1 a task can lead helpers: it starts other tasks, gives each a brief, and collects their reports
(ADR-0034; for people, [Helpers](../user-guide/helpers.md) in the user guide). Helpers are off until you turn them
on with `enabled = true` under `[agent_teams]` in `gateway.toml`. While they are on, **Limits** has a **Helpers**
section with the limits below, for the whole installation; nobody sets them per team. While they are off, nothing about
helpers shows anywhere. In `gateway.toml` the limits are under `[agent_teams]` too.

| Limit | Setting | What it bounds | Default |
|---|---|---|---|
| **Tasks may lead helpers** | `enabled` | Whether the installation has helpers at all. It is set in `gateway.toml` only, under `[agent_teams]`, and needs a restart; the page shows it as **On**, read-only ("Set in gateway.toml; a change needs a restart.") | Off |
| **Helpers per lead task** | `max_members` | Helpers one lead task has at once; at most 8 | 3 |
| **Tokens per team** | `token_budget` | Tokens for a lead task and its helpers together, counted where model calls are metered. At 80 % each working helper is asked to report what it has, and the lead to finish with their reports (it reads the notice with them, not before); at the helpers' share (below) the helpers stop at their next step and the lead writes up with the rest; at 100 % the team stops | 2,000,000 |
| Helpers stop at (no page row yet) | `helpers_stop_percent` | The share of **Tokens per team**, in percent (1–99), past which helpers' model calls are refused so the lead has the rest to write up. Set in `gateway.toml` only, under `[agent_teams]`; a change needs a restart | 90 % |
| **Time per team** | `deadline_minutes` | Time from the first helper's start; then the team stops. At most 24 hours | 120 minutes |
| **Tell the lead a helper is idle after** | `member_idle_minutes` | A helper that stopped without reporting | 10 minutes |
| **Tell the lead a helper is stuck after** | `member_stuck_minutes` | A helper working with no step finished | 15 minutes |
| **Stop an idle team after** | `idle_stop_minutes` | No message and no helper working | 30 minutes |
| **Helpers' model** | `member_route` | The model route for helpers started without an agent profile, for example a local model, so that a strong model can lead while helpers run locally. A route deleted since you opened the list is refused, and you choose again | The lead task's model |
| **Check before a team finishes** | `acceptance_check` | A command run in the lead task's workspace when it says the work is done, for example a test suite: one line, at most 4,096 bytes. It runs as the lead's shell commands do, for at most 10 minutes. If it fails or runs out of time, the team keeps working and the lead sees how it ended and the last 4 KiB of its output. A task started before an upgrade can't finish while a check is set until it is stopped and sent a message, which starts it on the new version. Empty the field for no check | None |

Each limit's row says when a change applies, as the gateway applies it: **Helpers per lead task** from a lead's next
helper (helpers already working keep going); **Tokens per team** and **Time per team** to the next team a lead starts
(a team already running keeps its budget and time); the three times within seconds, to teams
already running too. Its edit dialog says the same in full.

#### Disk
A helper working in its own copy has a full copy of the lead task's workspace, with the same disk limit, so
a lead task with the most helpers can use up to that many more copies of its files on the server.

#### Turning helpers off
With `enabled = false`, helpers that were still starting stay as they are until helpers are
on again; then each one gets its brief, or, if it waited more than ten minutes, it fails and its lead is told.

#### How helpers relate to Tasks running at once
Every helper is a task with its own environment, so it counts
towards **Tasks running at once** and **Running tasks per person** like any task, and it waits for a free place the
same way. So that a lead task and its helpers always fit, the most helpers a lead task can have is the smallest of
these:
- **Helpers per lead task**;
- **Tasks running at once**, minus one for the lead task;
- **Running tasks per person**, minus one for the lead task.

A helper that is **Done** still counts towards **Helpers per lead task**, since its lead can give it more work; so does
a helper that is **Failed** after it had started, since a message starts it again. It stops counting once it is stopped
(**Stop helper**, by the person or the lead) or the team ends. A helper that couldn't start doesn't count. At the limit,
the lead is refused another helper, told why, and carries on with the helpers it has.

#### A helper whose environment keeps stopping
A helper's environment is replaced when it stops unexpectedly, as any task's is. After the fifth loss within ten
minutes it is not replaced again: the helper shows **Failed**, and its lead is told at once, with the reason, and
decides whether to write to it again (which starts it) or stop it. The other helpers and the team carry on.

With 4 tasks running at once, at most 3 helpers. The page refuses a **Helpers per lead task** above that, and if you
lower the other two limits later, the smaller number applies from the next helper started, and beside
the value the page says how many helpers a lead task can have now, and which limit sets that. A helper waiting for its
lead doesn't keep its place: its environment is put to sleep when another task needs the place, and it wakes when its
lead writes to it. A larger team therefore needs a larger **Tasks running at once**, which means more host memory and
cores (see **Largest environment**).

## Connectors
<!-- Tests: WEB-U-043. -->
A connector gives the agent tools from another service, such as searching your team's documents or filing an issue in
your tracker; technically, it is a remote tool server (MCP). The gateway holds its credential and sends it; the agent
never sees one. A change applies from each task's next connection.

### The list
| Column | Meaning |
|---|---|
| **Connector** | The display name (or the name, when it has none), with the server's host and the source under it |
| **Tools** | **All tools**, the number allowed, or all but a number |
| **Status** | **Off**, **In new tasks**, and **Key missing** (with **Add it under Keys › Shared**) when its shared key does not exist |

Each row has **Edit** and a … menu with **Delete**.

### Add or edit a connector
Choose **Add connector**, or **Edit** on a connector.

| Part | Fields |
|---|---|
| **Connector** | **Name** (fixed once created), **Display name**, **Server address** (the server's `https://` address) |
| **Credential** | **Send a credential** (**No credential**, **As a bearer token**, or **In a header** with a **Header name**), **A credential is required**, and **Shared key**: a shared key (**Keys**, **Shared**) that is allowed for this connector, never a value |
| **Tools** | **Tools the agent may use**: **All tools**, **Only these** or **All but these**, with the **Tool names, one per line**, matched exactly |
| **Limits** | **Call timeout (seconds)**, from 1 to 600 |
| **Availability** | **Enabled**, and **Offer in new tasks** |

What a connector's tools may do is decided by the service, through the scopes of the credential it is given: give a
connector a least-privilege credential, and list only the tools the agent needs.

People can add their own credential for a connector (see [Keys](#keys)); theirs is
used before the shared one. Turning **Enabled** off applies at once, also to running tasks; **Offer in new tasks**
applies to tasks started from now on.

| Message | What to do |
|---|---|
| Use the server’s https:// address, without a name, password or fragment. | Remove any `user:password@` part and any `#…` |
| Put credentials in a secret, not in the address. | Remove the token from the address; use **Send a credential** and a secret |
| Use Authorization or a header name starting with X-, up to 64 characters. | Rename the header |
| A connector or tool server already uses this name. | Choose another name |
| … isn't allowed for this connector. Allow it first. | Choose **Allow it for …**, then save again; or tick the connector under the secret's **Who may use** |

### Delete a connector
**Delete**, in the row's … menu, asks for confirmation. Tasks lose the connector's tools from their next connection.

## Decision engines
<!-- Tests: WEB-U-074. -->
A decision engine is a service that a task consults at each step, to choose the model, narrow the tools or stop the
step: your own, or an outside multiple-choice service that picks the model route before each step (see [Decision
engines](extensions.md#decision-engines)). A task uses one only when its profile names it. The installation keeps the
engine's key; the agent never sees it.

### The list
| Column | Meaning |
|---|---|
| **Engine** | The display name, with the engine's host under it (and **From the configuration file** for an engine from there) |
| **Sees** | **Tool names only**, **Names and arguments** or **Full conversation** |
| **Status** | **On** or **Off**, and **Key missing** (with **Add it under Keys › Shared**) when the key's secret does not exist |

Each row has **Edit**, and a … menu with **Turn off** (or **Turn on**) and **Delete**.

### Add or edit an engine
**Add decision engine** first asks **What kind of engine is it?**:
- **Your own engine**: a service of yours that speaks this product's decision protocol, asked before each step and
  after each answer;
- **A multiple-choice service**: an outside service that answers multiple-choice questions and picks the model route
  before each step.

**Continue** opens the form for that kind; **Change** beside the kind goes back to it and keeps what you typed. An
engine keeps its kind, so **Edit** opens the form directly. The form asks for:
- **Display name**, what people see here and in profiles;
- **Engine address**, the engine's `https://` address;
- **Key**: a shared key (**Keys › Shared**) that is allowed for this engine, never a value, or **None: the engine needs no key**.
  There is no default: an engine is never saved without a key by mistake;
- **Model**, for a multiple-choice service only: the model identifier the service expects, as its documentation gives
  it;
- **The engine sees**: **Tool names only** (the default), **Names and arguments**, or **The full conversation**;
- the budget: **No budget** until you choose **Set a budget**. Then **Calls**, **Tokens** (a multiple-choice service
  only) and **Per**: **In total** or **Each month (UTC)**. In total counts from when the budget was saved, and saving a
  new budget starts a new count; each month counts the whole month, even after you change the budget in it. Once the
  budget is used up, the agent decides on its own until the period ends, and the task's owner is told. A line under the row reads the budget back with big
  numbers grouped. **Remove the budget** goes back to none.

**More options** holds the rest, with good defaults:
- **Identifier**, made from the display name and fixed once the engine is added;
- **Send the key**: **As a bearer token** (the default) or **In a header** with a **Header name**;
- **Time to answer (seconds)**: up to 10 seconds, 2 by default;
- for a multiple-choice service, **Confidence needed to switch routes** (0 to 1, 0.6 by default; below it the task
  keeps its own route for that step) and **Route descriptions**: one optional line for each model route, saying when
  it suits a step, up to 200 characters and 16 routes. A description of a route that no longer exists is kept, marked
  as no longer a model route, until you empty it.

Choosing an option that lets the engine see more asks you to confirm first, and the change is recorded in the
administration audit. A task already running keeps what the engine saw when it started; after you narrow an engine,
such a task decides on its own until it next runs.

**Turn off** asks first and says what it does: tasks whose profile uses the engine are refused, and told why, until it
is on again, and then their owners send a message to continue. **Turn on** applies at once. An engine that profiles or
open tasks use can't be deleted: turn it off instead, or change those profiles first.

| Message | What to do |
|---|---|
| Choose the engine's key, or None if it needs none. | Pick the key's secret, or **None** for an engine without one |
| Enter the model the service expects. | Copy the model identifier from the service's documentation into **Model** |
| Keep this description to one line of up to 200 characters. | Shorten that route's description |
| Calls must be 1 or more, or left empty. | A limit of 0 isn't allowed: leave the field empty for no limit on it (the same for **Tokens**) |
| Use the engine's https:// address, without a name, password or fragment. | Remove any `user:password@` part and any `#…` |
| … isn't allowed for this engine. Allow it first. | Choose **Allow it for …**, then save again; or tick the engine under the secret's **Who may use** |
| … use this engine. Turn it off instead, or change those agent profiles first. | Choose **Turn off** in the engine's … menu, or choose another engine in those profiles |
| An installation holds at most 16 decision engines. Delete one first. | Delete an engine nothing uses |

## Environments
<!-- Tests: WEB-U-047, WEB-U-060, WEB-U-097, WEB-U-098, WEB-E2E-031. -->
An environment is the machine a task runs in, built from Workstation or Minimal. The gateway checks each one before
people can pick it, and a change reaches a task at its next session. Workstation and Minimal are built in.

### Built-in environments
Minimal and Workstation (later Data) are the environments the installation built. The gateway adds each one by itself
when it starts and finds its image on the engine host: nobody types an image ID. The image must be one this version
built (`deploy/build-images.sh`, with `--workstation` for Workstation): it says which environment it is and is tagged
with that name. Its tools come from the image.

- A newer build. When the installation builds it again, the gateway's next start moves the environment to the new
  image. That change is recorded in the audit and shown on the environment ("Updated to the image built on <date>");
  tasks in it move to the new image at their next session.
- Pinning a built-in to one image. A built-in is otherwise whichever image carries its tag (`tqa-runtime:workstation`)
  on the engine host, so a new build moves it. To fix it to one image instead (an installation that must run exactly an
  approved build, or installations sharing one engine host whose builds differ), name the image by its ID in
  `gateway.toml`:

  ```toml
  [runtime.builtin_images]
  workstation = "sha256:<the image's 64 hex digits>"
  ```

  Only `minimal`, `workstation` and `data` can be named, and only by image ID (`docker image inspect --format
  '{{.Id}}' <image>`), never a tag: anything else stops the gateway, saying which. A pinned built-in is that image,
  provided it is on the host and says it is that environment. Its tag is not used, so a new build doesn't move it, and
  while the image isn't on the host the built-in isn't added. The ones not named are found by their tag as before.
  Everything else here applies unchanged: a pinned built-in is registered, checked and becomes the default the same
  way.
- An image built from it (a customised Workstation, for example) never takes its place. It is listed among the
  images on the host as derived from Workstation, and can be picked for a new environment of its own.
- Editing a built-in makes it yours: it is marked "Customised: rebuilds no longer update it", and later builds leave it
  as you set it. It is still checked against the build it was customised from, so a change of its size, name or
  description keeps it available (Desktop included, whose entry point differs from Workstation's); a new image is
  checked as any environment's is. A Desktop customised under an earlier version and refused for its entry point is
  checked again at the gateway's next start, when it is still on the image its tag names. **Reset to built-in**, in its
  … menu or on its page, asks first, then hands it back: it takes the current build's image, name and tools, keeps its
  size, description and on/off switch, and follows later builds again. The reset is recorded in the audit. It needs the
  built-in's image on the host ("build it again, then reset" otherwise), and for the default environment a reset that
  would change its image or tools is refused, as an edit would be.
- On a new install, Workstation appears and becomes the default once it is found and its checks pass (a few seconds
  after the gateway starts), so new tasks have Python, Node and the rest (and, as in every environment, `python` runs
  Python 3). That happens only while nobody has chosen a default: when `gateway.toml` lists its own environments, or an
  administrator has already set environments up, their default stays. Without Workstation on the host, or while its checks refuse it, Minimal stays the default. On microVM
  installs Workstation needs its own disk image: its row says when it isn't built, with the command that builds it.
- Your own environment of the same name, or one in `gateway.toml`, is kept, and the built-in isn't added.
- Deleting a built-in is final: a later build doesn't bring it back. Add it again from its image when you want it:
  **Add environment**, then its card under **Start from a built-in**. It comes back with every tool its image declares
  and its own size (Desktop's 2 CPUs and 4 GB), as your own environment, so later builds don't update it.
- When the container engine doesn't answer at start, the environments stay as they were; the next start tries again.

### The images on the host (API)
`GET /api/v1/admin/environment-images` (administrators) lists the images on the engine host that hold a TensorQuay
runtime, newest first, at most 200: each image's ID and tags, when it was built, its size, which environment it says it
is and its tools, whether this version can run it, which environments use it and how many tasks run it now. An image
that can't be used says why in plain words ("too old for this version: rebuild it", "its capability label names an
unknown capability: gpu"); a built-in that isn't added says why ("deleted by an administrator"). A built-in with its own size says it in `sizes`
(Desktop: 2 CPUs, 4096 MiB, 1024 processes; null for the others). The gateway never pulls:
only what is on the host is listed. When the container engine doesn't answer, the list answers `503 engine_unavailable`;
try again in a moment. An empty list means no such image is on the host yet: build them with `deploy/build-images.sh`.

### Build one from a recipe
To add tools without writing a Dockerfile, write a recipe: the built-in to start from and the packages to add from its
own sources. On the engine host, from the repository:

```toml
name = "data-r"                       # the environment's name: lowercase letters, digits, - or _
display_name = "R and LaTeX"
base = "workstation"                  # minimal or workstation
capabilities = []                     # extra tools the packages bring, if any: shell, python, node, build, browser, data
[add]
apt = ["r-base=4.5.0-3", "texlive-latex-base"]
pip = ["pandas==2.2.3"]               # Workstation only
npm = []                              # Workstation only
```

```sh
deploy/build-environment.sh data-r.toml             # --dry-run prints the Dockerfile and builds nothing
```

The image is tagged `tqa-env:data-r` and appears in the images on the host, based on Workstation, with its tools; add
it as an environment from there. A recipe lists packages only: no commands, files or addresses. Name a version for
each package to build the same image every time. The image records the versions it got, and a rebuild that got other
versions says which. A later build of the recipe is a new image: environments made from the old one keep it until you
pick the new one.

When the build can't go ahead, it says why in one line and tags nothing:

| It says | What to do |
|---|---|
| The package r-bsae wasn't found in the apt sources of Workstation. | Fix the name (or the version) in the recipe. |
| The build couldn't reach deb.debian.org. Check this host's network, then build again. | The host needs the package mirrors. |
| Workstation isn't on this host: build it with deploy/build-images.sh --workstation first. | Build the base (also when it's too old for this version). |
| Minimal has no pip: build on Workstation, or list the tool under apt. | Change the base, or use the apt package. |
| `run` isn't a recipe field: a recipe lists packages only. | Remove it; anything else belongs in your own Dockerfile. |
| That name belongs to a built-in environment; choose another. | Rename the recipe. |
| The build ran out of disk space on this host. Free some space, then build again. | Free space on the engine host. |
| The build took longer than 60 minutes and was stopped. | Raise the bound with `--timeout MINUTES`. |

### The list
| Column | Meaning |
|---|---|
| **Environment** | The display name, **Built in** or based on its base, and the source (**Found on this host** for a built-in the gateway registered itself) |
| **Tools** | The tools it declares, each by name: **Shell**, **Python**, **Node.js**, **Build tools**, **Browser**, **Data** |
| **Status** | **Default**, **Off** and the check state, with the reason when it can't be picked |

Each row has **Edit**, which opens the environment's page, and a … menu with **Check the image now**,
**Reset to built-in** (on a customised built-in, whose row says "Customised: rebuilds no longer update it"),
**Make default** (when it can be) and **Delete**.

The check state is one of:

| State | Meaning |
|---|---|
| **Verified** | The image passed every check; people can pick it |
| **Checking…** | A new or changed image is waiting for the gateway's check, which comes within a few minutes; the page refreshes on its own |
| **Not checked** | It has not been checked; choose **Check the image now** in its menu |
| **Refused** | The image failed a check; the sentence under it says why and what to do |

People can pick an environment only when it is enabled and either verified, or built in and not yet checked. A refused
image reads, for example, "The image does not start from Workstation or Minimal. Build it FROM one of them." On
installs that run tasks in microVMs, an image also needs its VM disk image, built by an operator with
`images/rootfs/build_rootfs.py`; the refusal names it, and **Check the image now** checks again once it is built.

### Credential files in an image
The check also looks through the files an environment's image adds to its base for credentials left there by mistake:
private keys, SSH keys, saved logins for a code host, AWS, Docker, npm and PyPI credentials, and `.netrc` passwords. It reads names and the first
bytes of files, never runs anything, and shows only paths, never what a file holds. An image with such a file is refused,
and the page lists the paths with what each one is.

- Remove the file and build the image again: a credential in an image reaches every task that uses it. Tasks get their
  credentials from the secret store instead. Keep the file out of the build itself: deleting it in a later build step
  hides it from tasks, and from this check, but it stays inside the image's earlier layer, where anyone with the image
  can extract it.
- Or acknowledge it, when the file is meant to be there (a test certificate's key for a development server, for
  example): list its path under `acknowledged_secret_files` in that environment's `[[templates]]` entry in
  `gateway.toml`. This is set in the file only, not on the page. The environment is then verified, with the path shown
  as a warning.

Very large images can pass the limits of this look-through (`[template_checks]` in `gateway.toml`, 160,000 files and
6 GiB by default). Such an image is refused rather than passed unchecked; raise the limit it names if the image is meant
to be that large. The look-through catches common mistakes, not every possible secret: what an image holds stays the
administrator's responsibility.

### Add or edit an environment
**Add environment** opens a page of its own (`/settings/environments/new`), and **Edit** opens the environment's page
(`/settings/environments/<name>`); **Environments** at the top goes back, and **Save** and **Cancel** stay at the
bottom. Leaving with unsaved changes, by any link or by closing the tab, asks first. Give the **Display name** and
**Description** (optional: one line on what it is for, shown where people pick an environment, with its tools on
hover). The environment's ID is made from the display name and shown under it ("ID: data-science"); **Change** opens it
to type your own: lower-case letters, digits, - and _, starting with a letter. The ID is fixed once the environment
exists, and `new` is reserved (it is the Add page's address).

Then its **Image**, chosen rather than typed. Build or pull it on the engine host first: the gateway never pulls.

- **Start from a built-in**: a card for each built-in's current build, with its tools. Choosing one takes that build's
  image and ticks its tools.
- **Or pick one of your images**: the other images on the host, each by its tag ("Untagged image" without one), with
  what it is based on, when it was built, its size and which environments use it. One that can't be an environment is
  shown but can't be picked, with why ("Too old for this version: rebuild it from a current Workstation or Minimal.").
  Picking one takes its image and the tools it declares. With none on the host the page says so: build them as the
  Built-in environments part above says.
- **Advanced** holds the image ID itself, pinned by digest (`sha256:…` or `name@sha256:…`, never a tag), for an image
  the lists don't show; it opens by itself when the host's images can't be listed or the environment's image is none of
  the choices. When adding, **How to build one** there gives a starter file (from Workstation, as root, then back to the
  runtime user) and the commands that build it and read its ID.

A new environment starts from the default built-in, already chosen. A built-in sets its **Capabilities** ("Set by
Workstation: Shell, Python, …"); for your own image or an image ID, choose them: they are declared, not detected, so
the agent gets only the tools they allow. Its size may stay empty to use the installation's defaults:
**CPUs** (in halves, from 0.5), **Memory (GB)** (from 0.25), **Processes**, and **Disk for new workspaces (GB)** (a
workspace made for this environment gets this much space; an existing workspace keeps its size). No field may exceed
the installation's ceiling (`[sandbox.ceiling]` in the configuration file, shown under Limits as **Largest
environment**), which each field's hint names; a value above it is refused. On microVM installs a workspace's disk is an
image of that size, made once: it never grows or shrinks, even when its task moves to another environment. If the
configuration file later lowers the ceiling below an environment's size, the list shows what tasks get, marked ("8 CPUs,
lowered"), the field says "Lowered to the installation's ceiling of 8 CPUs", and new tasks get the ceiling. Saving the
environment accepts the lowered size and removes the mark, even when you changed only another field; a ceiling raised
later no longer gives the old size back. A running task keeps the size it started with; a change applies from its next
environment. **Enabled** off hides it from the picker; tasks already on it keep working.

Saving starts the gateway's check. The default environment stays enabled, and its image, capabilities, CPUs and memory
cannot change while it is the default (its processes and disk can): make another environment the default first.

### Make default and delete
**Make default**, in the row's … menu, appears only on environments people can pick now, and asks for confirmation:
new tasks start in it, and existing tasks keep theirs.

**Delete**, in the row's … menu, is unavailable for the default ("Make another environment the default first.") or while
tasks use the environment ("… tasks use it; they can switch to another first."); the menu says which. Deleting removes
it from the list only: its image and VM disk image stay on the engine until an operator removes them. If something still
uses it, the dialog says what: the default, active tasks, or tool servers that list it.

## Usage
<!-- Tests: WEB-U-034, WEB-E2E-022. -->
Usage shows the model tokens used through the gateway, counted from each model request.

Choose a **Period**: **This month** (the default), **Last month**, or the last 7 or 90 days. The page leads with the
period's tokens (in and out together) and their change against the period before it: this month against the same days
of last month, last month against the month before, a run of days against as many days just before. Under it are the
requests, tokens in and tokens out, and, when a model server reported them, a line with the cached and reasoning
tokens.

Then a bar for each day of the period (a day without requests is a gap), and two rankings side by side,
**Usage by person** and **Usage by model**, largest first with each one's share of the tokens: the top five, and
**Show all … people** or **Show all … models** for the rest. Requests by an account that was since removed count as
**Removed account**. **Show each day** lists the days with requests, newest first.

The counts include the one small request the gateway makes to name a task after its first run completes (a few
words, on the task's own route, in the task's language: a name in another writing system than the task's, such as a
Chinese name for an English task, is not used, and the task keeps its first words); `[tasks] auto_title = false` in
`gateway.toml` turns that off.

The page counts tokens, not money: routes have no prices. Decision engines count against their own budgets, on the
**Decision engines** page.

Two notes can appear:

- Some requests reported no token usage. Their tokens are not counted; the note says how many requests. Tasks ask
  for usage unless the route's **Ask the server for token usage** is off (see [Models](#models)); a server that still sends none
  doesn't report it while streaming.
- A long ranking shows only the largest rows. The totals still include the rest.

## Account
<!-- Tests: WEB-U-078. -->
Every person has this page. It shows the person's **Email** and **Role**, and sets the **Theme** and **Language**,
as the Account menu also does.

## API tokens
<!-- Tests: WEB-U-023, WEB-E2E-020. -->
Every person has this page. Programs and scripts use an API token instead of the person's password, sent as
`Authorization: Bearer <token>`. A token acts as the person who created it, with their role.

**Create token** asks for a **Name** and when it **Expires in**: 7, 30, 90 (the default), 180 or 365 days. The token
is shown once, when it is created: copy it then. The list shows each token's name, a short hint of the token, when
it was **Created**, when it **Expires** and when it was **Last used**. An expired token stays listed, greyed and marked
**Expired**, until it is revoked. **Revoke token** asks first; revoking stops programs using the token at once. A
person can hold up to 50 tokens.

An API token cannot change shared secrets or Git credentials; those need a signed-in browser session. An
administrator's token can change model routes, egress rules and limits through the API.

The API is described in `crates/tqa-gateway/openapi.json` in the repository (OpenAPI 3): every route, its fields and
its refusals, with the same error codes the pages explain.

## Keys
<!-- Tests: WEB-U-076, WEB-U-044, WEB-U-053, WEB-U-141, WEB-U-149. -->
Every person has this page: the keys the agent uses for them. First their **Keys for web services**, then
**Keys for connectors**, in one table by the connector each is for, with "For a connector" under its name; each
section says under its heading what it is for. People never see git words here. **Add a key for a connector** asks "Which service is it for?" first: each connector that takes a person's
key, the ones still **Needed** first. Everyone switches between their own keys, **Mine**, and the **Shared** keys:
an administrator manages those (see [Shared keys](#shared-keys)), a member reads the ones they may use.

Keys are write-only: after saving, a key is never shown again. Each person's keys are their own: nobody else,
administrators included, can see or use them. Saving needs a signed-in browser session; an API token cannot change
them. **Reads**, in a row's … menu, lists each time a task of yours used that key, as the Shared keys page's
[Reads](#reads) do.

### Keys for web services
<!-- Tests: WEB-U-141, WEB-U-142, WEB-U-143. -->
**Keys for web services**, above the rest, are the keys a person's tasks send to a service's web address, such as an
API token for GitHub: **Add a key for a web service**, each key's page, and what a task shows when it uses one are in
the [user guide](../user-guide/keys.md). An administrator sees that members' keys exist, never their names or values
(see [Members' own keys](#members-own-keys)).

### Keys for Git hosts
<!-- Tests: WEB-U-149, WEB-U-142. -->
A key for a Git host is a key for a web service (ADR-0037 §7): add one for the host's address, such as
`github.com/acme`, and Git over https in a task uses it by itself. The keys people used to save for a project link were
copied into keys for their web addresses, each marked **Check the name**; Keys lists no project keys any more, and
**Add a key for a connector** offers none.

The agent's own tool for saving changes is retired; older tasks still show its lines. Where one failed for lack of a
key, it reads "Couldn't save the changes to example.com/team/report" and links to "Add the key for example.com" (or
"Add a key in Settings › Keys" when it named no site), which opens **Keys**. The work stays as it was in the workspace; after adding a key for that web address, ask the agent to save
the changes again.

### Keys for connectors
<!-- Tests: WEB-U-044. -->
Each connector that takes a person's key is listed with its state: **Added**, **Needed** (the connector requires one
and none is held) or **Optional**. A connector that sends no key isn't listed.

- **Add** or **Replace** opens one masked field, **Key** or **Header value**. The value is sent once to the gateway and
  never shown again; it applies from the agent's next connection to that service.
- **Remove**, in the row's … menu, takes the person's key away; the shared key applies again, if the administrator
  set one.
- A key counts only for the connector's current address: if an administrator moves the connector, add it again.

## Agent profiles
<!-- Tests: WEB-U-049, WEB-U-059, WEB-U-104, WEB-U-105, WEB-E2E-033, E2E-PROFILE-018. -->
Every person has this page (its old address, `/settings/profiles`, leads here). An agent profile sets a task up for
one kind of work, such as a Researcher with web access or a Reviewer who only reads: its instructions, and the model,
environment and tools it uses. Models and environments are what an administrator sets up; a profile chooses among
them. People choose one in the New task picker when they start a task; what the profile sets is then fixed for that
task, and the task's bar says so. With no profiles at all, the page shows one empty state with **Add agent profile**.

### The list
**Your agent profiles** are only yours. **Shared agent profiles** are for everyone, and only administrators add, edit
or delete them; anyone can **Duplicate** a shared profile to make their own copy. A profile you can change has
**Edit** and a … menu with **Duplicate** (for a shared one) and **Delete**; a shared one you can only read has
**Duplicate** alone. **Add agent profile**, **Edit** and **Duplicate** each open the profile's own page. **Status** reads **Ready**, or
**Can't be used** with the reason, such as "Its model route no longer exists." or "A tool server it lists no longer
exists." Fix what the reason names, or edit the profile.

### Add or edit an agent profile
Choose **Add agent profile**, or **Edit** on a profile: it opens on a page of its own, with **Agent profiles** at the
top to go back and **Save** and **Cancel** at the bottom; leaving with unsaved changes asks first. "An agent profile can only take away from what this
installation allows. It can lower a limit and choose fewer tools, skills or connectors, never more." There is no
approval policy and there are no rules: the task's tools decide what the agent can do, and the network allow-list
(**Internet access**) decides what it can reach. **Save** saves it. A shared profile you can't change opens read-only,
with **Close**.

| Part | Fields |
|---|---|
| **Basics** | **Display name**, the ID made from it ("ID: code-review"; **Change** to type your own; fixed once created), **Description** |
| **Instructions** | **Instructions for the agent**, up to 16 KiB |
| **Model and environment** | **Model** and **Environment**: one of each, or **The task chooses** |
| **Tools** | Every built-in tool, ticked unless the profile leaves it out, with **All** to tick or clear them at once (see below) |
| **Skills** | **Skills offered**: **All skills**, **No skills** or **Only these skills**, with the **Skill names** |
| **Connectors** | The connectors the agent may use; on a shared profile, also its **Tool servers** |
| **Limits** | **Steps per run**, **Seconds per run**, **Output tokens per reply**, **Bytes per tool result**, and **When the workspace is busy** |
| **Sharing** | For administrators: **Shared with everyone** on a new profile |

The tools are shown as cards, one per group, with **All** above them; each tool has a line on what it lets the agent
do: **Files** (**Read files**, **Write
files**, **Edit files**), **Finding** (**Find files**, **Search file contents**), **Commands** (**Run commands**: run
commands in the task's computer), **Notes**, **Web** (**Fetch web pages**), **Skills** (**Use skills**) and
**Channels**. A profile with every tool ticked allows every tool, including ones a later version adds; untick a tool
to leave it out (at least one stays). A profile of read-only tools (**Read files**, **Find files**, **Search file
contents**) makes tasks that can look but not change anything.

Among the tools are the four channel tools: **Post to channels**, **Read channels**, **Wait on channels** and
**Claim channel tasks**. A task has them only while it belongs to a channel, so leaving them ticked costs nothing
elsewhere. An empty limit uses the installation's.
**When the workspace is busy** decides whether a new run **Wait for it** or **Don't start** while another task uses
the workspace.

**Tool servers** are the tool servers (MCP) that run beside a task in its environment (see
[Extensions](extensions.md)). On this page only a shared profile chooses them, so an administrator sets them. Each
one is listed with where it runs, for example on by default, or only in some environments. Turning
**Shared with everyone** off removes them. A profile can list at most 8 connectors and tool servers together.

A profile's rules, which ask or deny before a tool runs, are set through the API; the form shows how many there are.

### Delete an agent profile
**Delete** asks for confirmation (**Delete agent profile**). "Tasks already made from this agent profile keep what it
set; only new tasks lose it."
