# ADR-0002: Rust gateway

## Status

Accepted

## Date

2026-09-22

## Context

The gateway owns authentication, event persistence, runtime links, model relay and sandbox
lifecycle management. It needs the same protocol definitions as the Rust harness. A second
implementation language would introduce another toolchain and protocol translation boundary.

## Decision

Implement the gateway in Rust with axum for HTTP and runtime links and sqlx for PostgreSQL.
Share wire types through `tqa-types`. Keep the gateway independent of the loop and runtime
implementation; communicate with sandboxed runtimes through the protocol.

## Consequences

The harness and gateway share a toolchain, types and validation gates. Maintainers need Rust
expertise across both. Shared language does not permit linking the agent loop into the gateway;
the dependency-direction gate preserves that boundary.

## Links

- [Architecture sections 3 and 10](../ARCHITECTURE.md)
- [Product decision 1](../PRD.md#12-decisions-for-the-po)
- [Development stack](../DEV-PLAN.md#2-software-stack)
