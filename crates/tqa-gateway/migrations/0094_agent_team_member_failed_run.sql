-- E2E-AGENTS-021 (7 Oct 2026): a helper whose run failed showed "Working…", since only a first turn ever stored
-- `failed`. A failed run (or a sandbox lost too often to replace) now makes it `failed` with `failed_run` set: unlike
-- a helper that couldn't start, it keeps its seat and the lead's next message wakes it (ADR-0034 point 9: "the lead
-- may assign again or stop_member"); its next run makes it `working` and clears the flag.
ALTER TABLE agent_team_members ADD COLUMN failed_run boolean NOT NULL DEFAULT false;
-- Its failures, counted once each (a stored run.failed, a give-up): the wake's key names the count, so the next message
-- after each failure wakes it once more, though a run that failed before reading leaves its place in the log unmoved.
ALTER TABLE agent_team_members ADD COLUMN failures integer NOT NULL DEFAULT 0 CHECK (failures >= 0);
