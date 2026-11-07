# tqa-tools

The tool boundary for the walking skeleton. It depends only on `tqa-types` within the workspace. Tools run inside
the surrounding sandbox; they neither create that sandbox nor carry provider credentials.

`Tool` exposes immutable `ToolSpec` metadata and an asynchronous `call`. Builtin `ToolName` values match
`[a-z][a-z0-9_]{0,63}`; the future `mcp.` namespace is reserved. `ToolRegistry` compiles object-shaped JSON Schemas
at registration, rejects duplicate names, and validates arguments before dispatch. Invalid arguments include a JSON
pointer without copying their values into diagnostics. External schema retrieval is disabled. A `ToolOutput` holds
text, an optional process exit code, search progress when applicable, and an empty artifact vector. `Artifact` has no
variants in this skeleton.

Registration uses the shared catalog limits: 256 tools, 64 KiB of encoded parameters per tool, and 1 MiB of
encoded model-facing definitions and metadata, including the array and function wrappers. It checks borrowed
inputs before schema cloning or compilation. `RegistryError::CatalogLimit` identifies only the exceeded bound;
failed registrations consume no catalog capacity. Exact limits remain valid.

The loop renders tool failures with `ToolError::model_message()`, which includes the stable code, argument path
and actionable reason within 500 characters. Error details describe constraints and locations, never file contents.
Logs use `Display`, which omits those details.

## Workspace paths

`WorkspaceRoot::open` accepts an absolute existing directory and canonicalizes it. `resolve` rejects empty paths,
NUL bytes, more than 4096 bytes, and more than 64 components. Absolute paths must begin under the canonical root.
Each existing prefix is canonicalized before processing the next component, including parent traversal; new
suffixes are permitted. Symlink targets and every resolved prefix must stay under the root. Dangling symlinks fail
closed. Writes reject `.git` components both before and after canonicalization.

These checks are path validation, not a kernel filesystem capability: a concurrent actor replacing directory
entries between validation and I/O requires the surrounding sandbox boundary. Shell commands can access paths
outside the workspace inside that sandbox; only their configured working directory is workspace-confined.

## Builtins

| Tool | Behavior | Parallel safe |
|---|---|---|
| `read_file` | Line-numbered pages, inclusive line ranges, stateless cursors for long lines, binary summary | yes |
| `write_file` | Optional parents, exclusive staging file in the destination directory, sync and atomic rename, three-line replacement summary | no |
| `edit_file` | Exact ordered replacements, overlapping uniqueness checks, preserved mode and line endings, bounded diff | no |
| `glob` | Sorted matching paths, ignore-aware parallel traversal, page cursors and scan budgets | yes |
| `grep` | Regex or literal matching, file discovery, matching-line counts or line-numbered content with context | yes |
| `shell` | `sh -c`, workspace cwd, bounded output, deadline and process-group cancellation | no |
| `fetch_url` | GET through an explicitly configured authenticated policy proxy, bounded text and HTML extraction, returned in an untrusted-content block | no |

`read_file` accepts `path`, optional one-based `start_line` and inclusive `end_line`, or `cursor` without line
arguments. A default page returns up to 200 lines and at most 8 KiB, including six-column line numbers, tabs and
continuation notices. A larger requested range is trimmed to the page. Follow the final notice literally with
`start_line` or the opaque cursor. Line numbers are presentation only; never include them in `edit_file` arguments.
Prefer grep to locate a range before reading it. A start beyond EOF reports the known line count and requested start; an empty file reports zero lines.

The reader scans newlines with `memchr` without decoding skipped text. It holds one page and an 8 KiB read buffer,
and accepts source files up to `MAX_READ_FILE_BYTES` (64 MiB), independently of the edit size cap. Long-line cursors
are versioned base64url records containing a workspace-relative path, byte position, size and modification time.
Changed files reject their old cursors. The reader sizes a long line without retaining it; it does not scan the whole
file merely to count lines. The continuation says `unknown` until the total is known. NUL in the first 8 KiB or invalid
UTF-8 in the selected page produces a one-line binary summary. Partial UTF-8 characters continue on the next page.

Write summaries count complete removed and added file lines. A failed staged write leaves the destination unchanged
and removes its staging file. Newly created parent directories may remain after failure. An edit validates all
replacements before writing once. Uniqueness includes overlapping occurrences; `replace_all` replaces non-overlapping
matches from left to right. Misses report whitespace-only near matches, and ambiguous edits report line numbers.
An edit checks the original length and modification time immediately before rename and refuses a concurrent change.
Rename-based replacement breaks hard links: other names for the original file keep the old bytes.

