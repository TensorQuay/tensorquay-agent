CREATE TABLE sandbox_instances (
    conversation_id uuid PRIMARY KEY REFERENCES conversations(id),
    sandbox_id text UNIQUE CHECK (octet_length(sandbox_id) BETWEEN 1 AND 128),
    phase text NOT NULL DEFAULT 'starting' CHECK (phase IN ('starting','live','destroying')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_action_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE idempotency ADD COLUMN conversation_id uuid REFERENCES conversations(id);
ALTER TABLE idempotency ADD COLUMN queued_frame jsonb CHECK (octet_length(queued_frame::text)<=1048576);
ALTER TABLE idempotency ADD COLUMN execution_status text NOT NULL DEFAULT 'delivered'
    CHECK (execution_status IN ('queued','delivered','failed'));
ALTER TABLE idempotency ADD COLUMN execution_error text CHECK (execution_error='sandbox_failed');
ALTER TABLE idempotency ADD COLUMN ordinal bigint GENERATED ALWAYS AS IDENTITY;
ALTER TABLE idempotency ADD CONSTRAINT queued_action_has_frame
    CHECK (execution_status<>'queued' OR (conversation_id IS NOT NULL AND queued_frame IS NOT NULL));
CREATE INDEX idempotency_queue ON idempotency(conversation_id,ordinal) WHERE execution_status='queued';
