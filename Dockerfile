# syntax=docker/dockerfile:1
FROM rust:1.88-bookworm AS builder
WORKDIR /app

# Cache dependency compilation when Cargo manifests are unchanged.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && printf 'fn main() {}\n' > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY . .
RUN cargo build --release --bins

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home dodo
WORKDIR /app
COPY --from=builder /app/target/release/dodo /usr/local/bin/dodo
COPY --from=builder /app/target/release/mock-psp /usr/local/bin/mock-psp
USER dodo
ENV PORT=3000
EXPOSE 3000
CMD ["dodo"]
