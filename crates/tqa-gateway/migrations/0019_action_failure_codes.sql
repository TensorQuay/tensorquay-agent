-- A failed action records the wire code that explains it: a sandbox failure or a runtime that exited at start.
ALTER TABLE idempotency DROP CONSTRAINT IF EXISTS idempotency_execution_error_check;
ALTER TABLE idempotency ADD CONSTRAINT idempotency_execution_error_check
    CHECK (execution_error IN ('sandbox_failed','runtime_start_failed'));
