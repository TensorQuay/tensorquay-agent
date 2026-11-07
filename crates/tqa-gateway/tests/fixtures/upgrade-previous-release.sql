-- level: 103
-- GW-I-016: the previous release's database, as synthetic rows in every table that exists at the level above (the
-- highest migration the previous release shipped). The test migrates an empty database to that level, loads this file,
-- upgrades it with every newer migration and checks that each row is kept and that the schema equals a fresh install's.
--
-- Written by hand against the migrations up to the level, never copied from a real database. Each table has at least
-- one row (the test fails on a table without one), with the awkward cases a long-lived installation has: optional
-- columns set and unset, a customised and a deleted built-in, a refused environment check, a running and an ended team,
-- a helper whose run failed and kept its seat, queued and held messages (one copied to its team's log), usage of a
-- purged task and an estimated one, screen take-overs and hand-backs, a purged screenshot, a deleted Desktop, and seeded
-- and administrator settings.
--
-- At each release, once it is on main: raise the level to that release's highest migration, add rows for the tables it
-- added, and adjust rows for columns it changed (docs/release.md, "The upgrade fixture").

-- People.
INSERT INTO users (id, email, password_hash, role, created_at, updated_at) VALUES
    ('00000000-0000-7000-8000-000000000101', 'admin@example.invalid', 'fixture-hash', 'admin',
     '2026-09-01 08:00:00+00', '2026-09-01 08:00:00+00');
INSERT INTO users (id, email, password_hash, role, invited_by, last_active_at) VALUES
    ('00000000-0000-7000-8000-000000000102', 'Member@Example.invalid', 'fixture-hash', 'member',
     '00000000-0000-7000-8000-000000000101', '2026-10-09 17:30:00+00');
INSERT INTO users (id, email, password_hash, role, invited_by, disabled_at) VALUES
    ('00000000-0000-7000-8000-000000000103', 'left@example.invalid', 'fixture-hash', 'member',
     '00000000-0000-7000-8000-000000000101', '2026-10-02 12:00:00+00');

INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES
    ('00000000-0000-7000-8000-000000000111', '00000000-0000-7000-8000-000000000101',
     sha256('fixture:session-admin'::bytea), '2026-11-01 00:00:00+00'),
    ('00000000-0000-7000-8000-000000000112', '00000000-0000-7000-8000-000000000102',
     sha256('fixture:session-member'::bytea), '2026-10-01 00:00:00+00');

INSERT INTO invitations (id, email, role, token_hash, created_by, expires_at, accepted_by, accepted_at) VALUES
    ('00000000-0000-7000-8000-000000000121', 'member@example.invalid', 'member', sha256('fixture:invite-1'::bytea),
     '00000000-0000-7000-8000-000000000101', '2026-09-08 00:00:00+00', '00000000-0000-7000-8000-000000000102',
     '2026-09-02 09:00:00+00');
INSERT INTO invitations (id, email, role, token_hash, created_by, expires_at, revoked_at) VALUES
    ('00000000-0000-7000-8000-000000000122', 'later@example.invalid', 'admin', sha256('fixture:invite-2'::bytea),
     '00000000-0000-7000-8000-000000000101', '2026-10-20 00:00:00+00', '2026-10-05 10:00:00+00');

INSERT INTO api_tokens (id, user_id, name, hint, token_hash, expires_at, last_used_at) VALUES
    ('00000000-0000-7000-8000-000000000131', '00000000-0000-7000-8000-000000000101', 'ci', 'ab12',
     sha256('fixture:api-token-1'::bytea), '2027-01-01 00:00:00+00', '2026-10-09 06:00:00+00');
INSERT INTO api_tokens (id, user_id, name, hint, token_hash, expires_at, revoked_at) VALUES
    ('00000000-0000-7000-8000-000000000132', '00000000-0000-7000-8000-000000000102', 'old laptop', 'cd34',
     sha256('fixture:api-token-2'::bytea), '2027-01-01 00:00:00+00', '2026-10-03 08:00:00+00');

-- Workspaces first (a task needs its workspace); the lease that names a task is set once the task exists.
INSERT INTO workspaces (id, owner_id, name, source_url, last_used_at, size_bytes, size_measured_at, quota_bytes)
VALUES
    ('00000000-0000-7000-8000-000000000201', '00000000-0000-7000-8000-000000000101', 'Release notes',
     'https://git.example.com/team/notes.git', '2026-10-09 17:00:00+00', 1048576, '2026-10-09 17:05:00+00',
     10737418240);
INSERT INTO workspaces (id, owner_id, name, state) VALUES
    ('00000000-0000-7000-8000-000000000202', '00000000-0000-7000-8000-000000000102', 'Scratch', 'deleted');
INSERT INTO workspaces (id, owner_id, name, forked_from, fork_consistent, kind) VALUES
    ('00000000-0000-7000-8000-000000000203', '00000000-0000-7000-8000-000000000101', 'Find the sources',
     '00000000-0000-7000-8000-000000000201', true, 'helper_copy');
INSERT INTO workspaces (id, owner_id, name, state) VALUES
    ('00000000-0000-7000-8000-000000000204', '00000000-0000-7000-8000-000000000101', 'Weekly report', 'failed');

INSERT INTO workspace_shares (workspace_id, user_id, access, created_by) VALUES
    ('00000000-0000-7000-8000-000000000201', '00000000-0000-7000-8000-000000000102', 'read',
     '00000000-0000-7000-8000-000000000101');

-- Shared profiles and personal ones (a task keeps a copy of the profile it started with).
INSERT INTO agent_profiles (id, owner_id, name, document, last_set_by) VALUES
    ('00000000-0000-7000-8000-000000000601', NULL, 'reviewer',
     '{"name": "reviewer", "tools": ["files", "shell"], "repositories": true}', 'admin'),
    ('00000000-0000-7000-8000-000000000602', '00000000-0000-7000-8000-000000000102', 'notes',
     '{"name": "notes", "tools": ["files"], "repositories": false}', 'owner');

-- Tasks: a team's lead and its helper, a deleted task, and an ended team's lead and helper.
INSERT INTO conversations (id, owner_id, title, model_route, backend, workspace_id, template, template_digest,
                           template_version, pinned, mcp_servers, connectors, profile_id, profile_version, profile,
                           title_source, last_activity_at)
VALUES
    ('00000000-0000-7000-8000-000000000301', '00000000-0000-7000-8000-000000000101', 'Write the release notes',
     'local', 'docker', '00000000-0000-7000-8000-000000000201', 'workstation', 'sha256:fixture', 2, true,
     '{issues}', '{issues}', '00000000-0000-7000-8000-000000000601', 1,
     '{"name": "reviewer", "tools": ["files", "shell"], "repositories": true}', 'person', '2026-10-09 17:00:00+00');
INSERT INTO conversations (id, owner_id, title, model_route, backend, workspace_id, title_source) VALUES
    ('00000000-0000-7000-8000-000000000302', '00000000-0000-7000-8000-000000000101', 'Find the sources', 'local',
     'docker', '00000000-0000-7000-8000-000000000203', 'model');
INSERT INTO conversations (id, owner_id, title, model_route, backend, state, workspace_dir, workspace_id) VALUES
    ('00000000-0000-7000-8000-000000000303', '00000000-0000-7000-8000-000000000102', 'An old experiment', 'seeded',
     'firecracker', 'deleted', 'legacy/an-old-experiment', '00000000-0000-7000-8000-000000000202');
INSERT INTO conversations (id, owner_id, title, model_route, backend, workspace_id) VALUES
    ('00000000-0000-7000-8000-000000000304', '00000000-0000-7000-8000-000000000101', 'Weekly report', 'local',
     'docker', '00000000-0000-7000-8000-000000000204'),
    ('00000000-0000-7000-8000-000000000305', '00000000-0000-7000-8000-000000000101', 'Draft the report', 'local',
     'docker', '00000000-0000-7000-8000-000000000204');

UPDATE workspaces SET lease_conversation_id = '00000000-0000-7000-8000-000000000301',
                      lease_expires_at = '2026-10-09 18:00:00+00'
WHERE id = '00000000-0000-7000-8000-000000000201';

INSERT INTO events (conversation_id, seq, run_id, step, ts, frame, lifecycle) VALUES
    ('00000000-0000-7000-8000-000000000301', 1, 'run-1', NULL, '2026-10-09 16:00:00+00',
     '{"type": "event", "seq": 1, "run_id": "run-1", "event": {"type": "run.started", "run_id": "run-1"}}',
     'run.started'),
    ('00000000-0000-7000-8000-000000000301', 4, 'run-1', 3, '2026-10-09 16:05:00+00',
     '{"type": "event", "seq": 4, "run_id": "run-1", "event": {"type": "run.completed", "run_id": "run-1"}}',
     'run.completed'),
    ('00000000-0000-7000-8000-000000000303', 1, 'run-9', NULL, '2026-09-20 10:00:00+00',
     '{"type": "event", "seq": 1, "run_id": "run-9", "event": {"type": "run.failed", "run_id": "run-9"}}',
     'run.failed');
INSERT INTO events (conversation_id, seq, run_id, step, ts, frame, call_id) VALUES
    ('00000000-0000-7000-8000-000000000301', 2, 'run-1', 1, '2026-10-09 16:01:00+00',
     '{"type": "event", "seq": 2, "run_id": "run-1", "event": {"type": "tool.call", "call_id": "call-1"}}', 'call-1');
INSERT INTO events (conversation_id, seq, run_id, step, ts, blob_key) VALUES
    ('00000000-0000-7000-8000-000000000301', 3, 'run-1', 2, '2026-10-09 16:02:00+00', 'blob-301-3');

-- Messages and actions: delivered, queued and held, failed, and one about a deleted task. `ordinal` is the identity.
INSERT INTO idempotency (key, user_id, route, response_status, response_body, body_hash) VALUES
    ('fixture-create-task', '00000000-0000-7000-8000-000000000101', 'POST /api/v1/conversations', 201,
     convert_to('{"id": "00000000-0000-7000-8000-000000000301"}', 'UTF8'), sha256('fixture:body-1'::bytea));
INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, queued_frame,
                         execution_status, held) VALUES
    ('fixture-queued', '00000000-0000-7000-8000-000000000101', 'POST /api/v1/conversations/{id}/messages', 202,
     convert_to('{"queued": true}', 'UTF8'), '00000000-0000-7000-8000-000000000301',
     convert_to('{"type": "user.message", "text": "And the known limits"}', 'UTF8'), 'queued', true);
INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, execution_status,
                         execution_error) VALUES
    ('fixture-failed', '00000000-0000-7000-8000-000000000102', 'POST /api/v1/conversations/{id}/messages', 202,
     convert_to('{"queued": true}', 'UTF8'), '00000000-0000-7000-8000-000000000303', 'failed', 'run_failed');
INSERT INTO idempotency (key, user_id, route, response_status, response_body, about_conversation) VALUES
    ('fixture-delete', '00000000-0000-7000-8000-000000000102', 'DELETE /api/v1/conversations/{id}', 204,
     convert_to('{}', 'UTF8'), '00000000-0000-7000-8000-000000000303');

-- Sandboxes and their tokens.
INSERT INTO runtime_tokens (conversation_id, token_hash, sandbox_id, peer_address) VALUES
    ('00000000-0000-7000-8000-000000000301', sha256('fixture:runtime-301'::bytea), 'sbx-301', '192.0.2.10');
INSERT INTO runtime_token_reservations (id, conversation_id, token_hash, expires_at) VALUES
    ('00000000-0000-7000-8000-000000000311', '00000000-0000-7000-8000-000000000302',
     sha256('fixture:reservation-302'::bytea), '2026-10-09 18:00:00+00');
INSERT INTO sandbox_instances (conversation_id, sandbox_id, phase, idle_seconds, max_lifetime_seconds,
                               cpu_millicores, memory_bytes, pids) VALUES
    ('00000000-0000-7000-8000-000000000301', 'sbx-301', 'live', 900, 86400, 2000, 4294967296, 1024);
