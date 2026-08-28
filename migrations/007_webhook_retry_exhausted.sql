ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS exhausted_at timestamptz;
CREATE INDEX IF NOT EXISTS webhook_deliveries_claim ON webhook_deliveries(next_attempt_at, id) WHERE delivered_at IS NULL AND exhausted_at IS NULL;
