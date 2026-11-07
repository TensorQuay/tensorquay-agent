# Web page server

The image runs the Next.js standalone build as UID/GID 1000 on port 3000. Build it from the repository root:

```sh
docker build -f images/web/Dockerfile -t tqa-web:local .
```

The official Node.js base is pinned to its multi-platform digest. The builder installs the frozen pnpm lockfile;
the final image contains only traced runtime dependencies, static assets and the self-hosted brand fonts.
Image optimisation is disabled, and the optional native image dependency is excluded by the package policy.

Deploy behind the gateway on its internal web network. Do not publish this server or give it session cookies,
provider keys, database credentials or the Docker socket. The gateway forwards pages and static assets while the
browser sends API and event-stream requests to that same public gateway origin (ADR-0022).
