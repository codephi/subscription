# subscription

## Painel administrativo

O painel interno fica em [`admin-ui/`](admin-ui/). Ele mostra indicadores e
evidências de Billing, catálogo, auditoria e detalhes de workspaces, planos,
carteira, créditos e consumo. Ele permite criar workspaces em estado `CREATED`,
com autoria registrada na auditoria, além de formulários para ofertas e ações operacionais.
Foi criado para acesso por rede local/VPN e ainda não tem login; mantenha também
a API restrita a essa rede.

Para executar localmente com a API na porta 3000:

```sh
cp .env.example .env
cargo run
cd admin-ui
npm ci
npm run dev
```

O Vite encaminha `/v1` para `http://127.0.0.1:3000`. Para outra origem, configure
`VITE_API_BASE_URL` no frontend e `APP_CORS_ALLOW_ORIGINS` na API. Nunca coloque
segredos Stripe em variáveis `VITE_`. Rode `npm run api:types` em `admin-ui/`
após mudar contratos HTTP; o comando exporta `/openapi.json` para o snapshot e
regenera `src/api/generated.ts`. Verifique com `npm test`, `npm run test:e2e`
e `npm run build`. O teste ponta a ponta usa Playwright com respostas de API
simuladas; na primeira execução, rode `npx playwright install chromium`.

As consultas novas incluem `/v1/admin/billing/records/{kind}` e detalhe,
`/v1/admin/catalog/{kind}` e `/v1/admin/audit-events`, paginadas por UUID.
Filtros de Billing aceitam workspace, estado, cobrança e correlação. Operações
de crédito e transição preservam a chave de idempotência no navegador até a
resolução da tentativa. O painel usa apenas referências de ator digitadas pelo
operador; isso não representa autenticação.

O cadastro de produtos conecta produto, itens e preços sem exigir cópia de IDs.
O caminho comum pede nome, unidade de consumo e créditos por cobrança; unidade,
quantidade por cobrança e vigência começam com padrões úteis, e opções como
faixas, hierarquia e escala ficam em configurações avançadas. Os cadastros
avulsos de itens e preços selecionam produto e item pelo nome. É possível salvar
preços como rascunho e publicar depois, ou revisar e publicar o conjunto,
ativando itens e produto em sequência. A sessão guarda o progresso confirmado
para retomar falhas conhecidas; criações sem resposta confirmada exigem conferir
o catálogo antes de uma nova tentativa.

O [plano do frontend](docs/plano-frontend-administrativo.md) registra o escopo
entregue e os limites operacionais atuais. Para o inventário detalhado de telas,
ações, contratos, arquitetura, configuração e testes, consulte a
[documentação funcional do frontend](docs/frontend-administrativo.md).

O [modelo de dados da API](docs/modelo-de-dados.md) documenta as tabelas do
PostgreSQL e seus relacionamentos, organizados por domínio.

Para integrar um SaaS de infraestrutura, consulte os guias de
[créditos por execução](docs/integracao-saas/credito.md),
[assinatura recorrente](docs/integracao-saas/assinatura.md) e o
[plano para operar tudo pelo admin-ui](docs/integracao-saas/admin-ui-futuro.md).
O [índice da integração](docs/integracao-saas/README.md) reúne o mapeamento de
identidades, pré-requisitos e bloqueios atuais antes de cobrar clientes reais.

Subscription API built with Axum, PostgreSQL, and OpenTelemetry. It exposes:

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
- `/v1/workspaces/{workspace_id}/usage-events` for atomic metered usage,
  versioned unit/tier pricing, pending blocks, and strict credit debits.
- `/v1/workspaces/{workspace_id}/products/{product_id}/eligibility` and nested
  item-wallet meter, statement, and pricing-accumulator routes for usage reads.
- `/v1/admin/workspaces/{workspace_id}/items/{item_id}/usage/reconcile` for
  non-destructive item ledger reconciliation.
- `/v1/workspaces/{workspace_id}/billing-connections` and nested capabilities
  and setup-session routes for legacy environment-referenced connections.
- `/v1/admin/integrations/providers` and
  `/v1/admin/workspaces/{workspace_id}/integrations` for provider discovery,
  encrypted Stripe credential setup, rotation and connection tests.
- `/v1/workspaces/{workspace_id}/payment-method-bindings` for tokenized cards.
- customer-plan collection, OnDemand, regularization and paid-upgrade flows.
- `/v1/billing/webhooks/{connection_id}` for exact-body Stripe signature
  validation and idempotent payment convergence.
- `/v1/admin/billing/operations` and the unmatched-payment queue for monitoring.

