<<<<<<< HEAD
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS event_type text NOT NULL DEFAULT 'provider.event';
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS exhausted_at timestamptz;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS lease_until timestamptz;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE webhook_events ADD COLUMN IF NOT EXISTS event_type text NOT NULL DEFAULT 'provider.event';
CREATE INDEX IF NOT EXISTS webhook_deliveries_due ON webhook_deliveries(next_attempt_at, id)
=======
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS exhausted_at timestamptz;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS lease_until timestamptz;
ALTER TABLE webhook_deliveries ALTER COLUMN max_attempts SET DEFAULT 5;
UPDATE webhook_deliveries SET max_attempts = 5 WHERE max_attempts > 5;
CREATE INDEX IF NOT EXISTS webhook_deliveries_claim ON webhook_deliveries(next_attempt_at, id)
>>>>>>> 36e8a96 (Harden webhook outbox and signatures)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;