Both search tools use `ignore` with four traversal workers, respecting nested `.gitignore` files and negations,
`.git/info/exclude`, `.ignore`, and `.tqaignore`. Ignore syntax follows Git, including literal unmatched `[` characters.
Ignore files are individually size-bounded and validated before loading; symlinked ignore files and external Git
metadata are refused. Global host ignore configuration and rules above the workspace are excluded. Hidden components
must be named explicitly in the path or glob; a content regex does not enable hidden paths. Symlinks are never followed,
including in an explicitly requested search path. Paths must be bounded UTF-8 without control characters.

`grep` uses `grep-regex`, `grep-matcher` and `grep-searcher`. Its default `files_with_matches` mode gives the cheapest
discovery result. Narrow the path and request `content` to obtain `path:line:text`; optional context uses
`path-line-text` and `--` between separated groups. Rows are capped at 500 bytes with `[truncated]`, or fewer bytes for
smaller pages. `count` counts matching lines, including a line with multiple matches only once. Binary files are
skipped and counted in a footer; each file finishes binary detection before its results are returned. Query compilation
is capped at 64 KiB of pattern, 10 MiB of compiled regex and 1 MiB of DFA cache. The streaming search buffer is bounded
by the scan-byte ceiling plus one byte so reaching the budget cannot masquerade as EOF.

Search workers own separate streaming searchers. Files larger than 8 MiB share one permit per search, held until
the enlarged buffer is released, so large single-line inputs cannot multiply that buffer by the worker count.
The opened file's metadata length bounds each read; a growing small file cannot bypass the permit. Waiting for
the permit checks cancellation and elapsed limits every 10 ms. A window of at most 256 in-flight or completed file results
bounds look-ahead, and the caller merges them in sorted path order. Each result retains at most the cursor skip
plus one page of content; counts and file discovery retain one aggregate per file. Every worker checks cancellation
between files and during reads, and all workers are joined before the call returns.

Search pages are sorted by relative path and fit 8 KiB including notices. `max_results` caps rows per page; the
configured result limits are ceilings. Search cursors contain a version, a SHA-256 digest of normalized options and a
result position. Repeat the same options; a mismatch or unavailable position requires a new search. Pagination
rescans the current tree, so edits between pages require searching again. No index or server-side cursor state is kept.

`ToolOutput.search` supplies `complete`, `scanned_files`, `scanned_bytes`, `reason`, and `next_cursor`. Complete means
exhausted with neither a budget stop nor another page. Grep counts examined file bytes; glob accounts matching files'
sizes from metadata. Ignored subtrees are not traversed. A file, byte, elapsed-time or directory-entry ceiling returns
bounded results with an incomplete notice and a reason; directory-entry exhaustion uses `Files`. Narrow the query
after a budget stop. Grep returns a prefix of fully classified files: a file cut by the byte or time budget
contributes no rows. Finished speculative results are included only when their actual read bytes fit the remaining
in-order allowance; no file is scanned again within a call. The one-worker path follows the same rule. Reported
counts cover the ordered prefix, including the examined boundary file, and exclude speculative later files. A stopped
traversal returns no grep rows because its collected paths cannot certify a prefix. Cursors are issued only for a
further page without a budget stop.

## Explicit limits and cancellation

Construct `ToolContext` with a workspace, `CancellationToken`, `Clock`, `Sleeper`, environment allow-list and
`ToolLimits`. No implicit context is created. `ToolLimits::RECOMMENDED` provides:

| Field | Value |
|---|---|
| `shell_timeout`, `file_timeout` | 30 seconds each |
| `shell_output_bytes`, `file_read_bytes` | 1 MiB each |
| `read_page_lines` | 200 |
| `read_page_bytes`, `search_page_bytes` | 8 KiB each, configurable from 512 through 8192 bytes |
| `search_threads` | 0: available parallelism capped at 8; explicit values 1-64 |
| `search_max_files` | 200000 |
| `search_max_bytes` | 512 MiB |
| `search_max_elapsed` | 5 seconds |
| `glob_max_elapsed` | 2 seconds (glob only lists paths; TOOL-E-016) |
| `glob_results`, `grep_results` | 1000 each |
| `glob_entries` | 250000 |
| `ignore_file_bytes` | 64 KiB per ignore file |