INSERT INTO sandbox_instances (conversation_id, sandbox_id) VALUES
    ('00000000-0000-7000-8000-000000000302', 'sbx-302');

INSERT INTO workspace_snapshots (id, workspace_id, conversation_id, after_seq, files, truncated) VALUES
    ('00000000-0000-7000-8000-000000000211', '00000000-0000-7000-8000-000000000201',
     '00000000-0000-7000-8000-000000000301', 4, 12, false);

INSERT INTO usage (id, conversation_id, user_id, run_id, route, prompt_tokens, completion_tokens, cached_tokens,
                   reasoning_tokens, kind) VALUES
    ('00000000-0000-7000-8000-000000000321', '00000000-0000-7000-8000-000000000301',
     '00000000-0000-7000-8000-000000000101', 'run-1', 'local', 1200, 300, 800, 40, 'model'),
    ('00000000-0000-7000-8000-000000000322', '00000000-0000-7000-8000-000000000301',
     '00000000-0000-7000-8000-000000000101', NULL, 'local', 90, 8, NULL, NULL, 'title'),
    ('00000000-0000-7000-8000-000000000323', NULL, '00000000-0000-7000-8000-000000000102', 'run-8', 'seeded',
     NULL, NULL, NULL, NULL, 'model');
INSERT INTO usage (id, conversation_id, user_id, run_id, route, prompt_tokens, completion_tokens, kind, estimated) VALUES
    ('00000000-0000-7000-8000-000000000324', '00000000-0000-7000-8000-000000000302',
     '00000000-0000-7000-8000-000000000101', 'run-2', 'local', 5200, 13974, 'model', true);