The provider-neutral Billing foundation defines connector capabilities and the
normalized collection state machine. Stripe is the first adapter. Collection attempts
are persisted before external I/O, use a stable provider idempotency key, and
preserve uncertain outcomes without an automatic retry.

Scheduled collections are not dispatched before `scheduled_at`. Outbox delivery
preserves aggregate sequence across leases, retries and dead letters: a dead-lettered
predecessor blocks later sequences until it is replayed and successfully delivered.
Expired credit lots never fund usage; insufficient eligible credits return
`409 insufficient_credit` and roll back every related write.

Accounts event IDs are immutable identities: redelivery of the original envelope
is deduplicated; a changed envelope with the same ID returns
`409 workspace_event_identity_conflict` without changing the inbox or projection.
Foundation tests inject a deferred PostgreSQL failure at commit and verify both
complete rollback and a single persisted effect after retry.

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
   - Stripe API and webhook secrets in environment variables selected by each
     BillingConnection's `env://VARIABLE` references.
4. Run:
   - `cargo run --all-features`

The default application address is `http://127.0.0.1:8080`. Open
`http://127.0.0.1:8080/docs` for Swagger UI, use `/openapi.json` for the raw
contract, and `/health` for the liveness check. Values copied from
`.env.example` override the host and port (that example uses `0.0.0.0:3000`).

Migrations are managed by SQLx and executed on startup from `migrations/`.
Swagger UI is available at `/docs` with the generated OpenAPI contract. Every
new HTTP route is included in `/openapi.json`, assigned to one domain category,
and covered by a contract check.

