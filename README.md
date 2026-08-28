# Dodo Payments

A small Rust/Axum payment API with tenant-scoped PostgreSQL persistence, invoice payments, payment attempts, a mock PSP, idempotency, and signed asynchronous webhooks.

## Architecture

PostgreSQL is the durable coordination boundary when `DATABASE_URL` is set. Invoice payment uses a short transaction to lock the tenant-scoped invoice and create a pending payment claim before any PSP call. The claim commits, the PSP is called with a two-second deadline, and a second short transaction finalizes the payment and invoice. The in-memory store remains a development fallback only.

## Routes

- `GET /health`
- Customer CRUD: `/customers`
- Invoice create/list/get: `/invoices`, `/invoices/{id}`
- `POST /invoices/{id}/finalize` (`draft → open`)
- `POST /invoices/{id}/pay` (only `open` invoices)
- Payment create/get/attempts: `/payments`, `/payments/{id}`, `/payments/{id}/attempts`
- Webhook registration and inbound mock events: `/webhooks`, `/webhooks/mock`, `/webhooks/{registration_id}`

PostgreSQL routes require `X-API-Key` in `prefix_secret` format. When `API_TOKEN` is configured, bearer middleware also applies.

## State and idempotency

Invoice states are `draft`, `open`, `paid`, `void`, and `uncollectible`. Payment attempts are `pending`, `succeeded`, or `failed`. Failed and unknown PSP outcomes leave the invoice `open`; a timeout leaves the payment attempt `pending` because the provider may have charged.

`Idempotency-Key` is tenant-scoped. Its fingerprint is a versioned SHA-256 over invoice ID, payment token, amount, and uppercase currency. A matching key/fingerprint replays the durable operation without a PSP call; a changed fingerprint returns `409 Conflict`. PostgreSQL row locking prevents two local claims for one invoice. Exactly-once external charging is not claimed because the mock PSP has no reconciliation API.

## Webhooks

`invoice.created`, `invoice.paid`, and `invoice.payment_failed` are queued in the same PostgreSQL transaction as the corresponding state change. Delivery is asynchronous through PostgreSQL rows and `FOR UPDATE SKIP LOCKED`. Payload bytes are signed as `timestamp + "." + raw_payload` with HMAC-SHA256. Consumers should reject timestamps older than five minutes. Deliveries retry at most five times: immediate, +5 seconds, +30 seconds, +5 minutes, +30 minutes, then become exhausted.

## Run and test

Requirements: Rust, Docker Compose, and PostgreSQL for integration coverage.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
DATABASE_URL=postgres://dodo:dodo@localhost:5432/dodo cargo test --test postgres
BOOTSTRAP_API_KEY=demo_secret docker compose up --build
```

Override `APP_PORT` or `PSP_PORT` when host ports are occupied. Compose migrations run on a fresh database volume; existing volumes retain SQLx migration history and should not be deleted without review.

Example payment sequence:

```bash
# Create invoice, then finalize it
curl -X POST http://localhost:3000/invoices/{id}/finalize
curl -X POST http://localhost:3000/invoices/{id}/pay \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: invoice-001' \
  -d '{"payment_method_token":"tok_success"}'
```

The mock PSP supports `tok_success`, `tok_insufficient_funds`, `tok_card_declined`, `tok_timeout`, and `tok_network_error`. A demo video is intentionally a submission artifact: **[demo video placeholder]**.

This take-home is not PCI compliant and is not suitable for real funds without provider idempotency/reconciliation, secret management, observability, and operational controls.
