# tqa-runtime

The outbound-only process for one sandbox and one conversation. It owns the gateway link, private write-ahead
log, in-memory history and one core runner. It does not listen on a port or receive provider credentials.

The binary reads `TQA_GATEWAY_URL`, `TQA_RUNTIME_TOKEN_FILE`, `TQA_STATE_DIR` and optional `TQA_EGRESS_PROXY_URL`, with
a fixed `/workspace`
mount. The gateway setting is an HTTP(S) origin: no endpoint path, credentials, query or fragment, at most 2048
bytes. The runtime derives the link, history and validated relay paths on that origin. HTTP and WebSocket clients
connect directly without proxies or redirects. The token is read once from a regular file of at most 4096 bytes;
one trailing newline is stripped and the remainder must be nonempty printable ASCII. State and workspace mounts
must exist and must not overlap. Skills are read at each run start from the fixed read-only mount `/skills`
(`install` and `user` tiers; absent means none): the index follows the instruction files in the instruction slot,
the `skill` tool is registered only when a skill exists (ADR-0029), and the runner emits `context.loaded` after
`run.started` or `run.resumed`. Logs are JSON on stderr at info level and contain neither payloads nor credentials.

The optional proxy setting is a credential-free HTTP(S) origin of at most 2048 bytes on the gateway's
host. Ambient HTTP proxy variables are ignored. When present, the runtime registers the Network-class
`fetch_url` tool with the mounted credential and passes only the credential-free proxy origin to shell
children. When absent, `fetch_url` is unavailable. Gateway link, history and relay clients still connect
directly; the proxy is exclusively for permitted tool egress.

An optional `workspace.clone` in `session.config` initializes an empty workspace from a public HTTPS
repository. The runtime journals `workspace.clone` status before the first model run and keeps link
registration and replay active during the transfer. One pending user message waits for initialization.
Git receives a scrubbed environment, the explicit policy proxy, disabled credential helpers, verified
TLS and no redirects or submodules. A branch or tag may be selected with `ref`. Transfer and installation
share a two-minute deadline; retained diagnostics are capped at 16 KiB and never enter the transcript.

An existing workspace is preserved and reports `skipped`. A new checkout is staged on the workspace
filesystem and installed with exclusive creation, without replacing a pre-existing path. An interrupted
attempt is marked in private state and fails closed on restart. Failed initialization produces a plain
status and closes a pending run with the stable failure code without contacting the model. Completed,
skipped and failed initialization are remembered in the journal; none is automatically retried.

No run is offered `git_push` (ADR-0037 §7, RT-I-040): a task pushes with plain `git` and the person's key. A
started `git_push` call in a journal from an earlier release closes with an unknown-outcome result and is never
executed again; the result asks the user to check the remote.

| Exit | Meaning |
|---|---|
| 0 | Signal-driven shutdown, token revocation after registration, or durable parking |
| 1 | Storage, history or another runtime failure |
| 2 | Missing, invalid or unsupported process or session configuration |
| 3 | No accepted registration within sixty seconds |
| 4 | A reconnect changed a frozen session field |

Before initial registration, unauthorized responses are retried with full jitter, a 250 ms exponential base and a
5 second ceiling. After registration, an unauthorized response stops the process. Every connection begins with
`session.config`. Conversation, protocol, policy, limits, workspace and environment (including its digest)
are frozen for the process lifetime. The runtime builds one provider per `fallback_routes` snapshot, each on its own
relay path, so a run can switch routes without any credential in the sandbox; `session.update` replaces the route and
its fallbacks together. The link-only `session.update { route }` replaces the route snapshot for future
runs; an active run retains its own snapshot across all steps. The route in each authenticated `session.config`
also refreshes the snapshot for future runs, recovering updates missed while the link was down. Neither refresh
emits an event. Every other frozen field must still match on reconnect. The durable watermark can advance;
a future watermark beyond the runtime's journal is a fatal competing-writer error.
The link upgrade request carries the runtime's protocol revision in the `TQA-Protocol-Revision` header.
An unparseable configuration, typically from a gateway at a newer revision, receives one best-effort
unsequenced `unsupported_protocol` error and then a WebSocket close, with no redial, before exit 2. The
diagnostic names a bounded field identifier when available and the runtime's revision, without echoing
received values. Unknown limits
are rejected, so an older runtime cannot silently ignore a newer gateway's budget.

An owned blocking worker assigns sequence numbers, builds UTC timestamps, appends complete frames, notifies the
socket writer and supervisor, and then acknowledges the sink. It syncs at step completion, terminal run events,
and every hundred records. Typed frames are limited to
256 KiB; oversize frames fail the sink without truncation.
On every registration the worker truncates the acknowledged prefix and replays the remaining log before new frames.
A single writer owns the socket and preserves sequence order. Replay pages contain at most the configured queue
capacity (64 frames by default), including after registration; the socket never buffers the whole retained tail.
Its one-slot notification can coalesce: every skipped
frame remains in the log and is fetched before a later sequence. A link outage therefore never blocks the running
core. The supervisor's separate bounded queue supplies the in-memory history.

