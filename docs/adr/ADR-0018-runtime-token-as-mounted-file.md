# ADR-0018: Runtime token delivered as a mounted file

## Status

Accepted

## Date

2026-09-22

## Context

Each sandbox needs one credential, the per-run token it uses to dial the gateway. Passing it as an environment
variable places it in the container-create request body, in `docker inspect` output and in the debug logs of the
Docker client library, which logs request bodies at debug level. Our own code redacts, but a third-party library's
logging is outside our control, and an operator enabling debug logging must never expose a credential.

## Decision

The sandbox backend writes the token to a per-sandbox file under `storage.root` with mode 0600 in a 0700 directory,
bind-mounts it read-only at `/run/tqa/token`, and sets `TQA_RUNTIME_TOKEN_FILE=/run/tqa/token`. No secret value is
ever placed in a create request, a label or an environment variable. The file is removed when the sandbox is
destroyed. The runtime reads the token from the named file. As defence in depth, release builds cap the `log`
facade at info so no `log`-based crate can emit debug or trace output in production; the gateway's subscriber
additionally pins the Docker client's logging to warn.

## Consequences

Inspect output and client logs are safe to share. Firecracker uses the same contract through a small read-only
block device or an equivalent injection. Tests assert the token is absent from every surface except the mounted file
(SBX-C-005) and that the built create body's `Debug` output never contains it.

## Links

- ARCHITECTURE §4.2 and §12; DEV-GUIDELINES §1.2 rule 7; TEST-PLAN SBX-C-005
