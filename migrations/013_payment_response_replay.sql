ALTER TABLE payments ADD COLUMN IF NOT EXISTS response_status integer NOT NULL DEFAULT 402;
ALTER TABLE payments ADD COLUMN IF NOT EXISTS response_body jsonb;
