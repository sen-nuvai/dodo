# Dodo Payments

A small Rust/Axum payment API with tenant-scoped PostgreSQL persistence, invoice payments, payment attempts, a mock PSP, durable idempotency, and signed asynchronous webhooks.

## Architecture

When `DATABASE_URL` is configured, PostgreSQL is the durability and coordination boundary. Invoice payment locks the tenant-scoped invoice, checks the tenant/key fingerprint, creates a pending payment claim, commits that claim, calls the PSP with a two-second deadline, and finalizes the payment in a second transaction. The row lock is never held during PSP network I/O. The in-memory store is an explicit development fallback only.

The mock PSP supports `tok_success`, `tok_insufficient_funds`, `tok_card_declined`, `tok_timeout`, and `tok_network_error`. A timeout is unknown: the attempt remains `pending`, the invoice remains `open`, and the service does not blindly retry the charge. The mock PSP has no reconciliation API, so exactly-once external charging is not claimed across a process crash.

## Routes

- `GET /health`
- Customer CRUD: `/customers`
- Invoice create/list/get: `/invoices`, `/invoices/{id}`
- `POST /invoices/{id}/finalize` (`draft → open`)
- `POST /invoices/{id}/pay` (only `open` invoices)
- Payment create/get/attempts: `/payments`, `/payments/{id}`, `/payments/{id}/attempts`
- Webhook registration and inbound events: `/webhooks`, `/webhooks/mock`, `/webhooks/{registration_id}`

PostgreSQL routes require `X-API-Key` in `prefix_secret` format. When `API_TOKEN` is configured, bearer middleware also applies.

## State and idempotency

Invoice states are `draft`, `open`, `paid`, `void`, and `uncollectible`. Payment and payment-attempt states are `pending`, `succeeded`, and `failed`. Failed and unknown PSP outcomes leave the invoice `open`.

`Idempotency-Key` is tenant-scoped by `(tenant_id, idempotency_key)`. The fingerprint is a versioned, length-delimited SHA-256 over invoice ID, payment token, amount, and uppercase currency. A matching key/fingerprint returns the persisted payment response without another PSP call when complete. A matching in-flight claim returns `202 Accepted` with pending payment data; callers do not synchronously receive the eventual result. A changed fingerprint returns `409 Conflict`. PostgreSQL invoice locking prevents concurrent local claims for one invoice. Payment response status and JSON response data are persisted for direct payment replay. Raw tokens are never persisted or logged.

## Webhooks

`invoice.created`, `invoice.paid`, and `invoice.payment_failed` are inserted into the PostgreSQL delivery outbox in the same transaction as the related state change. A background worker claims rows with `FOR UPDATE SKIP LOCKED` and a lease, then posts asynchronously.

Outbound requests include:

```text
X-Webhook-Timestamp: <unix_timestamp>
X-Webhook-Signature: sha256=<hex_hmac>
Content-Type: application/json
```

The HMAC input is exactly `timestamp + "." + raw_payload`. Consumers should reject timestamps outside the five-minute replay window. Failed deliveries have at most five attempts: immediate, +5 seconds, +30 seconds, +5 minutes, and +30 minutes. Attempt five becomes `exhausted` with the last error retained.

## Run and test

Requirements: Rust, Docker Compose, and PostgreSQL for live integration coverage.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

For live PostgreSQL tests, start an isolated database with a host port and provide its connection string through the environment:

```bash
DATABASE_URL=postgres://<user>:<password>@127.0.0.1:<port>/<database> cargo test --test postgres
```

Start the application stack, overriding host ports when necessary:

```bash
APP_PORT=3303 PSP_PORT=4403 BOOTSTRAP_API_KEY=demo_secret docker compose up --build
```

Compose runs SQLx migrations from the application. Existing persisted volumes retain migration history and must not be deleted without review.

## Example payment sequence

```bash
base=http://localhost:3303
key=demo_secret
curl -X POST "$base/invoices/{id}/finalize" -H "X-API-Key: $key"
curl -X POST "$base/invoices/{id}/pay" \
  -H "X-API-Key: $key" \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: invoice-001' \
  -d '{"payment_method_token":"tok_success"}'
```

## Demo video

[Watch the 2–3 minute Loom demo](https://www.loom.com/share/be42a223e3cb47a2b135c350bd05eb10)

The video demonstrates Docker Compose startup, tenant-scoped customer and invoice creation, draft-to-open finalization, successful and declined payments, idempotent replay, and durable webhook events.

This take-home is not PCI compliant and is not suitable for real funds without provider idempotency/reconciliation, secret management, observability, and operational controls.