API logs are newline-delimited JSON on stdout, following
[Elastic Common Schema (ECS) 9.5](https://www.elastic.co/docs/reference/ecs).
Every event includes `@timestamp` (UTC), `ecs.version`, `log.level`, `log.logger`,
`message`, and `service.name`/`service.version`. The service identity uses the
OpenTelemetry resource (`OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES`),
falling back to the Cargo package name and version. Set
`deployment.environment.name` (or the legacy `deployment.environment`) in
`OTEL_RESOURCE_ATTRIBUTES` to populate `service.environment`.

HTTP completion logs include `http.request.method`, `url.path`,
`http.response.status_code`, `event.duration` in nanoseconds, and `event.outcome`
(`failure` for 5xx; `success` otherwise, from the server's perspective). Duration
measures response creation, excluding streaming body delivery. These logs cover
health checks, missing routes, body-limit rejections, CORS preflights, and MCP
when enabled. Query strings, request bodies, authorization, and cookie headers
are omitted from access logs.
When an OpenTelemetry span exists, `trace.id` and `span.id` correlate the log
with its trace. Existing `error`/`error_code` fields map to
`error.message`/`error.code`; other contextual fields appear under `labels`.

`RUST_LOG` controls verbosity (default `info`), for example
`RUST_LOG=subscription=debug,info`. `OTEL_ENABLED=false` disables trace export
and trace middleware while retaining the same ECS logs and HTTP completion
events. No collector is required to consume stdout logs.

```json
{"@timestamp":"2026-09-27T12:00:00.000Z","ecs":{"version":"9.5.0"},"log":{"level":"info","logger":"subscription::routes::access_log"},"service":{"name":"subscription","version":"0.1.0"},"message":"HTTP request completed","event":{"kind":"event","action":"http_request","duration":1200000,"outcome":"success"},"http":{"request":{"method":"GET"},"response":{"status_code":200}},"url":{"path":"/health"}}
```

HTTP OpenTelemetry export (`http/protobuf` or `http/json`) uses the blocking
Reqwest client on the batch processor's dedicated thread. The client is selected
explicitly because the SDK otherwise prefers the async client, which needs a
Tokio runtime and can crash batch export, followed by repeated
`BatchSpanProcessor.OnEnd.AfterShutdown` warnings and lost spans. The optional
`OTEL_USE_SIMPLE_EXPORTER=true` mode retains its async HTTP client for exports
inside Tokio. Both modes honor `OTEL_EXPORTER_OTLP_TRACES_TIMEOUT`, falling back
to `OTEL_EXPORTER_OTLP_TIMEOUT` (milliseconds) and then the SDK default.

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

Each integration fixture owns a clean disposable PostgreSQL container. Independent
connections still exercise real database locks; fixtures do not share catalog or
wallet state. PostgreSQL cleanup runs outside individual test runtimes, including
after assertion failures. Docker must be running; the application database is not used.

The [test matrix](docs/test-matrix.md) preserves individual normative scenarios and
explicit coverage gaps. A green suite does **not** imply all V1 scenarios are covered.
Use `bash scripts/check-phase-gate.sh 8` to validate the paid MVP and every previous phase;
any open matrix row blocks the gate before the full quality checks. `--check-only`
prints coverage blockers without running Cargo. Phases 5–8 have passed their
coverage gates and complete the paid MVP boundary. Phases 9 and 10 remain
explicitly deferred in the matrix.

Catalog price validation rejects invalid tier boundaries and accumulation cycles
without a representable next UTC boundary with HTTP 422. Publication revalidates
legacy drafts, and extreme calendar intervals return errors instead of panicking.
Regression tests cover these cases, decimal credit ratios and quarterly boundaries;
the publication error is included in OpenAPI/Swagger. No new dependency or migration
is required for these validation changes.

Wallet regression tests also verify database hierarchy checks, rejection of updates
to every immutable wallet/lifecycle column, and scope reconciliation without lazy
creation. Removing and restoring item applicability preserves wallet identity,
pending units and the item statement. Run `bash scripts/check-phase-gate.sh 3`
to repeat the complete accumulated phase 3 gate, including migration round trips.

Wallet provisioning reconciliation rebuilds missing or divergent effective-state
projections from the immutable lifecycle history. Recovery from a recorded `ERROR`
appends a new transition with the next sequence and the reconciliation actor;
retries do not duplicate lifecycle events.

Administrative reconciliation rolls back incomplete materialization to a savepoint
before recording `ERROR`, the sanitized error detail, and the transactional
`workspace_provisioning.started` / `.failed` pair. Concurrent retries reuse that
outcome; a new scope records its own failure. Recovery emits `.started` / `.completed`.
If the failure record or its outbox cannot be persisted, the entire transaction
rolls back. Accounts ingestion retains its all-or-nothing transaction and delivery
retry behavior. No migration or dependency was added for this change.

The reconciliation route returns HTTP 200 with the persisted operational result,
including `status: ERROR`; failure to persist that result returns an error response.
Eligibility now shares the operational wallet guard: inactive workspaces or event
gaps return 409, and absent/current-scope-incomplete provisioning returns 503.
These contracts are documented in Swagger and covered by automated tests.

Catalog scope calculation and selection are serialized before reading applicable
items, so concurrent changes to different items cannot publish a stale snapshot.
Returning to a previous scope rechecks every expected wallet, specialization and
effective state. Historical provisioning counters alone never release credit,
eligibility or usage, including usage of another item that stayed active. Reads
report `ready: false` and `PROVISIONING` without `completed_at` until explicit
reconciliation restores the hierarchy. Reconciliation preserves wallet identities,
balances and history; rejected calls leave no reservations or financial effects.
These fixes require no migration or dependency change.

The phase 3 acceptance scope retains the explicit authentication deferral in
ADR 0003: Accounts events are signed, while common and administrative routes
remain open within a trusted network. Later phase gates preserve this decision.

Phase 4 closes the credit ledger gate with explicit commit-failure, backend-loss,
and lost-response tests. Duplicate keys/transactions return HTTP 409 with an
optional `error.existing_operation` containing the original workspace, operation
kind, resource ID and transaction ID. Clients can query that transaction after a
lost response; the conflict never returns original metadata or another workspace's
reference. A reservation without a completed resource omits this field.

Credit reconciliation now checks the sum of every signed entry, continuous
sequence and balance chain, and remaining unexpired lots. Expired residuals awaiting
forfeiture produce `consistent: false`. Reconciliation never repairs history.
Migration `202609100001_credit_lot_origin` protects lot identity, owner, granting
entry, original quantity and creation time against changes/deletion; effective
classification, expiry and remaining balance remain projections for later lifecycle
flows. The migration is reversible and preserves existing rows.

Run `bash scripts/check-phase-gate.sh 4` to validate phases 0–4 together. Regression
tests also prove one version increment and one ledger/lot/event per distinct
concurrent credit, rollback without leftover reservations, and outbox visibility
only after commit with no private request context. No new dependency is required.

Phase 5 uses migration `202609110001_plan_calendar_anchor` to persist
which historical cycle started the current calendar anchor and backfills existing
plans. Downgrade renewals now follow the new anchor without skipping periods;
reclassified on-demand credits survive subsequent allowance expirations.
Renewal checks offer revocation after acquiring the plan lock, so a concurrent
committed revocation cannot create another cycle. Calendar overflow returns a
validation error instead of panicking.

Product eligibility selects an effective entitlement for the requested product;
a newer pending or revoked subscription cannot hide another valid contract.
The response reports the selected contract's persisted renewal status.
Downgrades recheck admission against current evidence; CARD transitions await
the Billing evidence flow.

Cancellation immediately closes unactivated contracts (including recurring
plans) and releases their active slot. Activated recurring contracts retain the
current cycle and entitlement until period end; nonrecurring contracts close
immediately while preserving credit history and balance. Repeated cancellation
preserves the first result and cannot replace an administrative revocation.
Effective cancellation and scheduling persist one `customer_plan.canceled` or
`customer_plan.cancellation_scheduled` event with the contract change.

The server starts a durable scheduler for materialized recurring FREE cycles.
Migration `202609110002_subscription_calendar_jobs` backfills pending calendar work;
cycle creation and completion update the queue in the same database transaction.
Workers claim jobs with `SKIP LOCKED` and a 60-second lease. A restarted process
reclaims expired leases, while cycle locking prevents duplicate allowance or
expiry entries. Failed jobs persist an error code and retry after 30 seconds,
allowing other workspaces to proceed. Idle workers check again after one second.
Only internal calendar work is executed; paid collection and provider calls remain
in the Billing flow. The administrative cycle runner continues to close queue jobs
atomically when used for explicit catch-up.

Admission requirements can be published as immutable policy versions and linked
to `APPROVAL_REQUIRED` plans. Accounts submits signed, sequenced attestations for
verified email or identity with an opaque reference and expiry. Admission and
downgrade lock the workspace, evaluate the latest evidence, and persist the exact
evidence used by the decision in the same transaction. Expired, withdrawn,
out-of-sequence, cross-workspace, unsigned, or altered events cannot authorize a
contract. CARD evidence remains owned by the Billing setup flow.

Migration `202609110004_active_plan_slot_lifecycle` keeps the exclusive plan slot
aligned with commercial and renewal status. Published plan Products reject late
inserts as well as updates and deletes. Phase 5 is complete; run
`bash scripts/check-phase-gate.sh 5` for its accumulated validation.

Phase 6 completes deterministic usage metering for unit and tiered prices,
pending blocks, cycle accumulators, consolidated debits and immutable lot
allocations. Eligibility exposes `customer_plan_entitled`, `credit_sufficient`
and `access_allowed` separately and remains a nonbinding snapshot; every usage
command revalidates the plan, Product, materialized wallets and balance under
transactional locks. A Product outside the effective plan returns
`403 product_not_entitled` without usage or financial effects. Run
`bash scripts/check-phase-gate.sh 6` for the accumulated validation.

Phase 7 now has an idempotent provider-neutral confirmation path for initial
paid plans. A fake connector proves one provider attempt, webhook/response race
convergence and one atomic activation, cycle, entitlement and allowance grant.
The persisted Billing payment and CollectionRequest are linked to the resulting
credit entry through immutable typed references. Due collection requests expire
under row locks: initial purchases are canceled, renewals become `PAST_DUE` /
`RENEWAL_INACTIVE`, and OnDemand expiration leaves the active plan unchanged.
Late confirmations of terminal requests are recorded as rejected without cycle,
entitlement or credit effects. A definitive renewal failure also moves the plan
to `PAST_DUE` / `RENEWAL_INACTIVE` atomically while preserving available credit.
Composite database references also enforce that each connection, tokenized card,
CustomerPlan and collection belongs to the same workspace/customer. V1 attempts
accept only provider-tokenized cards, and uncertain outcomes remain pending
without automatic retry or provider polling. Confirmed paid renewals preserve the
calendar anchor, replace the active cycle and grant one allowance under concurrent
delivery. `POST /v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/renewal-regularizations`
creates an idempotent manual regularization for a past-due plan using the
Subscription payment window; confirmation resets the anchor and restores one
current cycle without retroactive credit. Confirmed provider payments without a matching local
collection are persisted as idempotent operational cases without financial or
subscription effects.

Phase 8 adds the first production adapter: Stripe SetupIntent and PaymentIntent,
environment-referenced legacy secrets, encrypted workspace credentials, signed webhooks, tokenized payment bindings and
operational views for unmatched payments and externally observed refunds. See
`docs/runbooks/billing-mvp.md` for uncertain payments, replay and credential
rotation procedures.

The admin's **Credenciais padrão** page stores encrypted Stripe credentials for
new workspaces. Each workspace receives its own integration record and Stripe
customer; workspace-level integrations can still be configured independently.
Default credentials are never returned by the API, and require
`BILLING_CREDENTIAL_ENCRYPTION_KEY` to be configured.

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

- [MVP readiness](docs/mvp-readiness.md): current scope, release blockers,
  deferred features, and the recommended path to a paid MVP.
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
