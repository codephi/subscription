CREATE TABLE workspace_projections (
  workspace_id uuid PRIMARY KEY,
  operational_status text NOT NULL
    CHECK (operational_status IN ('CREATED', 'ACTIVE', 'BLOCKED', 'TERMINATED')),
  external_sequence bigint NOT NULL CHECK (external_sequence >= 1),
  external_occurred_at timestamptz NOT NULL,
  last_event_id uuid NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_workspace_projections_updated_at
BEFORE UPDATE ON workspace_projections
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE integration_inbox (
  event_id uuid PRIMARY KEY,
  workspace_id uuid NOT NULL,
  event_type text NOT NULL,
  schema_version integer NOT NULL CHECK (schema_version > 0),
  aggregate_id uuid NOT NULL,
  external_sequence bigint NOT NULL CHECK (external_sequence >= 1),
  occurred_at timestamptz NOT NULL,
  correlation_id uuid NOT NULL,
  causation_id uuid,
  payload jsonb NOT NULL,
  processing_status text NOT NULL
    CHECK (processing_status IN ('RECEIVED', 'PROCESSED', 'IGNORED', 'QUARANTINED')),
  received_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz
);

CREATE INDEX idx_integration_inbox_workspace_sequence
  ON integration_inbox (workspace_id, external_sequence);

CREATE TABLE integration_inbox_quarantine (
  event_id uuid PRIMARY KEY REFERENCES integration_inbox(event_id) ON DELETE RESTRICT,
  reason_code text NOT NULL,
  reason_detail text NOT NULL,
  quarantined_at timestamptz NOT NULL DEFAULT now(),
  replayed_at timestamptz
);

CREATE TABLE outbox_events (
  event_id uuid PRIMARY KEY,
  event_type text NOT NULL,
  aggregate_type text NOT NULL,
  aggregate_id uuid NOT NULL,
  aggregate_sequence bigint NOT NULL CHECK (aggregate_sequence >= 1),
  workspace_id uuid NOT NULL,
  correlation_id uuid NOT NULL,
  causation_id uuid,
  payload jsonb NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  available_at timestamptz NOT NULL DEFAULT now(),
  lease_owner uuid,
  lease_until timestamptz,
  delivery_attempts integer NOT NULL DEFAULT 0 CHECK (delivery_attempts >= 0),
  delivered_at timestamptz,
  dead_lettered_at timestamptz,
  last_error text,
  UNIQUE (aggregate_type, aggregate_id, aggregate_sequence, event_type)
);

CREATE INDEX idx_outbox_events_delivery
  ON outbox_events (available_at, occurred_at)
  WHERE delivered_at IS NULL AND dead_lettered_at IS NULL;

CREATE TABLE idempotency_records (
  workspace_id uuid NOT NULL,
  idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 255),
  operation_kind text NOT NULL,
  request_hash text NOT NULL,
  resource_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, idempotency_key)
);

CREATE TABLE transaction_reservations (
  workspace_id uuid NOT NULL,
  transaction_id text NOT NULL CHECK (length(transaction_id) BETWEEN 1 AND 255),
  operation_kind text NOT NULL,
  resource_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, transaction_id)
);

CREATE TABLE audit_events (
  audit_event_id uuid PRIMARY KEY,
  workspace_id uuid,
  actor_reference text,
  action text NOT NULL,
  resource_kind text NOT NULL,
  resource_id uuid,
  correlation_id uuid NOT NULL,
  details jsonb NOT NULL DEFAULT '{}'::jsonb,
  occurred_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_events_workspace_time
  ON audit_events (workspace_id, occurred_at DESC);
