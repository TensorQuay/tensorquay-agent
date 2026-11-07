-- Messages sent while the task's run is in progress wait on the gateway and go out as one turn when it ends (GW-I-129).
-- `held` marks a message that waited (or was put back after the runtime refused it as busy), so the queue's list shows
-- it; a person may cancel one before it goes out (`sending` marks a turn's messages claimed in the transaction that
-- decides they go out, so a cancel either wins or finds them sent); a run that fails or is stopped drops its held messages. Independent
-- of 0058 (decision engine budgets), which touches other tables; placeholder number, set at merge. The failure codes
-- keep 0056's list (decision_engine_unavailable) and add the two a run's end gives.
ALTER TABLE idempotency ADD COLUMN held boolean NOT NULL DEFAULT false;
ALTER TABLE idempotency DROP CONSTRAINT idempotency_execution_status_check;
ALTER TABLE idempotency ADD CONSTRAINT idempotency_execution_status_check
    CHECK (execution_status IN ('queued','sending','delivered','failed','cancelled'));
ALTER TABLE idempotency DROP CONSTRAINT idempotency_execution_error_check;
ALTER TABLE idempotency ADD CONSTRAINT idempotency_execution_error_check
    CHECK (execution_error IN ('sandbox_failed','runtime_start_failed','decision_engine_unavailable','run_failed',
                               'run_cancelled'));
