# Implementation Review

## Completed

- Tenant-scoped PostgreSQL persistence and API-key authentication.
- Draft invoice creation with explicit `draft → open` finalization.
- Invoice states limited to `draft`, `open`, `paid`, `void`, and `uncollectible`.
- Payment-attempt states represented separately as `pending`, `succeeded`, and `failed`.
- Versioned SHA-256 payment fingerprints over invoice ID, token, amount, and uppercase currency; raw tokens are not persisted.
- Durable PostgreSQL idempotency claims before PSP invocation, tenant-scoped key uniqueness, same-request replay, and changed-fingerprint conflict handling.
- Persisted payment response status/body metadata for direct payment replay.
- Invoice-row locking and claim-before-PSP invoice payment flow without holding the row lock over PSP network I/O.
- Transactional `invoice.created`, `invoice.paid`, and `invoice.payment_failed` outbox rows.
- Asynchronous PostgreSQL webhook worker using `FOR UPDATE SKIP LOCKED`, leases, bounded retries, and exhaustion.
- Timestamped raw-body HMAC signing and verification using `timestamp + "." + raw_payload`, `X-Webhook-Timestamp`, `X-Webhook-Signature: sha256=<hex>`, and a five-minute freshness window.
- Mock PSP timeout/network/decline behavior with a two-second service deadline and unknown timeout outcomes preserved as pending.

## Important Design Decisions

- PostgreSQL is the durable coordination boundary; no Redis, Kafka, RabbitMQ, or other broker was introduced.
- The PSP is called only after a durable local claim commits. This prevents duplicate local initiation but cannot guarantee exactly-once remote charging across a crash after provider acceptance.
- Payment failures and unknown outcomes leave invoices open; only a confirmed success marks an invoice paid.
- Webhook delivery is asynchronous and does not delay the payment response.
- The in-memory store remains a development fallback for fast API tests.

## Known Limitations

- The mock PSP has no provider idempotency or reconciliation API, so a process crash after remote acceptance and before local finalization leaves an unknown external outcome.
- PostgreSQL integration tests skip only when `DATABASE_URL` is unset; when configured, connection and migration failures are fatal.
- The retry schedule is tested as deterministic delay values, while full delivery timing depends on the worker and destination availability.
- This take-home is not PCI compliant and lacks production secret management, observability, rate limiting, operational replay tooling, and compliance controls.

## Tests Added / Run

- API tests cover health, authentication, validation, direct payments, invoice concurrency, replay, and timeout/network behavior.
- PostgreSQL tests cover repository idempotency concurrency, tenant isolation, timestamped webhook signatures, transactional invoice/payment outbox rows, HTTP-level concurrent invoice payment claims, PSP call counting, and worker lease/exhaustion fields.
- Retry schedule unit coverage verifies immediate, 5-second, 30-second, 5-minute, and 30-minute delays.

## Final Verification

Passed locally:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
Docker Compose config validation
```

A fresh isolated Compose stack was built and started with alternate ports. PostgreSQL became healthy, the API started, the mock PSP started, `/health` returned `ok`, and the smoke flow completed customer creation, invoice creation, finalization, successful payment, and same-key replay.

Live PostgreSQL integration tests were also run against an isolated PostgreSQL container and passed.
