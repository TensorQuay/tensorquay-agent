# ADR-0015: Client transport and the outbound runtime link

## Status

Accepted

## Date

2026-09-22

## Context

Two hops carry the same typed frames: the runtime to the gateway, and the gateway to browsers and programs. They have
different shapes. A sandbox produces a dense stream of events and accepts a few actions. A browser wants history,
reconnection and ordinary authentication. A sandbox must also never be reachable from outside, because anything that
listens inside it is an entry point that the network rules would then have to defend.

Alternatives considered: a WebSocket on both hops; long polling for clients; gRPC streaming; a listener inside the
sandbox that the gateway calls.

## Decision

- Clients receive server-sent events and act with plain `POST` requests. Each event carries its sequence as the
  event id, so a browser reconnects with `Last-Event-ID` and the gateway replays from PostgreSQL. A client that opens
  a stream itself passes the same cursor as `after`, because a browser cannot set that header on a new connection.
- The runtime holds one outbound WebSocket to the gateway, dialled with its per-sandbox token. Frames go up and
  actions come down on that socket.
- Nothing listens inside a sandbox. The gateway never connects to it.

Server-sent events fit the client hop because it is plain HTTP: cookies, proxies, HTTP/2 multiplexing and any HTTP
client work without a second protocol. The runtime hop needs both directions on one connection, which is what a
WebSocket gives, and dialling outward keeps the sandbox unreachable.

## Consequences

- The gateway serves HTTP/2 so many streams share a connection, and reverse proxies must not buffer the stream.
- Actions are ordinary requests with an idempotency key rather than socket messages, so retries are safe.
- A sandbox needs no inbound network rules at all, which is what makes the internal-only network in ADR-0021 simple.
- Browser support for server-sent events is unremarkable; the only real constraint is the missing header on a new
  connection, handled by the cursor above.

## Links

- ARCHITECTURE §9, §10 and §11.1; ADR-0005; ADR-0021; ADR-0022; TEST-PLAN GW-I-007, WEB-U-002
