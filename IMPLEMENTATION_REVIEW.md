# Implementation Review

## Completed

- Added a draft-to-open invoice finalization route and aligned invoice/payment status vocabulary.
- Added versioned length-delimited SHA-256 payment fingerprints covering invoice, token, amount, and currency.
- Preserved tenant-scoped idempotency locking and prevented duplicate in-memory invoice charges.
- Added timestamped HMAC webhook verification over raw payload bytes, tenant/registration scoping, leases, and bounded asynchronous retries.
- Resolved duplicate migration numbering and documented the invoice/payment/webhook contracts.

## Important Design Decisions

- PostgreSQL is the durable coordination boundary; no Redis, queue broker, or external distributed system was added.
- PSP timeout/network outcomes remain pending/unknown rather than being treated as confirmed failure.
- Webhook delivery is asynchronous and uses a transactional database delivery row.

## Known Limitations

- The current PostgreSQL invoice-pay handler still requires further claim-before-PSP integration work before it can honestly guarantee one PSP initiation across instances.
- The mock PSP has no reconciliation API, so exactly-once external charging cannot be claimed across a process crash.
- PostgreSQL integration tests are skipped when `DATABASE_URL` is unset.
- Docker Compose verification may require overriding `PSP_PORT` if host port 4000 is occupied.

## Tests Added / Run

Existing API and repository tests remain green: 8 API tests and 4 PostgreSQL-gated tests. Formatting, Clippy with warnings denied, and `cargo test` passed locally. A live PostgreSQL and Docker Compose run must be recorded separately when available.

## Final Verification

- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: passed; PostgreSQL tests require `DATABASE_URL` for real database coverage.
- Docker Compose: not completed in this environment; port 4000 may require `PSP_PORT` override.
