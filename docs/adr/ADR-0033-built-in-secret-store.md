# ADR-0033: The built-in secret store is the only secret store

## Status

Accepted (the Product Owner's decision, 2026-09-27)

## Date

2026-09-27

## Context

FR-28 and ARCHITECTURE §10 described a `SecretStore` with two backends: the built-in store (AEAD in PostgreSQL under
an install key held outside the database) and an external vault reached with a machine identity. A connect-only
backend for such a vault was built and then reviewed (see History).

The review found two things:
- The vault's server image is only partly under a permissive licence. Its core runs code under an enterprise licence
  that requires a subscription for production use.
- No v1 requirement depends on anything that enterprise code offers: rotation, audit, access control, dynamic
  secrets, approvals, point-in-time recovery, HA, KMS/HSM, rate limits and SSO.

FR-28's behaviour is met in full by the built-in store:
- encrypted at rest under an install key outside the database;
- write-only from the UI and API;
- the gateway is the only reader;
- rotation without restart;
- failing closed.

The Product Owner decided (2026-09-27) that the product's secret store is our own, that anything it needs is built
into it, and that nothing is borrowed, bundled or connected.

## Decision

1. **One secret store: the built-in store.** Values are encrypted with AEAD (XChaCha20-Poly1305) in PostgreSQL under
   an install key held in a private file outside the database, and authenticated to their reference. The gateway is
   the only reader; values are write-only from the UI and API; nothing reaches a sandbox.
2. **No other backends.** There is no `[secret_store] backend` option and no external vault settings. The
   `SecretStore` trait stays as the internal boundary (it keeps the store testable and its consumers unaware of
   storage), not as an extension point.
3. **What the store may need later is built into it**, never borrowed:
   - read audit (which run read which reference; about 1 day);
   - wrapping the install key with a KMS or HSM (about 2–3 days plus a dependency review under ADR-0017).
   Each needs its own Product Owner decision.
4. **Records superseded:**
   - the external-vault parts of FR-28, NFR-1, the PRD's §12 decision 7 and its product-surface and deployment lines;
   - ARCHITECTURE §10 (secret store), §12 (the threat row) and §14 (key settings);
   - DEV-PLAN Phase 6 item 1 and the stack table;
   - TEST-PLAN GW-I-029 (restated for the built-in store), GW-U-051 and SEC-029 (void; IDs are never reused).

## Consequences

- One store to secure, test and operate. Nothing extra to install, back up or license. Backups are PostgreSQL's
  plus the install key.
- Operators who keep secrets in an external vault copy the values in through the admin page or the seed file; there
  is no live link.
- FR-28 is met by the built-in store alone; Gate 6's GW-I-029 row is back to the built-in store and passes on it.
- **Upgrade:** an install that set a `backend` key or an external vault section under `[secret_store]` must remove
  them. The gateway refuses unknown settings at start rather than running without values.

## Amendment: a read's run is the task's own (30 Sep 2026)

A key read records the run it was for only when that run belongs to the reading task's conversation. The runtime sends
`X-TQA-Run-ID` on relayed model calls, and a decision request names its run. The gateway keeps the id only when that
task's `run.started` or `run.resumed` is stored, waiting at most 1 s, and only when it isn't stored yet. Otherwise the
run is recorded as none, and `tqa_relay_unattributed_total{route,reason}` counts it (`missing` or `foreign`). The id
never authorizes anything; a malformed header is still refused. A Git push's key reads name the run of the journaled
call that its setup verified. The MCP proxy takes no call context (ADR-0035), so connector reads name no run. Requiring
the header, once every supported runtime sends it, is the Product Owner's decision.

## History

The external vault was Infisical. A connect-only `infisical` backend (our own HTTP client, no Infisical code or image)
was on the development branch from 2026-09-27 and is removed by
this decision. A second slice was abandoned before merge. The licence review, the feature map and the design note
are in the team's working notes, not in the repository.

## Links

- PRD FR-28, NFR-1, §12 decision 7; ARCHITECTURE §10, §12, §14, §18; TEST-PLAN GW-I-151
- ADR-0017 (licence policy), ADR-0028 (runtime-mutable settings and the secrets seed)
- TEST-PLAN GW-I-029, GW-I-054, GW-U-051 (void), SEC-029 (void)
