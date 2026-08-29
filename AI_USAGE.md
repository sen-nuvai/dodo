# AI Usage

## Tools used

- **Claude Code with the LunaCode harness:** reviewed and incrementally modified the existing Rust/Axum/PostgreSQL payment service. I used it to inspect the existing payment flow and migrations, refine idempotency claims and invoice state transitions, harden the webhook outbox/worker, update the mock PSP integration, add tests, reconcile documentation, and run formatting, lint, test, and Docker Compose checks.
- **Shell tools (`cargo`, `docker-compose`, `curl`):** used to compile, lint, run tests, build containers, inspect service status, and exercise `/health` and the local stack.

AI was used for implementation assistance and review prompts; the resulting code and design decisions were checked against the assignment and validated with executable tests. No claim is made that the system is production-ready or PCI compliant.

## Three decisions made by the candidate

### 1. Keep a small in-memory fallback

- **AI suggestion:** replace the in-memory store immediately with a fully mandatory Postgres runtime and remove all fallback behavior.
- **Decision:** retain an explicit in-memory mode when `DATABASE_URL` is absent, while making Postgres the configured Docker runtime.
- **Why:** this keeps fast unit/API tests runnable without external services and makes the interview project easier to demonstrate, while still exercising durable behavior in Postgres mode. The limitation is documented rather than hidden.

### 2. Do not add a message broker

- **AI suggestion:** introduce Redis/Kafka/RabbitMQ for webhook delivery and payment work.
- **Decision:** use Postgres webhook delivery rows plus one `SKIP LOCKED` worker.
- **Why:** the assignment asks for a small service and explicitly rewards restraint. A transactional outbox demonstrates the required reliability properties without adding infrastructure that the candidate would need to defend in a short interview.

### 3. Treat PSP timeout as unknown, not failed

- **AI suggestion:** return a normal payment failure after the HTTP timeout so callers can retry.
- **Decision:** persist the attempt as `pending`, leave the invoice unresolved/open, and do not blindly retry. The mock PSP has no reconciliation API; a real provider integration would need reconciliation.
- **Why:** a timeout only proves that the client did not receive a result; it does not prove that the provider did not charge. Marking it failed and blindly retrying could double-charge. The design document explicitly discusses this crash/timeout window.

## Something AI got wrong and was corrected

The initial generated Dockerfile used Rust 1.85, but the resolved dependency graph required a newer compiler. Docker builds failed with errors such as `home@0.5.12 requires rustc 1.88` and ICU packages requiring Rust 1.88. I changed the builder image to Rust 1.88 and rebuilt the Compose stack successfully.

The first bootstrap implementation also used `ON CONFLICT(api_key_prefix)` even though an existing database only had a partial index, causing PostgreSQL error `42P10: there is no unique or exclusion constraint matching the ON CONFLICT specification`. I replaced that with an explicit lookup/update-or-insert flow and then verified startup against the existing database.

## Verification

The final verification commands were:

```text
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The current suite passes 9 API tests and 7 PostgreSQL integration tests when `DATABASE_URL` is configured. Docker Compose was rebuilt with the app, PostgreSQL, and mock PSP services; PostgreSQL reported healthy, the API returned `ok` from `/health`, and the public Loom demo link is included in `README.md`.
