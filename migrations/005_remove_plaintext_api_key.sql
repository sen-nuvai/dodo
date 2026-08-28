-- Plaintext tenant API keys are no longer used; authentication is prefix + hash only.
ALTER TABLE tenants DROP COLUMN IF EXISTS api_key;
