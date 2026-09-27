CREATE TABLE billing_checkouts (
  checkout_id uuid PRIMARY KEY,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  checkout_kind text NOT NULL CHECK (checkout_kind IN ('INITIAL','ON_DEMAND')),
  on_demand_plan_id uuid REFERENCES on_demand_plans(on_demand_plan_id) ON DELETE RESTRICT,
  transaction_id text NOT NULL CHECK (length(transaction_id) BETWEEN 1 AND 255),
  idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 255),
  request_sha256 text NOT NULL CHECK (length(request_sha256)=64),
  collection_request_id uuid UNIQUE REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  lease_expires_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((checkout_kind='ON_DEMAND')=(on_demand_plan_id IS NOT NULL)),
  UNIQUE (workspace_id,idempotency_key),
  UNIQUE (workspace_id,transaction_id)
);

CREATE INDEX ix_billing_checkouts_collection
  ON billing_checkouts(workspace_id,collection_request_id)
  WHERE collection_request_id IS NOT NULL;

CREATE TRIGGER trg_billing_checkouts_updated_at
BEFORE UPDATE ON billing_checkouts FOR EACH ROW EXECUTE FUNCTION set_updated_at();