Zero limits, invalid page sizes and unrepresentable deadlines are rejected. Argument limits can reduce these ceilings.
A path too long to fit its page or cursor produces a typed size error. Filesystem jobs run on blocking workers, check cancellation between bounded
chunks, and retain their worker until cleanup completes. A running operating-system filesystem syscall cannot be
forcibly interrupted; cancellation and timeout wait for it to return before joining the worker.

Shell children start with a fresh environment containing inherited `PATH`, `LANG` and `TERM` when present, fixed
`HOME=/workspace/.tqa/home`, and explicitly allowed parent variable names. The workspace-relative home is created
on first use with mode 0700; symlinks are refused. Pip and npm use their normal cache locations beneath this private
home, keeping caches and dotfiles out of the project root. An allow-list entry cannot override HOME. The shell
itself may add standard variables such as PWD. Standard input is closed. Process groups use the safe
`Command::process_group(0)` API; cancellation and timeout send TERM to the group, then KILL after a two-second grace
period when the group has not exited. The child is reaped. Cleanup returns only once no member of the group exists:
after KILL it waits up to one more second and otherwise fails. Members that outlived the leader and were reparented
to this process, as under the runtime's child subreaper, are reaped by group; the leader is only ever reaped through
its own handle. When the leader returns, exited members are reaped the same way before the group is checked for
processes still running, so they never trigger the background notice. Dropping a call also kills its group; the
process runtime owns eventual child reaping.

Each output stream is decoded incrementally and keeps its own byte order. Streams merge **as observed** when pipes
are read; cross-stream write order is not guaranteed. Each stderr line is labelled `[stderr]`. Shell text retains a
head and tail, marks truncation and lossy UTF-8, and includes markers within the output byte cap. Very small caps
can only hold a prefix of a marker. Escaped process groups are a sandbox lifecycle concern.
A command that uses `setsid` to create a new session escapes process-group cleanup and can live until its sandbox
is destroyed. Linux runtime child-subreaper cleanup belongs to the runtime lifecycle contract.

## Every call runs (ADR-0035)

