# dodo

A Rust/Axum payment API with tenant-scoped Postgres persistence, idempotent payment creation, deterministic mock PSP outcomes, invoices, attempts, and signed webhooks.

## Routes

- `GET /health` returns `ok` without authentication.
- `POST /customers`, `GET /customers`, `GET|PUT /customers/{id}` manage tenant customers.
- `POST /invoices`, `GET /invoices` (optional `status` and `customer_id` filters), and `GET /invoices/{id}` manage invoices.
- `POST /invoices/{id}/finalize` transitions an invoice from `draft` to `open`.
- `POST /invoices/{id}/pay` pays an `open` invoice and creates its payment/attempt transactionally in Postgres.
- `POST /payments` and `GET /payments/{id}` create and retrieve payments; `GET /payments/{id}/attempts` lists attempts.
- `POST /webhooks` registers an outbound target and returns its signing secret. `POST /webhooks/mock` accepts a provider event, while `POST /webhooks/{registration_id}` selects a registration explicitly.

All non-health routes use `X-API-Key` in Postgres mode. If `API_TOKEN` is set, every route also requires either `Authorization: Bearer $API_TOKEN` or the same value in `X-API-Key`. Tenant keys use `prefix_secret` format; only the prefix and SHA-256 hash are stored.

## Payment and invoice statuses

Payments and attempts are `pending`, `succeeded`, or `failed`. Invoices are `draft`, `open`, `paid`, `void`, or `uncollectible`; finalize before paying. A declined payment leaves the invoice `open`; timeout/network results remain a pending attempt with unknown PSP outcome. The mock tokens are `tok_success`, `tok_card_declined`, `tok_insufficient_funds`, `tok_network_error`, and `tok_timeout`; the timeout has a bounded two-second client deadline. Idempotency keys are tenant-scoped and fingerprints include invoice, token, amount, and currency; matching retries replay without another charge and mismatches return `409`.

Outbound `invoice.created`, `invoice.paid`, and `invoice.payment_failed` events are queued transactionally. Delivery is asynchronous and signed with HMAC-SHA256 over `timestamp.payload` using `X-Webhook-Timestamp` and `X-Webhook-Signature: t=...,v1=...`. Deliveries retry at most five times: immediate, +5s, +30s, +5m, +30m, then become exhausted.

## Run

```bash
cargo fmt --check
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
# Optional: runs Postgres-gated repository coverage; skips when DATABASE_URL is unset.
DATABASE_URL=postgres://dodo:dodo@localhost:5432/dodo cargo test --test postgres
cargo run
```

Docker Compose starts Postgres, the API, and the mock PSP. Set `BOOTSTRAP_API_KEY=prefix_secret` before `docker compose up -d --build`, then send it as `X-API-Key`. The server listens on port 3000 (`PORT` overrides it). Do not use `docker compose down -v` for routine cleanup.

Amounts are integer minor units (`unit_amount_cents` for line-item prices). This is not PCI compliant, horizontally scalable, or suitable for real funds without production persistence, provider, security, reconciliation, and operations review.
