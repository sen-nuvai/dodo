CREATE INDEX IF NOT EXISTS payments_idempotency_lookup ON payments (tenant_id, idempotency_key);
CREATE INDEX IF NOT EXISTS webhook_deliveries_due ON webhook_deliveries (next_attempt_at) WHERE delivered_at IS NULL;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE webhook_registrations ADD COLUMN IF NOT EXISTS active boolean NOT NULL DEFAULT true;
CREATE UNIQUE INDEX IF NOT EXISTS webhook_delivery_event ON webhook_deliveries (registration_id, event_id);
