-- Durable invoice payment claims are persisted before any PSP call.
ALTER TABLE invoices ADD COLUMN IF NOT EXISTS payment_claimed_at timestamptz;
CREATE INDEX IF NOT EXISTS invoices_payment_claimed ON invoices(payment_claimed_at) WHERE status = 'pending';

-- Business events use the existing durable webhook delivery queue.
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS event_type text NOT NULL DEFAULT 'payment';
