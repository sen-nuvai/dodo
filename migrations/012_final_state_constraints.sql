-- Final invoice lifecycle constraints. Legacy pending/failed values were normalized by migration 010.
ALTER TABLE invoices DROP CONSTRAINT IF EXISTS invoices_status_check;
ALTER TABLE invoices ADD CONSTRAINT invoices_status_check CHECK (status IN ('draft','open','paid','void','uncollectible'));
ALTER TABLE payments DROP CONSTRAINT IF EXISTS payments_status_check;
ALTER TABLE payments ADD CONSTRAINT payments_status_check CHECK (status IN ('pending','succeeded','failed'));
ALTER TABLE webhook_deliveries ALTER COLUMN max_attempts SET DEFAULT 5;
UPDATE webhook_deliveries SET max_attempts = 5 WHERE max_attempts > 5;
