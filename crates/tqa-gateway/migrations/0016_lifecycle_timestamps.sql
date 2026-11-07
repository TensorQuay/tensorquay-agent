ALTER TABLE runtime_token_reservations ADD COLUMN updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE sandbox_instances ADD COLUMN updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP;
