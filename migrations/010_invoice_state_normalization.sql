-- Normalize invoice state vocabulary and enforce the durable state machine.
UPDATE invoices SET status='open' WHERE status IN ('pending', 'failed');
ALTER TABLE invoices DROP CONSTRAINT IF EXISTS invoices_status_check;
ALTER TABLE invoices ADD CONSTRAINT invoices_status_check CHECK (status IN ('draft','open','paid','void','uncollectible'));