-- The Desktop's screen: a take-over, a hand-back, one ended by a restart, a sign-out that failed; stored screenshots.
INSERT INTO screen_notices (id, conversation_id, kind, by_user, by_name, at, after_seq, seconds, ended, reason) VALUES
    ('00000000-0000-7000-8000-000000000331', '00000000-0000-7000-8000-000000000301', 'screen.taken_over', '00000000-0000-7000-8000-000000000101',
     'admin@example.invalid', '2026-10-09 16:03:00+00', 2, NULL, NULL, NULL),
    ('00000000-0000-7000-8000-000000000332', '00000000-0000-7000-8000-000000000301', 'screen.handed_back', '00000000-0000-7000-8000-000000000101',
     'admin@example.invalid', '2026-10-09 16:04:35+00', 2, 95, 'hand_back', NULL),
    ('00000000-0000-7000-8000-000000000333', '00000000-0000-7000-8000-000000000301', 'screen.handed_back', '00000000-0000-7000-8000-000000000101',
     'admin@example.invalid', '2026-10-09 16:10:00+00', 4, NULL, 'restart', NULL),
    ('00000000-0000-7000-8000-000000000334', '00000000-0000-7000-8000-000000000301', 'screen.sign_out_failed', '00000000-0000-7000-8000-000000000101',
     'admin@example.invalid', '2026-10-09 16:11:00+00', 4, NULL, NULL, 'unconfirmed');
INSERT INTO screenshots (conversation_id, id, bytes, stored_at, purged_at) VALUES
    ('00000000-0000-7000-8000-000000000301', encode(sha256('fixture:screenshot-1'::bytea), 'hex'), 182044,
     '2026-10-09 16:03:10+00', NULL),
    ('00000000-0000-7000-8000-000000000301', encode(sha256('fixture:screenshot-2'::bytea), 'hex'), 240311,
     '2026-09-30 12:00:00+00', '2026-10-07 12:00:00+00');

-- Environments: a built-in, a customised built-in, an administrator's environment whose check refused it, a seeded one.
INSERT INTO templates (name, display_name, image, capabilities, is_default, version, last_set_by,
                       builtin_image_created, image_id, check_state, checked_version) VALUES
    ('minimal', 'Minimal', 'tqa-minimal:fixture', '{shell,files}', true, 2, 'builtin', '2026-10-01 00:00:00+00',
     'sha256:minimal-fixture', 'verified', 2);
INSERT INTO templates (name, display_name, image, capabilities, cpu_millicores, memory_bytes, pids, workspace_bytes,
                       clamped_from, sizes_from_limits, version, last_set_by, customised_builtin, description,
                       builtin_base_image) VALUES
    ('workstation', 'Workstation', 'tqa-workstation:fixture', '{shell,files,browser,git,python,node}', 2000,
     4294967296, 1024, 10737418240, '{"memory_bytes": 8589934592}', true, 3, 'admin', true,
     'Customised by an administrator', 'sha256:workstation-fixture');
