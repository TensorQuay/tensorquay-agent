# TensorQuay Agent: administrator's guide

This guide is for the people who run a TensorQuay Agent installation. It describes the product as it is today;
installation itself is covered by the deployment README in `deploy/`.

| Section | Content |
|---|---|
| [Settings pages](web-settings.md) | Administration (Overview, People, Models, Environments, Connectors, Decision engines, Internet access, Limits; shared keys under Keys › Shared) and Operations (Usage, Status) for administrators; Your account (Account, API tokens, Keys, Agent profiles) for everyone |
| [Extensions](extensions.md) | Connectors, tool servers in the sandbox (`[[mcp.servers]]`) and skills: which to choose, how to define and test one, and which tasks get it |
| [Decision engines](decision-engines.md) | Adding an engine with your own key, what it sees, when it is asked, what bounds its use and what happens when it fails; the [API reference](decision-engines-api.md) for its endpoints |

## Who is an administrator
Every account has one of two roles: administrator or member. Only administrators see the administration sections in
**Settings**: the **Administration** and **Operations** groups. A member who opens one of those pages directly
sees a short notice, such as "Only administrators can manage model routes.", and nothing else. Every member has
the **Your account** group: **Account**, **API tokens**, **Keys** and **Agent profiles** (their own;
shared agent profiles are for administrators to change).

## Files and the settings pages
Model routes, egress rules, limits and shared secrets can be set in two places: the configuration files the
installation starts from (`models.toml`, `gateway.toml` and the secrets seed file) and the settings pages. The rule
between the two is simple:

- The database holds the live settings. The files seed them.
- A change made on a settings page survives a restart.
- When you edit an item in a file, the file wins at the next start, and the page then shows the file's value.
- An item deleted on a page stays deleted after a restart, unless its entry in the file changes.
- An item removed from a file is kept, and the page marks it "No longer in the configuration file". Delete it on the page if it
  is no longer needed.

The **Source** column says where an item came from: "Configuration file", "No longer in the configuration file", or
"Added here" ("Set here" for limits; "Found on this host" for a built-in environment the gateway registered itself).
Models leaves the column out when every route was added on the page.

Every change, from a page or from a file, is recorded in the administration audit with who made it and when. Secret
values are never recorded. A change an upgrade makes to stored settings (for example, removing a setting a release retired)
is recorded the same way and shows as made by the configuration file ("seed"), with no person.

## What the agent may do
Tasks do not stop to ask for approval (ADR-0035): every tool call runs. The guardrails are around the agent, not in
front of each call:

- The sandbox: Each task runs in its own environment with its own workspace, isolated from the host and from other
  tasks.
