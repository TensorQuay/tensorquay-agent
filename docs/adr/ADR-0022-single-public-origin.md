# ADR-0022: The gateway is the single public origin

## Status

Accepted

## Date

2026-09-23

## Context

The browser needs the web pages, the HTTP API and the event streams on one origin. Session cookies, the exact
`Origin` check on state changes and `EventSource` all assume it. Three topologies were considered:

- The Next.js server in front, forwarding the API to the gateway. The largest process becomes the internet-facing
  one, and authentication would sit behind a Node server's proxy. Event streams would pass through a proxy that may
  buffer them. The gateway would also see one client address for every user, so per-address login throttling would
  need trusted forwarding headers.
- A third reverse proxy in front of both. This adds a component to every installation.
- The gateway in front, forwarding page requests to an internal web server.

## Decision

The gateway is the only public listener. It serves the API, the event streams, the health endpoints, the runtime link
and the relay itself. It forwards every other `GET` and `HEAD` request to the web server configured in `[web]`. The web
server listens on an internal address and renders only the application shell. The browser fetches all data from the
same-origin API.

Forwarded requests never carry credentials. The gateway removes `Cookie` and `Authorization` before forwarding, and
drops `Set-Cookie` from the reply. Other methods on page paths are refused. A development flag also forwards the
Next.js hot-reload WebSocket; it is off by default.

## Consequences

- Authentication, origin checks, throttling and streaming stay in one audited process.
- The web server holds no secrets and can be replaced by static files later without changing the browser contract.
- An installation has one address to publish. TLS terminates at the gateway or at the operator's own proxy.
- Next.js server-side data fetching is not used. A server-rendered first page would need a credentialed server-side
  call and is left for later if measurements justify it.

## Links

- ARCHITECTURE §2, §10 and §11; ADR-0015; TEST-PLAN WEB and GW rows
