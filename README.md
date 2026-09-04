# subscription

Starter project for building APIs with Axum, PostgreSQL, and OpenTelemetry. It exposes:

- `/health` as a liveness endpoint that returns `200 OK` with a JSON status payload.
- `/echo` to reflect the incoming request across all HTTP verbs with tracing spans per method when OTEL is enabled.
- `/mcp` as an optional MCP Streamable HTTP endpoint for `health_check` and `echo_request` tools when the `mcp` Cargo feature is enabled.
- `/v1/internal/accounts/workspace-events` for signed, ordered workspace lifecycle events.
- `/v1/products`, `/v1/items/{id}`, and `/v1/price-versions/{id}` for the
  versioned billing catalog.
- `/v1/catalog-scope/current` for the immutable item/price snapshot used by
  wallet provisioning.
- `/v1/workspaces/{workspace_id}/wallets` and `/wallet-provisioning` for the
  materialized wallet hierarchy and readiness state.
- `/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile` for
  idempotent recovery after catalog scope changes.
- `/v1/workspaces/{workspace_id}/credits/direct` for strictly idempotent direct
  credit grants backed by an append-only ledger and credit lot.
- `/v1/workspaces/{workspace_id}/customer-wallet/statement` and
  `/customer-wallet/transactions/{transaction_id}` for cursor-based history.
- `/v1/workspaces/{workspace_id}/billing-config` for source-specific credit
  controls.
- `/v1/subscriptions`, `/v1/subscription-plans/{plan_id}`, and nested plan
  catalog routes for immutable commercial offers and on-demand credit offers.
- `/v1/workspaces/{workspace_id}/customer-plans` for idempotent plan admission,
  lookup, end-of-period cancellation, and audited free-plan downgrades.
- `/v1/admin/workspaces/{workspace_id}/customer-plans/{id}/revoke` for
  immediate, audited administrative revocation without deleting wallet history.
- `/v1/admin/subscription-cycles/run` for deterministic, concurrency-safe free
  plan cycle advancement.

## Template Bootstrap

After creating a new repository from this template, run:

- `./scripts/init-template.sh`

The script uses the current repository directory name as the new Cargo package/bin name, updates the main hardcoded references (`Cargo.toml`, Rust imports, README, `.env.example`, and `Dockerfile.artifact`), then runs `cargo build` and `cargo test`.

If you want to override the detected name, pass it explicitly:

- `./scripts/init-template.sh my-new-api`

## Getting Started

### Prerequisites

- Rust toolchain 1.94 or newer.
- PostgreSQL access. You can use the included Docker Compose if you want a local DB.

### Quick start

1. Start a database (optional example using Compose):
   - `docker compose up -d postgres jaeger`
   - If you are upgrading from an older Postgres image and see a volume layout error, recreate the Postgres volume once:
   - `docker compose down -v`
   - `docker compose up -d postgres jaeger`
2. Create the local environment file (already ignored by Git):
   - `cp .env.example .env`
   - Its database and OTLP endpoints match the ports exposed by Docker Compose.
3. Optionally set:
   - `APP_HOST` and `APP_PORT` (defaults: `127.0.0.1:8080`).
   - `APP_CORS_ALLOW_ORIGINS` (comma-separated or `*`) and `APP_BODY_LIMIT_BYTES`.
   - `OTEL_ENABLED=false` to disable OpenTelemetry export and HTTP tracing middleware while keeping structured logs.
   - `MCP_ENABLED=true` to expose the MCP endpoint when compiled with `--features mcp`.
   - `MCP_PATH=/mcp` to change the MCP path.
   - `MCP_ALLOWED_ORIGINS=*` to keep the MCP endpoint fully open, or provide a comma-separated allowlist if you want to restrict it.
   - `ACCOUNTS_WEBHOOK_SECRET` to verify Accounts event signatures.
   - `OUTBOUND_EVENT_WEBHOOK_URL` and `OUTBOUND_EVENT_WEBHOOK_SECRET` together to enable durable signed event delivery.
