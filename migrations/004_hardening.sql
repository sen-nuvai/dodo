CREATE TABLE IF NOT EXISTS webhook_events (
    id uuid PRIMARY KEY,
    registration_id uuid NOT NULL REFERENCES webhook_registrations(id),
    event_id text NOT NULL,
    payload bytea NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(registration_id, event_id)
);
CREATE INDEX IF NOT EXISTS webhook_events_event ON webhook_events(registration_id, event_id);
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS last_error text;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS max_attempts int NOT NULL DEFAULT 10;
CREATE INDEX IF NOT EXISTS webhook_deliveries_claim ON webhook_deliveries(next_attempt_at, id) WHERE delivered_at IS NULL;
