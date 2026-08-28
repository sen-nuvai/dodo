-- Payment and invoice lifecycle hardening.
-- Keep migration versions unique: this is the only migration 007.
ALTER TABLE invoices ADD COLUMN IF NOT EXISTS finalized_at timestamptz;
ALTER TABLE invoices ADD CONSTRAINT invoices_status_check CHECK (status IN ('draft','open','pending','paid','failed'));
ALTER TABLE payments ADD CONSTRAINT payments_status_check CHECK (status IN ('pending','succeeded','failed'));
CREATE INDEX IF NOT EXISTS payments_tenant_status ON payments(tenant_id, status);
CREATE INDEX IF NOT EXISTS invoices_tenant_status ON invoices(tenant_id, status);
