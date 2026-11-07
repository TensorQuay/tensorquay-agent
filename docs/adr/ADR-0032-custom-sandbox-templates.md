# ADR-0032: Custom sandbox templates: registration by digest, background checks, and one rootfs key

## Status

Accepted

## Date

2026-09-27

## Context

DEV-PLAN Phase 6 adds custom sandbox templates, "derived from Workstation or Minimal and registered by digest". ARCHITECTURE §4.7
defines the `templates` table and the binding rules. §15.1 states the template contract: derived from the base image, no secrets, no
privileged settings, and the runtime binary intact.

Templates were start-up configuration (`[[templates]]`) outside ADR-0028. Nothing checked derivation or the runtime binary, so any
pinned image was accepted and failed late, if at all. On the microVM backend the rootfs is built by an operator script, and a template
pinned by registry digest could not start there (SBX-F-022): rootfs builds are keyed by image ID, while the pin named the registry
manifest.

## Decision

1. **Templates are runtime-mutable settings (ADR-0028).**
   - PostgreSQL holds them; `[[templates]]` seeds them with keyed seed digests, tombstones and `last_set_by='seed'`. They join the
     settings snapshot, and every write locks, bumps the generation and refreshes it. Names are immutable keys.
   - The admin API (GW-I-078 to GW-I-080), for administrators with a session or an API token:
     - `GET /api/v1/admin/templates`;
     - `PUT …/{name}`: create without a version, replace with the version read;
     - `DELETE …/{name}?version=`;
     - `POST …/{name}/default` with `{version}`, which moves the default atomically;
     - `POST …/{name}/check`, which queues the template's checks again (`202`).
     Writes take an `Idempotency-Key` (except `check`, which is naturally repeatable) and are audited in `admin_audit` with versions
     only, never values.
   - A write runs the same validator as the file (GW-U-041, GW-I-080), so the same fields fail with the same reasons.
   - The user API (`GET /api/v1/templates`) lists the offered templates only, and never shows an image or digest.
2. **Registration by digest; the gateway never pulls.** An administrator builds or pulls the image on the engine host and registers its
   pin: a local image ID or a registry digest, never a tag (`image_not_pinned`). The control plane holds no registry credentials and
   makes no registry traffic.