There is no permission policy, rule, class or grant: the task's tools set what the agent can do, and a spec says only
whether separate calls may run concurrently (tools that read). The file tools' paths mean the workspace, `.git`
included; that is a path contract, not a permission. Every git the tools start (the shell's, the clone, the push)
gets environment configuration that switches repository hooks and the file-system monitor off
(`GIT_SAFETY_ENV`), so a shared workspace's `.git` can't run code in another member's task (SEC-045, TOOL-U-039).

`PreparedAction` owns validated tool inputs and execution context without a cloning or mutation API.
The registry must validate the schema before construction. Object keys are sorted recursively by their
UTF-8 bytes, arrays retain their order, and compact JSON encodes the tool, arguments and context envelope.
SHA-256 covers that exact envelope. Context contains conversation, run, positive step and attempt, and
call identifiers. Nesting is capped at 128 and the encoded envelope fits the protocol frame byte limit.
Execution consumes the prepared value and receives its original tool and canonical arguments.
`call_prepared` also retains its hash and context for tools that bind an external operation to the call.
Canonical envelope encoding lives in `tqa-types`; existing digests are unchanged.

MCP adapters and background process handles remain separate contracts. Local tool resource ceilings apply independently
of the core result cap.

## Proxy fetch

`FetchUrl` takes a validated `ProxyEndpoint` and the runtime credential read from its mounted file.
It never reads proxy or authentication environment variables. Requests accept only `url`: an HTTP(S)
URL without userinfo, at most 4096 bytes. Every request and redirect uses CONNECT to the configured
proxy, including plain HTTP destinations. The sensitive bearer header is sent only on CONNECT;
origin requests carry no authorization or cookies. Proxy 403 is `egress_denied` with host and port,
407 is `proxy_authentication`, and an origin 403 remains an HTTP I/O failure.

The raw body is capped at 2 MiB, the extracted UTF-8 text at 1 MiB, response headers at 16 KiB,
and redirects at five. The total call deadline is at most thirty seconds, including conversion.
Requests advertise identity encoding; compressed responses and binary content types are refused.
HTML extraction uses the MIT-licensed `html2text` dependency on an owned blocking worker.
Before rendering, documents are bounded to 65536 DOM nodes, depth 128, and table spans of 1000.
Transport futures stay inside the cancellable call; no background connection task outlives it.
Both proxy TLS and destination TLS verify certificates against the built-in public roots.
The native TLS fixtures use `openssl` to generate short-lived certificates in disposable directories;
no private key or certificate fixture is stored in the repository.

`Shell::with_proxy` supplies credential-free uppercase and lowercase HTTP proxy settings explicitly,
plus `NO_PROXY` and `no_proxy` for the gateway host and loopback (`localhost`, `127.0.0.1`, `::1`; TOOL-U-060: a
command reaches a server it started on loopback). The existing environment scrub still applies;
the runtime token is never added to child environments. Network isolation remains the surrounding
sandbox's responsibility. The runtime registers `fetch_url` only when its explicit proxy input exists.

## Attached images
`probe_image` and `read_image` accept only an upload path `uploads/<name>` (the shared attachment rule). Neither
`uploads` nor the file may be a symlink, the file must be regular, and the opened file must be the one that was
checked; anything else is `Unreadable`, and a fifo or directory is never opened. The format comes from the first
bytes only (PNG, JPEG, GIF, WebP), never the name; other files are `NotImage`. `read_image` reads at most
`MAX_IMAGE_BYTES` (5 MiB) and reports `TooLarge` above it; `probe_image` reads only the header. Errors carry no
path or content.

## Run notes
`WriteNotes` and `ReadNotes` (`write_notes {text}`, `read_notes {}`) keep one notes file per conversation at
`.tqa/<conversation_id>/notes.md` (ADR-0024). Writes replace the file atomically, create its directory, stay within
the workspace through the same write resolution as `write_file` (a symlinked directory is refused) and accept at
most `MAX_NOTES_BYTES` (16 KiB); `read_notes` is parallel-safe and `write_notes` is not, and reading returns
`No notes yet.` before the first write. The file survives new sandboxes because it lives in the workspace; the
model context takes the latest notes from the journal, not from this file.

## Branch publishing
The `git_push` tool and its Git relay client are removed (ADR-0037 §7, the second release): a task pushes with plain
`git` and the person's key for the git host, which the egress proxy swaps in. A started `git_push` call in a past
task's journal still closes with its unknown-outcome result (`git_push_outcome_unknown`).

## Repository initialization

`RepositoryCloner` is the injectable transport used by runtime workspace initialization.
`GitCloner` spawns Git directly with a validated public HTTPS URL, mandatory policy proxy and a
scrubbed environment. It disables prompts, credentials, hooks, alternate protocols, redirects and
submodule recursion while preserving TLS verification. A caller may explicitly inject a trusted
CA bundle. Process execution is capped at 120 seconds; diagnostics are capped at 16 KiB and never
returned as status text. Cancellation and every exit path stop and reap the owned process group.
The runtime owns staging, no-overwrite installation and durable progress.

## Validation

Run `cargo nextest run -p tqa-tools -p tqa-testkit` and
`cargo llvm-cov nextest -p tqa-tools --fail-under-lines 90`. Tests reference TOOL identifiers in the test plan and
cover path escapes, atomic-write failure, registry rejection before dispatch and real process
group cleanup, cursor properties, large files, ignore rules and scan budgets. `tqa-testkit` supplies `FakeTool`,
injected time and disposable workspace fixtures. Run the ignored PERF-050 test explicitly with
`cargo nextest run --release -p tqa-tools --test performance --run-ignored only --no-capture -E 'test(perf_050_warm_checkout_literal_regex_and_glob)'`. It generates
100000 searchable files and 50000 ignored build files, measures warm-cache literal and regex grep with one worker
and automatic workers, and measures glob. The one-worker comparator uses a separate 30-second elapsed cap and must
complete; its other scan budgets stay at their recommended values. Automatic grep and glob must complete under
the unchanged recommended limits. Automatic grep must stay within 1.5 times ripgrep's median on the same tree when
ripgrep is available. Every measured grep and glob result must contain exactly the expected path. The output includes
sample timings, medians, elapsed caps and host load averages.

The ignored `perf_050_large_line_memory_one_worker` and `perf_050_large_line_memory_eight_workers` cases each scan
eight 48 MiB single-line files under recommended byte and elapsed limits. Run each case in its own release process
with a peak resident-memory recorder to compare the single-worker and eight-worker memory use.

## Untrusted content

`untrusted::wrap(source, attributes, body)` puts content from outside the conversation (a web page, a channel message)
in one delimited block the system prompt tells the model is data, never instructions (ADR-0014 point 10). The opening
tag holds the source and the attributes in the order given, with escaped values; the closing tag repeats an id derived
from the source, attributes and body, so the body can't end the block and the same inputs give the same block. Source
and attribute names are `[a-z_]{1,32}`. `fetch_url` uses it now; the channel tools will too (TOOL-U-035, SEC-034).
