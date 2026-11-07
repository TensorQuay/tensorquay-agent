# ADR-0019: The relay is a transparent authenticated proxy

## Status

Accepted

## Date

2026-09-22

## Context

Model calls leave a sandbox only through the gateway. An earlier draft had the runtime always speak Chat Completions
and the gateway translate to other protocols. Translation loses features that must round-trip between turns:
thinking blocks with signatures, Responses reasoning items, cache-control markers. It also puts a second copy of every
protocol's semantics in the gateway, and it would place vendor settings inside the sandbox.

## Decision

The runtime speaks the route's own protocol to `/relay/r/{route}/...` with its runtime token and sends standard
requests without vendor settings. The relay checks the route and the caller, allows only that protocol's completion
endpoints, pins `model`, merges the route's `extra_body` and `extra_headers`, maps the standard reasoning field to the
route's field, injects the credential, forwards to the upstream endpoint and streams the answer back unchanged,
reading usage from the stream for metering. No protocol translation exists anywhere.

## Consequences

Each protocol is implemented once, in `tqa-llm`, and used by the runtime; the relay needs only route shaping and a
usage reader per protocol. Vendor settings and credentials never reach a sandbox. A new protocol needs an adapter and
a usage reader, nothing in the loop.

## Amendment: the gateway names its own refusals (30 Sep 2026)

A forwarded answer is the model server's, unchanged, so the runtime could not tell the relay's own refusal (a key whose
use couldn't be recorded, a spent budget, a rate) from a provider's error, and showed both as "the provider said".
Every refusal the gateway makes itself on the model relay (`/relay/r`), the engine relay (`/relay/d`) and the MCP
proxy (`/mcp`) carries `X-TQA-Gateway-Error: <code>`, equal to the body's `error.code`. Only the gateway sets it: it
is removed from every other answer on those paths, so a model server, an engine or a connector can't pass off its
error as the gateway's. The runtime records such a failure as `gateway.<code> <status>`, with no provider message.

## Links

- ARCHITECTURE §2, §6 and §10; TEST-PLAN GW-I-039, GW-I-148, GW-U-090