INSERT INTO templates (name, display_name, image, capabilities, version, last_set_by, enabled, base, check_state,
                       check_reason, checked_version, check_secret_files, check_scan_limit) VALUES
    ('research', 'Research', 'tqa-research:fixture', '{shell}', 2, 'admin', false, 'workstation', 'refused',
     'secret_files', 2, '{"items": [{"path": "config/token.txt"}]}', '{"files": 10000}');
INSERT INTO templates (name, display_name, image, capabilities, version, last_set_by, seed_digest, seed_state) VALUES
    ('seeded', 'Seeded', 'tqa-seeded:fixture', '{}', 1, 'seed', sha256('fixture:seed-template'::bytea), 'current');

INSERT INTO builtin_deletions (name, deleted_at) VALUES ('data', '2026-10-04 09:00:00+00');
INSERT INTO builtin_deletions (name, deleted_at, image) VALUES
    ('desktop', '2026-10-30 10:00:00+00', 'sha256:desktop-fixture');

-- Secrets, and everything that names one. Values are placeholders: the test never decrypts them.
INSERT INTO secrets (ref, nonce, ciphertext, rotate_after_days) VALUES
    ('secrets/route-key', decode(repeat('00', 24), 'hex'), decode(repeat('ab', 40), 'hex'), 90);
INSERT INTO secrets (ref, nonce, ciphertext, version, last_set_by, seed_digest, seed_state) VALUES
    ('secrets/issues-token', decode(repeat('01', 24), 'hex'), decode(repeat('cd', 40), 'hex'), 2, 'seed',
     sha256('fixture:seed-secret'::bytea), 'current');
INSERT INTO secrets (ref, nonce, ciphertext) VALUES
    ('secrets/git-' || encode(sha256('fixture:git'::bytea), 'hex'), decode(repeat('02', 24), 'hex'),
     decode(repeat('ef', 40), 'hex')),
    ('secrets/connector-' || encode(sha256('fixture:connector'::bytea), 'hex'), decode(repeat('03', 24), 'hex'),
     decode(repeat('ef', 40), 'hex')),
    ('secrets/host-' || encode(sha256('fixture:host-shared'::bytea), 'hex'), decode(repeat('04', 24), 'hex'),
     decode(repeat('ef', 40), 'hex')),
    ('secrets/host-' || encode(sha256('fixture:host-own'::bytea), 'hex'), decode(repeat('05', 24), 'hex'),
     decode(repeat('ef', 40), 'hex'));

INSERT INTO model_routes (name, display_name, document, description) VALUES
    ('local', 'Local model',
     '{"base_url": "http://127.0.0.1:8000/v1", "model": "fixture", "key_ref": "secrets/route-key", "stream_usage": true}',
     'The model on this host');
INSERT INTO model_routes (name, document, version, last_set_by, seed_digest, seed_state) VALUES
    ('seeded', '{"base_url": "http://127.0.0.1:8001/v1", "model": "seeded"}', 2, 'seed',
     sha256('fixture:seed-route'::bytea), 'current');

INSERT INTO route_probes (route, route_version, document_digest, probed_by, ok, observed) VALUES
    ('local', 1, sha256('fixture:route-document'::bytea), '00000000-0000-7000-8000-000000000101', true,
     '{"models": ["fixture"]}');

INSERT INTO connectors (name, display_name, document) VALUES
    ('issues', 'Issue tracker', '{"url": "https://issues.example.com/mcp", "secret_ref": "secrets/issues-token"}');
INSERT INTO connector_credentials (user_id, connector, origin, secret_ref, last_used_at) VALUES
    ('00000000-0000-7000-8000-000000000102', 'issues', 'https://issues.example.com',
     'secrets/connector-' || encode(sha256('fixture:connector'::bytea), 'hex'), '2026-10-08 11:00:00+00');

