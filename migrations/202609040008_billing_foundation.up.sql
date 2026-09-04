CREATE TABLE billing_connections (
  billing_connection_id uuid PRIMARY KEY,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  provider text NOT NULL CHECK (length(provider) BETWEEN 1 AND 50),
  external_account_reference text NOT NULL CHECK (length(external_account_reference) BETWEEN 1 AND 255),
  secret_reference text NOT NULL CHECK (length(secret_reference) BETWEEN 1 AND 255),
  capabilities text[] NOT NULL,
  status text NOT NULL CHECK (status IN ('ACTIVE','BLOCKED','REVOKED')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (workspace_id,provider,external_account_reference)
);

CREATE TRIGGER trg_billing_connections_updated_at
BEFORE UPDATE ON billing_connections FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE payment_method_bindings (
  payment_method_binding_id uuid PRIMARY KEY,
  billing_connection_id uuid NOT NULL REFERENCES billing_connections(billing_connection_id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  customer_id uuid NOT NULL,
  customer_plan_id uuid REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  payment_method text NOT NULL CHECK (payment_method='CARD'),
  provider_payment_method_reference text NOT NULL CHECK (length(provider_payment_method_reference) BETWEEN 1 AND 255),
  status text NOT NULL CHECK (status IN ('ACTIVE','REPLACED','DETACHED','INACTIVE','INVALID')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (workspace_id=customer_id),
  UNIQUE (billing_connection_id,provider_payment_method_reference)
);

CREATE TRIGGER trg_payment_method_bindings_updated_at
BEFORE UPDATE ON payment_method_bindings FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE collection_requests (
  collection_request_id uuid PRIMARY KEY,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  customer_id uuid NOT NULL,
  customer_plan_id uuid REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  plan_version_id uuid REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  on_demand_plan_id uuid REFERENCES on_demand_plans(on_demand_plan_id) ON DELETE RESTRICT,
  payment_method_binding_id uuid NOT NULL REFERENCES payment_method_bindings(payment_method_binding_id) ON DELETE RESTRICT,
  request_kind text NOT NULL CHECK (request_kind IN (
    'INITIAL','RENEWAL','RENEWAL_REGULARIZATION','PLAN_UPGRADE','ON_DEMAND'
  )),
  amount_minor bigint NOT NULL CHECK (amount_minor > 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  granted_credit_units bigint NOT NULL CHECK (granted_credit_units >= 0),
  status text NOT NULL CHECK (status IN (
    'SCHEDULED','COLLECTING','PENDING_PAYMENT','PAID','EXHAUSTED','EXPIRED','CANCELED','UNMATCHED'
  )),
  maximum_attempts integer NOT NULL DEFAULT 1 CHECK (maximum_attempts=1),
  attempts_started integer NOT NULL DEFAULT 0 CHECK (attempts_started BETWEEN 0 AND 1),
  transaction_id text NOT NULL CHECK (length(transaction_id) BETWEEN 1 AND 255),
  idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 255),
  correlation_id uuid NOT NULL,
  scheduled_at timestamptz NOT NULL,
  payment_expires_at timestamptz NOT NULL,
  terminal_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (workspace_id=customer_id),
  CHECK (payment_expires_at > scheduled_at),
  CHECK ((request_kind='ON_DEMAND')=(on_demand_plan_id IS NOT NULL)),
  UNIQUE (workspace_id,transaction_id),
  UNIQUE (workspace_id,idempotency_key)
);

CREATE TRIGGER trg_collection_requests_updated_at
BEFORE UPDATE ON collection_requests FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE collection_attempts (
  collection_attempt_id uuid PRIMARY KEY,
  collection_request_id uuid NOT NULL REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  attempt_number integer NOT NULL CHECK (attempt_number=1),
  connector text NOT NULL CHECK (length(connector) BETWEEN 1 AND 50),
  payment_method text NOT NULL CHECK (payment_method='CARD'),
  provider_idempotency_key text NOT NULL CHECK (length(provider_idempotency_key) BETWEEN 1 AND 255),
  status text NOT NULL CHECK (status IN ('SCHEDULED','STARTED','PENDING','REQUIRES_ACTION','SUCCEEDED','FAILED','UNCERTAIN')),
  scheduled_at timestamptz NOT NULL,
  started_at timestamptz,
  finished_at timestamptz,
  failure_code text,
  next_action_url text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (collection_request_id,attempt_number),
  UNIQUE (connector,provider_idempotency_key)
);

CREATE TRIGGER trg_collection_attempts_updated_at
BEFORE UPDATE ON collection_attempts FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE billing_payments (
  billing_payment_id uuid PRIMARY KEY,
  collection_request_id uuid NOT NULL REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  collection_attempt_id uuid NOT NULL UNIQUE REFERENCES collection_attempts(collection_attempt_id) ON DELETE RESTRICT,
  provider text NOT NULL CHECK (length(provider) BETWEEN 1 AND 50),
  provider_payment_id text CHECK (provider_payment_id IS NULL OR length(provider_payment_id) BETWEEN 1 AND 255),
  state text NOT NULL CHECK (state IN ('PENDING','REQUIRES_ACTION','CONFIRMED','FAILED','CANCELED')),
  amount_minor bigint NOT NULL CHECK (amount_minor > 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  confirmed_at timestamptz,
  failure_code text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider,provider_payment_id)
);

CREATE TRIGGER trg_billing_payments_updated_at
BEFORE UPDATE ON billing_payments FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE billing_webhook_inbox (
  billing_webhook_inbox_id uuid PRIMARY KEY,
  provider text NOT NULL CHECK (length(provider) BETWEEN 1 AND 50),
  provider_event_id text NOT NULL CHECK (length(provider_event_id) BETWEEN 1 AND 255),
  event_type text NOT NULL CHECK (length(event_type) BETWEEN 1 AND 100),
  payload_sha256 text NOT NULL CHECK (payload_sha256 ~ '^[0-9a-f]{64}$'),
  payload jsonb NOT NULL,
  received_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz,
  result text CHECK (result IS NULL OR result IN ('APPLIED','UNMATCHED','REJECTED','DUPLICATE')),
  failure_code text,
  UNIQUE (provider,provider_event_id)
);

CREATE INDEX idx_collection_requests_scheduled
  ON collection_requests (scheduled_at,collection_request_id)
  WHERE status='SCHEDULED';

CREATE INDEX idx_collection_requests_expiration
  ON collection_requests (payment_expires_at,collection_request_id)
  WHERE status IN ('SCHEDULED','COLLECTING','PENDING_PAYMENT');