- The network: An environment reaches only the hosts on [Internet access](web-settings.md#internet-access), and
  the agent is told which ones. Keep the list to what tasks need.
- The credentials: keys and connector credentials stay in the gateway. A person adds their own key for an outside
  service in [Keys](web-settings.md#keys), and the gateway puts it into that person's tasks' requests at the network
  edge, so a task never holds it. A git host is an outside service like any other: tasks clone and push with plain
  `git` and the person's key. What a key allows is what a task can do with it, so choose keys narrowly:
  - Protect important branches on the git host, such as `main` and release branches. The gateway has no branch rules
    of its own: a task may push a new branch or update an existing one, and where the host allows force pushes, that
    update may rewrite the branch's history. The host's own rules decide, and a refusal reaches the agent.
  - Use narrowly scoped tokens: one repository or project where the service allows it, with write access only where
    tasks need it, and an expiry.
  - Give connectors least-privilege credentials: a service account or token that can do only what tasks should do
    in that service, whether it is a shared key or a person's own.
  - Removed in this release: the Git relay, the `git_push` tool, the stored Git credentials and their routes, a
    profile's `repositories` switch, and `git_url` on a new workspace. A request that still sends `repositories` or
    `git_url` is refused, naming the field. Each person's Git credentials became keys of theirs: at the first start
    after the upgrade the gateway copies any not yet copied, then removes the original. One whose address isn't a web
    address (for example `git@github.com:team/repo.git`) can't become a key: it is kept, unused, and **Status** in
    **Settings** says how many, so the person can add the key under **Keys** by hand.

## Deleting tasks and workspaces
A delete is final (PRD §12 item 18); there is no undo and no recycle bin.

- Deleting a task: it is gone for everyone at once. Reading it, its events or files, its live updates and any action on it
  answer *not found*, and a page open on it ends its live view. Within a few minutes the gateway removes its
  conversation, stored outputs (large outputs kept as files too), held messages, saved changes of its runs and its
  environment's own state. The workspace and its files stay, also for a task that made its own workspace when it
  started; delete the workspace to remove them.
- Deleting a workspace: its files go at once; its tasks and their data, its saved changes and its shares are removed within
  a few minutes. Copies made from it stay, each a workspace of its own.

What stays, by design:

| Kept | Why |
|---|---|
| One audit record per delete: who deleted which task or workspace, by id, when (actor `system` for workspace retention) | Accountability; it holds no title or content, and ages out with the other audit records |
| Token usage, counted for the task's owner | Past usage and limits must not change when a task is deleted |
| Channel posts the task made, shown as by "a deleted task" | They belong to the channel and its other members |
| Key read records (ids only) | They age out after `read_audit_days`, like the rest |

Two consequences for API clients: deleting a task that is already deleted answers as the first delete did (`204`), so a
retry after a timeout sees no error, and answers `404` once the task is purged. A create retried with the same
idempotency key after its task was deleted and purged makes a new task.

The removal runs in the background: once a minute, and at once after a delete. A task whose environment is still
stopping waits for it. Two metrics show it working: `tqa_purge_targets_total{kind, outcome}` (`done`, `deferred`,
`failed`) and `tqa_purge_oldest_seconds`, the age of the oldest deletion not yet finished. Alert if that gauge stays
above a few minutes; the gateway log names a failing step (`purge step failed`) with the task's or workspace's id.

## When two administrators edit at once
Each item carries a version. If someone else saved the same item after you opened it, your save is refused with a
message such as "Someone changed this route since you opened it." Depending on the page, your edits are kept so you can
save again, or you reload to see the current value first. Nothing is overwritten silently.

## When tasks are slow to start
On a busy host the container engine can take tens of seconds to create a task's container, and sometimes too long to
answer. `[sandbox] create_budget_seconds` in `gateway.toml` (default 120, 60 through 600) is the longest a person sees
"Starting…" before a start fails: it covers all of one start's container work, retries included. It can't be set below
60, which is how long one container create may take on its own. Within the budget, a create that times out is tried
again, at most three times in all, after waiting about 1 s and then about 2 s (with a little random spread so that
starts retried together don't hit the engine at once). The number of tries and the waits are fixed; the budget is the
one setting, since it is the time people feel. The gateway's README has the details.

## When the stack does not come back
If the host ran out of memory and the container engine was stopped with it, the installation may not start again:
Docker reports `container with given ID already exists`, or an old database process is still running beside the new
one. The deployment README in `deploy/`, under "After the Docker daemon was killed", lists the steps. Stop an old
database gently before anything else, and check it before you start the stack.

## The certificate authority for keys to web addresses
Tasks use keys for web addresses without seeing them (ADR-0037): for a keyed address the gateway's proxy answers the
task with a certificate from the install's own certificate authority, which only sandboxes trust, never people's
browsers or the host. The gateway makes it at its first start, as two files in its own private directory,
`<storage.root>/host-ca` (mode 0700): `host-ca.pem` and its private key `host-ca-key.pem` (mode 0600, gateway only).
No task's sandbox mounts that directory. Treat the key like the install key, and note that backups of the storage
root now include it: keep them as private as the install key.

`[host_keys] ca_directory` puts it elsewhere; the gateway refuses a relative path, one inside a directory a sandbox
mounts (the storage root's `workspaces`, `runtime-state` and `skills`, or the installation's skills), and an existing
directory that others can read, saying which.

To replace it, for example if the key file may have been copied, run this with the gateway's configuration file; the
gateway can keep running:

```sh
tqa-gateway rotate-host-ca /etc/tqa/gateway.toml
```

It takes effect at the gateway's next start. Tasks already running keep working: their sandboxes trust the previous
authority, which the gateway keeps using for them for one sandbox lifetime (`[sandbox] max_lifetime_seconds`) and then
deletes. New sandboxes trust both.

The gateway opens a task's connection only to an address the task's running run holds a key for; every other site's
connection passes through unseen, with the site's own certificate. It keeps at most 16 such keyed connections per
sandbox and 256 in all (`[egress] keyed_per_sandbox` and `keyed_connections`); past that, a new one is closed, and
a keyed address that hasn't begun its answer within two minutes (`keyed_answer_seconds`) gets a refusal. A program that brings its own certificate list, rather than
the system's or `SSL_CERT_FILE`, gets a certificate error on keyed addresses only. The gateway asks a keyed address
for an uncompressed answer, and refuses a compressed one.

The gateway checks each keyed address's certificate against the public certificate authorities. For a server behind
your own authority, such as an internal git server, name the authority's certificate in a PEM file:

```toml
[egress]
ca_file = "/etc/tqa/internal-ca.pem"
```

The file may hold several authority certificates, up to 1 MiB. The gateway reads it at start and trusts it beside the
public authorities, never instead of them, and certificate checks are never turned off. A file it can't read,
or that holds anything but authority certificates (a private key, a revocation list, or one server's own certificate
rather than its authority's), stops the start.

`[git_push] ca_file`, the old name for this setting, was removed in this release: a `gateway.toml` that still has a
`[git_push]` section stops the start (and `tqa-gateway check` fails), naming `[egress] ca_file`. Move the file's name
there and delete `[git_push]`.

A key's address can also be an exact IP address, such as `https://10.0.0.5` or `https://[fd00::5]:8443/team`, for a
server your tasks reach by address. Such a key works only through an allow rule for that exact address (Settings ›
Internet), as any task's connection to an address does: a key never makes an address reachable. The gateway's
certificate for the address names the address, and it checks the server's certificate against the address too, so a
server behind your own authority needs that authority in `ca_file` and an address in its certificate.

## Restarting the gateway
Restart when no task is working on a step, or tell the people using the installation first. The settings' **Status**
page says under **Before a restart** how many tasks are working on a step, read again every 30 seconds. A restart
doesn't lose any task: open pages reconnect on their own, and every task carries on from where it was. But a step that
was waiting on the model starts over. The model's answer in progress is lost with the restart, so the task asks the
model again from the start of that step, and its page says so: "The connection to the server dropped for … s, so the
model was asked again and this step started over." On a fast cloud model that costs seconds. On a local model that
thinks for minutes, it costs those minutes again.

## Upgrading
Before you restart on a new version, check your `gateway.toml` with that version. `tqa-gateway check <path>` reads
and validates the file exactly as start-up does, and nothing else: it doesn't connect to the database or the container
engine, so it is safe to run next to the running installation. It prints `configuration ok: <path>` and exits 0, or
prints the reason start-up would stop with and exits 2. The reason never repeats a value from the file.

With the container images, run the new image's gateway with the file mounted where the installation mounts it:

```sh
docker run --rm --network none \
  -v "$TQA_INSTALL_DIR/gateway.toml:/etc/tqa/gateway.toml:ro" \
  tqa-gateway:<new version> check /etc/tqa/gateway.toml
```

The gateway itself stops with the same line and exit status when it starts on a file that fails the check; the file's
own problems are named (`the configuration file does not exist`, `the configuration path is not a regular file`,
`configuration exceeds 64 KiB`, `the configuration file cannot be read`).

### Asking for token usage is on for every route
A route's **Ask the server for token usage** (formerly named Usage in the stream) is now on unless you turn it off, and off now means something: the gateway sends
that route's requests without asking the server for usage, for a model server that refuses the request field. Until
now it was off by default but never applied to tasks, which always asked. So the upgrade turns it on for every route
you added where it was off; a route from the seed file follows the file: on unless the file sets it to false. Nothing
changes for your tasks, and the route's **Check** now asks the same way as a task does. Each changed route shows the
upgrade in the administration audit. If a model server refuses usage reporting, its route's check fails, says why, and
offers **Edit route** to turn the switch off.

### Keys for web addresses need this version's runtime image
Keys for web addresses reach a task's runtime with the message that starts each run, and a runtime from an earlier
version can't read them. Environments built before this version keep working, but without keys: a task whose person
has keys says "This environment is too old to use your keys" and runs on without them. After the upgrade, point the
installation at this version's runtime image and rebuild custom environments from it (on microVM installs, their root
filesystems too); the deployment README's upgrade notes (`deploy/README.md`) give the steps.

### A delete removes the data
Deleting a task or a workspace now removes its data within minutes (see
[Deleting tasks and workspaces](#deleting-tasks-and-workspaces)); before, a deleted task's conversation and outputs were
kept on the server. The first start of this version also removes what was deleted under earlier versions: the
conversations, events, stored outputs and details of every task deleted before, and whatever files remain of every
workspace deleted before. Nothing that was not deleted is touched. Back up the database and the storage directory
first if you need a copy of those. The first start adds columns and indexes
(migration 0073); on a large installation the index on the usage table takes a little longer to build.

### The ready environments register themselves
Workstation (and later Data) appears among the environments by itself at the first start that finds its image on the
engine host, with its tools; nobody adds it or types an image ID. It needs images built with this version's
`deploy/build-images.sh --workstation`. Environments you made, and those in `gateway.toml`, keep their names; see
[Built-in environments](web-settings.md#built-in-environments).

### Approvals are removed (ADR-0035)
Runs no longer ask for approval (see [What the agent may do](#what-the-agent-may-do)). A `gateway.toml` that still has
`[permissions]`, `[sandbox] park_after_seconds`, `policy` or `rules` in a `[[profiles]]` entry, or `class` in a
`[[connectors]]` or `[[mcp.servers]]` entry fails the check and stops start-up, naming the key to delete. Rebuild every
environment from this version's images: an environment from an earlier version cannot continue a run. The deployment
README's upgrade note lists what the first start changes.

### Decision engines need this version's runtime image (2026-09-28)
Tasks without a decision engine run on the previous runtime image as before. A task whose profile names an engine
(see [Decision engines](extensions.md#decision-engines)) needs this version's runtime image: on an older one its
sandbox is refused with the notice that the environment's runtime is too old, and the task does not run without the
engine. Rebuild custom environments from this version's runtime image before you name an engine in a profile.

### Channels need this version's runtime image (2026-09-28)
Channels need no setting, but their agent tools live in the runtime. A task that belongs to a channel and runs on an
older runtime image starts without them, and without an error. After the upgrade, point the installation at this
version's runtime image and rebuild custom environments from it; the deployment README's upgrade notes
(`deploy/README.md`) give the steps, including microVM installs.

### Secrets allow the routes and connectors that use them (2026-09-28)
From this version a route or connector may use a shared secret only once the secret allows it (see
[Who may use a shared key](web-settings.md#who-may-use-a-shared-key)). Existing routes and connectors keep the secrets they
already use: the upgrade allows each secret for exactly the routes and connectors naming it then, so nothing that
worked stops working and nothing needs doing. Secrets created afterwards are used only by the routes and connectors an
administrator allows, or a configuration file names.

### microVM installs need new root filesystems (2026-09-27)
On installs that run tasks in microVMs, every environment needs a new root filesystem built after the upgrade; until
then its tasks cannot start, and the Environments page shows it refused with the reason. The operator steps are in the
deployment README's upgrade notes (`deploy/README.md`).

### Tool calls record every optional argument (2026-09-27)
From this version the built-in tools declare every argument, with the optional ones allowed to be `null`, so that
endpoints which enforce tool schemas strictly accept them. A tool call is recorded with each argument the model left
out as `null`, which the tool reads exactly as before. Calls recorded by an earlier version keep their arguments as they
were, so a run that resumes after the upgrade continues with them.