-- A credential the previous releases never moved (moved ones were removed at the level's last migration).
INSERT INTO git_credentials (user_id, remote, kind, username, secret_ref) VALUES
    ('00000000-0000-7000-8000-000000000102', 'https://git.example.com/team/notes.git', 'basic', 'builder',
     'secrets/git-' || encode(sha256('fixture:git'::bytea), 'hex'));
INSERT INTO git_push_operations (id, conversation_id, user_id, action_hash, run_id, step, attempt, call_id, remote,
                                 branch, commit_id, state, code, expires_at) VALUES
    (md5('fixture:push'), '00000000-0000-7000-8000-000000000301',
     '00000000-0000-7000-8000-000000000101', encode(sha256('fixture:push-action'::bytea), 'hex'), 'run-1', 3, 1,
     'call-push', 'https://git.example.com/team/notes.git', 'main',
     left(encode(sha256('fixture:commit'::bytea), 'hex'), 40), 'rejected', 'non_fast_forward',
     '2026-10-09 18:00:00+00');

INSERT INTO secret_consumers (ref, consumer_kind, consumer, bound_by) VALUES
    ('secrets/route-key', 'route', 'local', 'admin'),
    ('secrets/issues-token', 'connector', 'issues', 'upgrade');
INSERT INTO secret_reads (id, ref, consumer_kind, consumer, conversation_id, user_id, run_id, outcome, first_at,
                          last_at, count) VALUES
    ('00000000-0000-7000-8000-000000000401', 'secrets/route-key', 'route', 'local',
     '00000000-0000-7000-8000-000000000301', '00000000-0000-7000-8000-000000000101', 'run-1', 'ok',
     '2026-10-09 16:00:00+00', '2026-10-09 16:05:00+00', 3);

-- Keys for outside services: a shared one with a header and a person's own with basic sign-in.
INSERT INTO host_keys (id, owner_id, name, host, port, path_prefix, scheme, header_name, secret_ref, check_name) VALUES
    ('00000000-0000-7000-8000-000000000411', NULL, 'SEARCH_API', 'api.example.com', 443, '/v1', 'header', 'X-Api-Key',
     'secrets/host-' || encode(sha256('fixture:host-shared'::bytea), 'hex'), true);
INSERT INTO host_keys (id, owner_id, name, host, port, scheme, user_name, secret_ref) VALUES
    ('00000000-0000-7000-8000-000000000412', '00000000-0000-7000-8000-000000000102', 'REGISTRY',
     'registry.example.org', 8443, 'basic', 'builder',
     'secrets/host-' || encode(sha256('fixture:host-own'::bytea), 'hex'));
INSERT INTO host_key_users (key_id, user_id) VALUES
    ('00000000-0000-7000-8000-000000000411', '00000000-0000-7000-8000-000000000102');
INSERT INTO host_key_placeholders (hash, key_id, conversation_id, sandbox_id, delivery_id, run_id, ended_at,
                                   sender_id, name, host, port, path_prefix, scheme) VALUES
    (sha256('fixture:placeholder'::bytea), '00000000-0000-7000-8000-000000000411',
     '00000000-0000-7000-8000-000000000301', 'sbx-301', '00000000-0000-7000-8000-000000000421', 'run-1',
     '2026-10-09 16:05:00+00', '00000000-0000-7000-8000-000000000101', 'SEARCH_API', 'api.example.com', 443, '/v1',
     'header');

-- Administration: audit lines of every actor kind, settings, egress, tombstones, decision engines.
INSERT INTO admin_audit (id, actor_kind, actor_user_id, area, item, action, before_version, after_version,
                         value_hash_changed, detail) VALUES
    ('00000000-0000-7000-8000-000000000501', 'admin', '00000000-0000-7000-8000-000000000101', 'secrets',
     'secrets/route-key', 'create', NULL, 1, true, NULL),
    ('00000000-0000-7000-8000-000000000502', 'system', NULL, 'tasks', '00000000-0000-7000-8000-000000000303',
     'delete', NULL, NULL, false, NULL),
    ('00000000-0000-7000-8000-000000000503', 'builtin', NULL, 'templates', 'minimal', 'replace', 1, 2, true, NULL),
    ('00000000-0000-7000-8000-000000000504', 'user', '00000000-0000-7000-8000-000000000102', 'profiles', 'notes',
     'exposure', 1, 2, false, 'private->shared'),
    ('00000000-0000-7000-8000-000000000505', 'seed', NULL, 'limits', 'agent_teams.max_members', 'replace', 1, 2,
     true, NULL);

INSERT INTO settings (id, key, user_id, value, last_set_by) VALUES
    ('00000000-0000-7000-8000-000000000511', 'admission.slots', NULL, 4, 'admin');
INSERT INTO settings (id, key, user_id, value, version, last_set_by, seed_digest, seed_state, clamped_from) VALUES
    ('00000000-0000-7000-8000-000000000512', 'agent_teams.max_members', NULL, 6, 2, 'seed',
     sha256('fixture:seed-setting'::bytea), 'current', 12);
INSERT INTO settings (id, key, user_id, value, last_set_by) VALUES
    ('00000000-0000-7000-8000-000000000513', 'desktop.max_sessions', NULL, 2, 'admin'),
    ('00000000-0000-7000-8000-000000000514', 'computer.max_actions_per_run', NULL, 400, 'admin');
UPDATE settings_generation SET generation = 7 WHERE id = 1;

INSERT INTO egress_rules (host, ports, last_set_by, seed_digest, seed_state, preset) VALUES
    ('pypi.org', '{443}', 'seed', sha256('fixture:seed-egress'::bytea), 'current', 'package_registries');
INSERT INTO egress_rules (host, ports, version, preset, preset_edited) VALUES
    ('*', '{80,443}', 2, 'open_web', true);
INSERT INTO egress_rules (host, ports, internal) VALUES
    ('model-server', '{8000}', true);

INSERT INTO seed_tombstones (area, item, seed_digest) VALUES
    ('routes', 'retired-route', sha256('fixture:tombstone-route'::bytea)),
    ('decision_engines', 'retired-engine', sha256('fixture:tombstone-engine'::bytea));

INSERT INTO decision_engines (name, document, budget_since) VALUES
    ('majority', '{"kind": "vote", "route": "local", "budget": {"tokens": 100000}}', '2026-10-01 00:00:00+00');

UPDATE agent_team_settings SET member_route = 'local', acceptance_check = 'make test', version = 3 WHERE id = 1;

-- Channels: a person's channel with a task, a claim and a vote; a running team's; an ended team's.
INSERT INTO channels (id, owner_id, name, purpose, rule, budget, last_seq) VALUES
    ('00000000-0000-7000-8000-000000000701', '00000000-0000-7000-8000-000000000101', 'Release', 'Ship the release',
     'majority', 1000, 5);
INSERT INTO channels (id, owner_id, name, rule, last_seq, kind) VALUES
    ('00000000-0000-7000-8000-000000000702', '00000000-0000-7000-8000-000000000101', 'Write the release notes',
     'human', 3, 'agent_team');
INSERT INTO channels (id, owner_id, name, rule, state, last_seq, ended_at, kind) VALUES
    ('00000000-0000-7000-8000-000000000703', '00000000-0000-7000-8000-000000000101', 'Weekly report', 'human',
     'ended', 2, '2026-10-08 12:00:00+00', 'agent_team');

INSERT INTO channel_members (channel_id, user_id, conversation_id, role, added_by) VALUES
    ('00000000-0000-7000-8000-000000000701', '00000000-0000-7000-8000-000000000101', NULL, 'lead', NULL),
    ('00000000-0000-7000-8000-000000000701', NULL, '00000000-0000-7000-8000-000000000301', 'member',
     '00000000-0000-7000-8000-000000000101'),
    ('00000000-0000-7000-8000-000000000701', '00000000-0000-7000-8000-000000000102', NULL, 'observer',
     '00000000-0000-7000-8000-000000000101');

INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, spent_tokens, deadline_at,
                         lead_last_read_seq) VALUES
    ('00000000-0000-7000-8000-000000000801', '00000000-0000-7000-8000-000000000702',
     '00000000-0000-7000-8000-000000000301', '00000000-0000-7000-8000-000000000101', 200000, 1234,
     '2026-10-09 20:00:00+00', 2);
INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, state, stop_reason, token_budget, deadline_at,
                         ended_at) VALUES
    ('00000000-0000-7000-8000-000000000802', '00000000-0000-7000-8000-000000000703',
     '00000000-0000-7000-8000-000000000304', '00000000-0000-7000-8000-000000000101', 'ended', 'person', 100000,
     '2026-10-08 14:00:00+00', '2026-10-08 12:00:00+00');

-- A helper whose run failed (it keeps its seat: `failed_run`, one failure counted) and an ended team's stopped helper.
INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state, last_read_seq, failed_run,
                                failures) VALUES
    ('00000000-0000-7000-8000-000000000302', '00000000-0000-7000-8000-000000000801', 'researcher', 'Find the sources',
     'own_copy', 'failed', 1, true, 1),
    ('00000000-0000-7000-8000-000000000305', '00000000-0000-7000-8000-000000000802', 'writer', 'Draft the report',
     'my_files', 'stopped', 2, false, 0);

INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, parent, task_seq,
                              proposal_seq, body, to_kind, to_conversation) VALUES
    ('00000000-0000-7000-8000-000000000701', 1, 'user', '00000000-0000-7000-8000-000000000101', NULL, 'text', NULL,
     NULL, NULL, '{"type": "text", "text": "Plan for the release"}', NULL, NULL),
    ('00000000-0000-7000-8000-000000000701', 2, 'user', '00000000-0000-7000-8000-000000000101', NULL, 'task', 1,
     NULL, NULL, '{"type": "task", "text": "Check the notes"}', NULL, NULL),
    ('00000000-0000-7000-8000-000000000701', 3, 'conversation', '00000000-0000-7000-8000-000000000301',
     'Write the release notes', 'claim', NULL, 2, NULL, '{"type": "claim"}', NULL, NULL),
    ('00000000-0000-7000-8000-000000000701', 4, 'conversation', '00000000-0000-7000-8000-000000000301',
     'Write the release notes', 'proposal', NULL, NULL, NULL, '{"type": "proposal", "text": "Ship on Monday"}', NULL,
     NULL),
    ('00000000-0000-7000-8000-000000000701', 5, 'user', '00000000-0000-7000-8000-000000000102', NULL, 'vote', NULL,
     NULL, 4, '{"type": "vote", "choice": "yes"}', NULL, NULL),
    ('00000000-0000-7000-8000-000000000702', 1, 'gateway', 'gateway', NULL, 'assign', NULL, NULL, NULL,
     '{"type": "assign", "text": "Find the sources"}', 'member', '00000000-0000-7000-8000-000000000302'),
    ('00000000-0000-7000-8000-000000000702', 2, 'conversation', '00000000-0000-7000-8000-000000000302', 'researcher',
     'report', NULL, NULL, NULL, '{"type": "report", "text": "Three sources found"}', 'lead', NULL),
    ('00000000-0000-7000-8000-000000000702', 3, 'user', '00000000-0000-7000-8000-000000000101', NULL,
     'person_message', NULL, NULL, NULL, '{"type": "person_message", "text": "Keep it short"}', 'team', NULL),
    ('00000000-0000-7000-8000-000000000703', 1, 'gateway', 'gateway', NULL, 'notice', NULL, NULL, NULL,
     '{"type": "notice", "text": "writer stopped"}', 'lead', NULL),
    ('00000000-0000-7000-8000-000000000703', 2, 'gateway', 'gateway', NULL, 'ended', NULL, NULL, NULL,
     '{"type": "ended", "reason": "person"}', NULL, NULL);

-- A person's message to the lead while its team runs: held, with its copy in the team's log.
INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, queued_frame,
                         execution_status, held, team_copy_channel, team_copy_seq) VALUES
    ('fixture-team-copy', '00000000-0000-7000-8000-000000000101', 'POST /api/v1/conversations/{id}/messages', 202,
     convert_to('{"queued": true}', 'UTF8'), '00000000-0000-7000-8000-000000000301',
     convert_to('{"type": "user.message", "text": "Keep it short"}', 'UTF8'), 'queued', true,
     '00000000-0000-7000-8000-000000000702', 3);

INSERT INTO channel_claims (channel_id, task_seq, claimant_kind, claimant_id, claim_seq) VALUES
    ('00000000-0000-7000-8000-000000000701', 2, 'conversation', '00000000-0000-7000-8000-000000000301', 3);

INSERT INTO agent_team_reads (reader, call_key, team_id, run_id, seqs, more) VALUES
    ('00000000-0000-7000-8000-000000000301', 'call-read-1', '00000000-0000-7000-8000-000000000801', 'run-1', '{1,2}',
     false);
