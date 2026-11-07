# ADR-0003: PostgreSQL as the only database

## Status

Accepted

## Date

2026-09-22

## Context

The gateway persists accounts, conversations, events, usage and shared control-plane state.
Supporting a second engine for single-user installs would multiply migration paths and tests.
Event replay requires durable ordering and uniqueness per conversation.

## Decision

Use PostgreSQL for every deployment, including single-box installs. Access it through sqlx
with compile-time checked queries and forward-only migrations. Keep events append-only with
a unique `(conversation_id, seq)` key. Workspace files and large blobs use the host's local disk.

## Consequences

There is one database engine, query model and migration path to maintain. Every install needs
PostgreSQL, and operators must back up both database state and local file storage. Database
corrections proceed through new migrations rather than reversing applied migrations.

## Links

- [Architecture sections 4.5 and 10](../ARCHITECTURE.md)
- [Product requirements FR-16 and FR-29](../PRD.md#81-functional)
- [Product decision 2](../PRD.md#12-decisions-for-the-po)
- [Development stack](../DEV-PLAN.md#2-software-stack)