At startup only, authenticated history pages through the first watermark are joined with the log tail. Sequences
must be contiguous from one. The runtime compares the configured template and canonical capabilities with the last
`environment.changed` in that combined history, or Minimal with shell when absent. A change is journalled once and
consumed into history before core work starts. Digest-only updates and reconnects emit no change event.
The core then resumes an active run, answers interrupted calls, starts an unanswered
message, or becomes idle. Busy messages receive an unsequenced retryable error. Cancellation applies only to the
active run.
Only the supervisor generates thirty-second active-run heartbeats. A live socket cannot conceal a stalled supervisor.

The log cap is 64 MiB. A sink failure sends a best-effort unsequenced `io` error, syncs the existing log and exits 1
without writing a terminal event. The next process truncates the acknowledged prefix at registration, freeing the
space and leaving the run resumable. In-session acknowledgements and truncation are later work.
SIGTERM, SIGINT and credential revocation stop the core through its shutdown token, join cleanup, sync the log and
close the link within the container's ten-second stop timeout. They leave no terminal event.

Each run discovers root repository instructions on a blocking worker and assembles the versioned system prompt.
The stable prompt block names the template, the declared capabilities and the task's network allow-list from
`session.config` (PROMPT-U-009); a changed list applies from the next registration. File tools are always registered;
shell
requires the `shell` capability. Python, Node, build and browser capabilities currently add model facts only.
The builtin registry uses recommended tool limits, the session's tool-result cap, and an empty
environment allow-list. Spill files live under `.tqa/<conversation_id>/spill` in the workspace. The Chat, Responses and
Messages adapters use the gateway relay at the route's relay path (`…/chat/completions`, `…/responses` or
`…/messages`). Chat sends the canonical `reasoning_effort` field, Responses sends `reasoning.effort`, and Messages
sends the thinking budget the snapshot maps for the effort. A protocol this runtime cannot speak fails closed: an
unknown value is refused when the frame is parsed (the configuration gets one `unsupported_protocol` error and the
runtime exits 2; an update drops the link and is never adopted), and a future protocol variant without an adapter
produces a journalled failed run, with no relay traffic in any case.
All monotonic clocks, waits, wall timestamps, retry randomness and process cleanup are injected library dependencies.

Every call runs (ADR-0035): the runtime has no approval, grant or parking path. A history whose last run event is the
retired `run.parked` marker is failed with `approvals_removed` by the core, and the waiting call never runs.

| Runtime limit | Recommended value |
|---|---|
| Queue capacity | 64 entries |
| History page | 100 frames, 32 MiB encoded |
| In-memory history | 1 million frames, 256 MiB encoded |
| I/O deadline | 5 seconds |
| Initial registration | 60 seconds |
| Heartbeat interval | 30 seconds |
| Reconnect delay | 250 ms base, 5 second ceiling, full jitter |
| Shutdown cleanup | 8 seconds, with a final 1 second blocking-worker bound in the binary |
| Escaped-child cleanup | 1 second, at most 4096 inspected processes per pass |

The Linux binary enables child-subreaper ownership. A process-tool decorator and run-end cleanup inspect its own
children,
kill escaped processes and their groups, and reap each by its specific PID after the foreground tool has joined.
They never wait for an arbitrary child. Long-lived child services will require an explicit exclusion list when
introduced. Other supported development hosts use a no-op process reaper. The image runs under `tini` as PID 1,
uid/gid 1000, with `HOME=/tmp`; use the backend's read-only filesystem, dropped capabilities and resource limits.
The final image retains distribution copyright files for its separate command-line programs.

```sh
cargo fmt -p tqa-runtime -p tqa-testkit --check
cargo clippy -p tqa-runtime -p tqa-testkit --all-targets -- -D warnings
cargo nextest run -p tqa-runtime -p tqa-testkit
cargo llvm-cov nextest -p tqa-runtime --fail-under-lines 80

docker build -f images/runtime/Dockerfile -t tqa-runtime:dev .
cargo nextest run -p tqa-runtime --test docker_image --run-ignored all
```

On a rootless daemon run the Docker suites through the wrapper, joining the daemon's network namespace too:

```sh
TQA_ROOTLESS_NETNS=daemon tools/rootless-test.sh cargo nextest run -p tqa-runtime --test docker_image --run-ignored all
```

The fixture gives the runtime's state directory to uid 1000, which needs the daemon's user namespace. It also serves the
fake gateway from the test process, and a rootless daemon runs containers in a detached network namespace with
`--disable-host-loopback`, so their `host-gateway` address reaches a listener only inside that namespace. The flag is
off by default because suites that use the host's database on 127.0.0.1 must stay in the host's namespace.

Check available disk before coverage. The Docker suite fails loudly when selected without Docker or the image.
The non-default `testing` feature adds deterministic supervisor and append fault gates; only this crate's own dev
build enables it. The default runtime image contains no such seam. Deployment requires an immutable repository
digest and explicit `PATH` and `HOME` entries in the gateway's sandbox environment, matching the image policy.