4. Run:
   - `cargo run`

Migrations are managed by SQLx and executed on startup from `migrations/`.
Swagger UI is available at `/docs` with the generated OpenAPI contract. Every
new HTTP route is included in `/openapi.json` and covered by a contract check.

Business and administrative routes intentionally have no general authentication
middleware in the current delivery. Accounts and outgoing integration events
still require HMAC signatures. Do not expose these routes outside a trusted
network until service authentication is implemented.

### MCP HTTP

This template can expose an MCP server over Streamable HTTP in stateless JSON mode.

- It is not compiled into the default build. Use `--features mcp` to include MCP support.
- It only mounts when both the `mcp` Cargo feature is enabled and `MCP_ENABLED=true`.
- The MCP endpoint is intentionally not included in `/openapi.json`.
- CORS is permissive by default for both the REST API and MCP. Set `APP_CORS_ALLOW_ORIGINS` or `MCP_ALLOWED_ORIGINS` only if you want to restrict them.

The first version exposes two tools:

- `health_check` returns the service status and version.
- `echo_request` mirrors `method`, `path`, `headers`, and `body` from the tool input.
- When MCP is enabled, startup logs include the MCP endpoint URL.

To test with the inspector:

1. Start the API with MCP enabled:
   - `MCP_ENABLED=true cargo run --features mcp`
2. Launch the inspector:
   - `npx @modelcontextprotocol/inspector`
3. Connect using Streamable HTTP:
   - `http://127.0.0.1:8080/mcp`

Current limitations:

- no resources or prompts
- no bearer auth
- no SSE/session mode; `GET /mcp` returns `405 Method Not Allowed`

### Artifact image

`Dockerfile.artifact` expects a prebuilt binary in `artifacts/<bin-name>/<arch>/` and accepts `BIN_NAME` as a build argument. Example:

`docker build -f Dockerfile.artifact --build-arg TARGETARCH=amd64 --build-arg BIN_NAME=subscription .`

### SQLx note

The SQLx query macros use the database schema at compile time. Make sure `DATABASE_URL` is set when building. If you prefer offline builds, run `cargo sqlx prepare` and set `SQLX_OFFLINE=true`.

### Testing

- Full quality gate: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo build --all-features && cargo test --all-features`
- Unit tests: `cargo test`
- MCP tests: `cargo test --features mcp mcp`
- Integration tests: `cargo test --test integration`
  - Requires Docker; tests spin up `pgvector/pgvector:pg18` and, unless `OTEL_ENABLED=false`, a Jaeger collector via testcontainers.

## Architecture

This template is organized around four main layers:

- `routes/`: transport and protocol adapters for HTTP and MCP
- `services/`: business rules and use-case orchestration
- `repositories/`: persistence and external integration adapters
- `dto/`: request/response contracts, validation, and data transformation structs

Preferred flow:

- `route -> dto -> service -> repository -> service -> dto -> route`

Guidelines:

- keep HTTP and MCP details inside `routes/`
- keep business decisions inside `services/`
- keep SQLx, queues, and external API clients inside `repositories/`
- keep payload contracts and transformation structs inside `dto/`
- let `AppState` carry concrete repositories instead of exposing raw driver clients when possible

## Implementation planning

- [Technical plan](docs/plano-tecnico-api-assinaturas-rust.md): normative domain,
  API, transaction, and billing decisions for the subscriptions service.
- [Implementation phases](docs/fases-implementacao.md): incremental delivery
  order, including the operational integration with the external Accounts and
  workspaces system.
- [Test matrix](docs/test-matrix.md): mandatory traceability and quality gate for
  every implementation phase.

## Project Layout

```
src/
  config.rs         # environment loading
  db.rs             # connection pool + migrations
  dto/              # request/response contracts and shared transport payloads
  repositories/     # DB, queue, cache, and external integration adapters
  routes/           # HTTP and MCP transport handlers plus wiring
  services/         # business rules and use-case orchestration
```

Adjust the repositories and services to fit your application, then expand the router with new modules as needed.
