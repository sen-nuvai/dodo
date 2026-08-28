-- Preserve the exact signed webhook bytes for durable delivery.
ALTER TABLE webhook_deliveries
    ALTER COLUMN payload TYPE bytea USING convert_to(payload::text, 'UTF8');
