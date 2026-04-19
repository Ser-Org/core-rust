# ---- Build stage ----
# 1.85+ required: some transitive deps (notably moxcms via the `image` crate)
# declare `edition2024`, which was only stabilized in Rust 1.85.
FROM rust:1.85-bookworm AS builder

WORKDIR /app

# Cache dependency compilation: build a dummy crate first so every third-party
# dep in Cargo.lock is compiled into target/. This layer stays valid until
# Cargo.toml or Cargo.lock changes. scout-core = lib + bin, so both shims
# are needed.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && echo "fn main() {}" > src/main.rs \
 && echo "" > src/lib.rs \
 && cargo build --release --bin scout-core

# Real sources. Migrations are copied too because db::migrate() runs them at
# startup and reads them from disk.
COPY src ./src
COPY migrations ./migrations

# Invalidate the dummy build of our own crate; third-party deps stay cached.
RUN touch src/main.rs src/lib.rs \
 && cargo build --release --bin scout-core

# ---- Runtime stage ----
FROM debian:bookworm-slim

# ca-certificates: outbound HTTPS (Supabase, Stripe, Runway, BFL, Claude).
# tzdata: chrono timezone lookups.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && apt-get clean

WORKDIR /app

COPY --from=builder /app/target/release/scout-core ./scout-core
COPY --from=builder /app/migrations ./migrations

# Railway injects PORT at runtime; Config::load() reads it in src/config.rs.
EXPOSE 8080

CMD ["./scout-core"]
