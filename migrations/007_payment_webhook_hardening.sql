ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS event_type text NOT NULL DEFAULT 'provider.event';
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS exhausted_at timestamptz;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS lease_until timestamptz;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE webhook_events ADD COLUMN IF NOT EXISTS event_type text NOT NULL DEFAULT 'provider.event';
CREATE INDEX IF NOT EXISTS webhook_deliveries_due ON webhook_deliveries(next_attempt_at, id)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;
