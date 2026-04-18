# scout-rust

A full Rust rewrite of the `scout-core` Go backend. Functionally equivalent
surface area: same HTTP routes, same request/response JSON, same database
schema, same provider contracts.

## Stack

- **Web**: axum + tower-http
- **DB**: sqlx (Postgres, uuid, chrono, json, migrate)
- **Object store**: aws-sdk-s3 (pointed at Supabase Storage's S3-compatible endpoint)
- **Async runtime**: tokio
- **HTTP client**: reqwest (rustls-tls)
- **JWT auth**: jsonwebtoken
- **Stripe**: direct HTTPS calls via reqwest (no SDK)

## Running

```
export DATABASE_URL=postgres://scout:scout@localhost:5432/scout?sslmode=disable
export SUPABASE_URL=http://127.0.0.1:54321
export SUPABASE_SERVICE_KEY=...
export SUPABASE_JWT_SECRET=...
export TEXT_PROVIDER=mock
export VIDEO_PROVIDER=mock
cargo run --release
```

Migrations in `./migrations` are applied automatically at startup.

## Layout

```
src/
├── main.rs               # Wiring & startup
├── config.rs             # Environment config
├── logging.rs            # Tracing setup
├── error.rs              # AppError / IntoResponse
├── db.rs                 # Postgres pool + migrator
├── objectstore.rs        # S3/Supabase Storage wrapper
├── prompts.rs            # Prompt builders (inline, no Go template runtime)
├── flash.rs              # Flash music selector
├── media.rs              # Image resizing helpers
├── billing.rs            # Stripe integration via HTTP
├── app_state.rs          # Shared AppState
├── middleware.rs         # Auth + request ID
├── router.rs             # Axum router
├── models.rs             # Domain structs
├── providers/            # claude, ollama, runway, flux, mock
├── repos/                # postgres repositories
├── jobs/                 # scout_jobs-backed queue + worker dispatcher
└── handlers/             # HTTP handlers (onboarding, decisions, flash, ...)
migrations/               # Copied verbatim from scout-core + 100_scout_jobs
```

## Compatibility notes

- The frontend endpoints, JSON shapes, and database schema are byte-identical.
- The original River queue has been replaced with a lightweight Postgres-polling
  queue named `scout_jobs` because there is no first-party Rust client for
  River. Behaviour (retries, claim under SKIP LOCKED, backoff) matches.
- Prompts are rendered programmatically in `src/prompts.rs` — no template
  interpreter dependency.
