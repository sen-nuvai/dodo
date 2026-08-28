ALTER TABLE tenants ADD COLUMN IF NOT EXISTS api_key_prefix text;
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS api_key_hash text;
CREATE UNIQUE INDEX IF NOT EXISTS tenants_api_key_prefix ON tenants(api_key_prefix) WHERE api_key_prefix IS NOT NULL;