3. **Checks run in the background, never at write time, and never run image code** (amended 2026-09-27: credential files).
   - One checker task checks each template version once, against the file-provided templates as bases (GW-I-077). Engine trouble
     leaves a template `unchecked` and retries with a backoff from 30 s to 10 minutes. The checker never delays start-up or readiness.
   - The checks, in order, stop at the first failure with a closed reason code:
     1. present, with the pin matching (`image_not_found`);
     2. **derived**: the image's layers begin with all layers of a base, recorded as `base` (`not_derived`);
     3. protocol label at or above the floor, ADR-0026 (`runtime_label`, `runtime_too_old`);
     4. the image policy the backends apply at create (SBX-C-014): no declared volumes (`image_volumes`); no environment key the
        sandbox leaves unset (`image_env_unset`, reported by key name only); a numeric non-root user (`image_user`); the base's
        entrypoint (`image_entrypoint`);
     5. **runtime binary intact**: its SHA-256 equals the base's, read from a container that is created and never started
        (`runtime_missing`, `runtime_modified`, `runtime_unreadable`; SBX-U-008);
     6. **no credential files in the template's own layers** (the layers beyond its base's; SBX-U-010/011, SBX-C-014):
        `image_secret_file`, and `image_scan_limit` past a bound (below);
     7. on microVM installs, the rootfs for this image ID (`rootfs_missing`, `rootfs_outdated`, `rootfs_mismatch`, `rootfs_foreign`,
        `rootfs_ambiguous`; SBX-F-023).
   - **Credential files (check 6).** The engine's export of the image (`docker save`) is read once, without running anything;
     each gzip or tar layer blob is hashed uncompressed and scanned, and only blobs whose hash is one of the template's own layer
     IDs count, applied in the image's order (whiteouts and replacements included). A regular file is a hit when it has one of
     these shapes, named by a closed rule: a PEM private-key header in its first 4 KiB (`private_key`); `.git-credentials` with
     content (`git_credentials`); `.netrc` with a password (`netrc`); `.aws/credentials` with a secret key (`aws_credentials`);
     `.docker/config.json` with a stored registry credential (`docker_auth`); `.npmrc` with a token (`npmrc_token`); `.pypirc`
     with a password (`pypirc_password`); an `id_*` SSH key file (`ssh_key`). A named file's rule reads at most its first 64 KiB.
     - A hit the install file does not acknowledge refuses the template (`image_secret_file`). A template entry in the install
       file may list `acknowledged_secret_files` (absolute paths, at most 16; file-only like a connector's CA file, so the API
       refuses it with `400 file_only`): an acknowledged hit verifies it as a warning, logged once at WARN, so a real image (a dev
       server's test key) is never locked out and the choice is explicit.
     - The scan is bounded by `[template_checks]` `max_scan_entries` and `max_scan_bytes` (160,000 entries and 6 GiB by default,
       four times the Workstation template's own layers or more; 4.3 s on it) and by its own 60 s. Past a bound the template is
       refused with `image_scan_limit`, never counted clean; raising the setting is the explicit way to admit a larger one. A
       zstd layer, or an export the engine cuts short, leaves the template `unchecked` and retried.
     - The check stores and shows paths and rules only, never content: `check.secret_files` (every hit, the acknowledged ones
       marked, at most 16 and a count of the rest) and `check.scan_limit` (the bound and its value).
     - **The scan cannot prove absence.** A secret in any other shape, a key past a file's first 4 KiB, or one in a base layer
       passes; the scan catches the common, accidental cases, and what an image holds stays the administrator's responsibility.
     - **A deleted file is still in the image.** The scan follows whiteouts, as the sandbox sees the filesystem, so a credential
       file a later layer deletes is not a hit. Its bytes remain in the earlier layer, and anyone with the image can extract
       them. Secrets are kept out of the build (a build secret mount, or a multi-stage build that never copies them), never
       removed with a later `rm`.
   - Each result is stored for the version it checked; a result for a version the template has since left is discarded. Every state
     change is logged once, at INFO, and a refusal at WARN; a refused **default** is logged at ERROR (GW-I-077).
   - Check containers carry the check label and the gateway's instance label, so each gateway removes its own leftovers at start-up
     (SBX-D-009).
   - Create keeps its own image checks, because the engine can change after a check.
4. **Offered, verified, refused.**
   - A template is **offered** when it is enabled and either verified, or unchecked and provided by the file (`last_set_by='seed'`),
     so an engine that is briefly unreachable at start hides nothing the operator configured. A refused template is never offered.
   - A write resets the check only when a checked input changes: the image, capabilities, CPUs or memory. A rename or an `enabled`
     toggle keeps it.
   - Creating a conversation with, or switching to, a template that exists but is not offered is `409 template_unavailable`; an unknown
     name is `422 unknown_template`; no name means the default.
   - The readiness detail reports the default template, whether it is offered and why not, and the checks pending. It is a diagnostic
     and never makes the gateway unready.
5. **Exactly one default, and it stays offered.**
   - A second default, or none, is `409 default_count`. The default moves only through `POST …/{name}/default`, and only to a template
     offered now; otherwise `409 template_unavailable`.
   - A write that would leave the default unoffered is `409 template_is_default` ("make another template the default first"):
     disabling it, changing one of its checked inputs, or any write while it is not verified (a file-provided default is offered
     while unchecked, but an administrator's write is never the file's, so the write would leave it unoffered).
   - A later check can still refuse the default. Creates without a template name are then refused with `409` rather than silently
     falling back, the refusal is logged at ERROR once per refused version, and readiness names it.
6. **Deleting.** A template in use is `409 template_in_use` with what uses it: the default, the number of active conversations bound to
   it, and the `[[mcp.servers]]` entries that list it. A deleted file-provided template stays deleted until its seed entry changes.
7. **A template's new image reaches its idle conversations** (amended 2026-09-27). A conversation records the template's image
   (`template_digest`) when it is created or switches environment. Before each new sandbox for it, the gateway moves it to the
   template's current image when its latest run is finished and the template is offered, and records the template version it came
   from (logged at INFO). A security fix, or an upgrade of the built-in templates, so reaches bound conversations without a switch.
   - A run in progress (running or parked) keeps the image it started with, so a resumed run never changes environment mid-run.
   - A template that is not offered keeps the old pin, and a disabled or refused template stays usable by the conversations bound to it.
   - A pin the runtime floor refuses (an upgrade with no offered image to move to) is retired with an `unsupported_protocol` error
     that tells the owner to switch the environment or ask an administrator, never a silently failed sandbox (GW-I-087).
   - With protocol revision 8 the runtime announces an image-only update as `environment.changed` carrying the template version,
     never a digest (RT-I-029).
8. **One rootfs key: the image ID.**
   - microVM builds live in `<storage.root>/images/<image id>/`. A registry-digest pin is located through the builds' recorded
     repository digests: exactly one build may list it, and none or several is refused by name (SBX-F-022).
   - The rootfs is an operator step (`images/rootfs/build_rootfs.py`). A rootfs built by another VM host image is `outdated` and refused
     at create, since its guest init lacks newer guarantees; create and the status share one check, so they always agree.
9. **The gateway never deletes images or rootfs builds.** Removing them is an operator action.

## Consequences

- An administrator adds, replaces or retires an environment without editing files or restarting.
- A bad template shows its reason on the admin page (the check state and code) instead of failing at a user's first message, and it is
  never offered.
- A registry-digest pin works on both backends.
- A template stays unchecked for as long as the engine cannot answer; a file-provided one is still offered meanwhile, an API-created
  one is not.
- A VM host image upgrade means rebuilding every template's rootfs (deploy/firecracker/README.md).
- A new template image reaches every idle conversation bound to it at its next sandbox (decision 7); a run in progress finishes on
  the image it started with.
- A secret *file* inside an image is undetectable on both backends; only an environment key is checked, by name.
- Conversations today are only active or deleted, and a deleted one is never restored, so "in use" counts active conversations only. If
  an archive and restore feature arrives, a restored conversation whose template was deleted binds to the current default template and
  emits the existing `environment.changed` notice, so the user and the agent see the switch; the restore is neither refused nor silent.
- A new dependency, `tar` (MIT/Apache-2.0, default features off), reads the single entry of the engine's archive of the runtime binary,
  streamed within the size bound; nothing is unpacked to disk.
- **Tests:** GW-U-041, GW-I-075 to GW-I-080, GW-I-087, RT-I-029, SBX-U-007, SBX-U-008, SBX-D-009, SBX-F-022, SBX-F-023, WEB-U-047, WEB-U-048; existing
  SBX-C-014, SBX-C-017, GW-I-050.

## Alternatives

- **The gateway pulls images.** Rejected: registry credentials and egress would live in the control plane. Pulling on the engine host
  is an operator's normal job.
- **Check at write time and refuse the write.** Rejected: an engine that is slow or down would block every write, and the engine can
  change after a write anyway. Background checks with stored results keep writes fast and show the reason where the template is managed.
- **The gateway builds the rootfs.** Deferred: it needs the builder in Rust, or a builder container the gateway drives, with its own
  resource bounds.
- **Verify derivation by running the image** (for example `tqa-runtime --version`). Rejected: it runs untrusted code within the control
  plane's reach. The layer prefix and the copied binary's hash prove the same without executing anything.
- **Key microVM builds by registry digest.** Rejected: one image has many registry digests (mirrors, re-tags) but one ID, and a local
  build has none.

## Links

ARCHITECTURE §4.7, §15, §15.1, §18; ADR-0006, ADR-0012 (profiles reference templates by name), ADR-0026, ADR-0028, ADR-0031
(`[[mcp.servers]]` entries list templates); DEV-PLAN Phase 6.
