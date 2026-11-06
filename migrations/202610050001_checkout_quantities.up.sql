ALTER TABLE collection_requests
  ADD COLUMN credit_quantity integer NOT NULL DEFAULT 1
    CHECK (credit_quantity BETWEEN 1 AND 10000);

ALTER TABLE payment_method_bindings DROP CONSTRAINT payment_method_bindings_status_check;
ALTER TABLE payment_method_bindings ADD CONSTRAINT payment_method_bindings_status_check
  CHECK (status IN ('ACTIVE','PENDING','REPLACED','DETACHED','INACTIVE','INVALID'));

ALTER TABLE billing_checkouts
  ADD COLUMN credit_quantity integer NOT NULL DEFAULT 1 CHECK (credit_quantity BETWEEN 1 AND 10000),
  ADD COLUMN success_url text,
  ADD COLUMN cancel_url text,
  ADD COLUMN target_plan_version_id uuid REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT;

ALTER TABLE billing_checkouts DROP CONSTRAINT billing_checkouts_checkout_kind_check;
ALTER TABLE billing_checkouts DROP CONSTRAINT billing_checkouts_check;
ALTER TABLE billing_checkouts ADD CONSTRAINT billing_checkouts_checkout_kind_check
  CHECK (checkout_kind IN ('INITIAL','ON_DEMAND','PLAN_UPGRADE'));
ALTER TABLE billing_checkouts ADD CONSTRAINT billing_checkouts_kind_terms_check CHECK (
  (checkout_kind='ON_DEMAND') = (on_demand_plan_id IS NOT NULL)
  AND (checkout_kind='PLAN_UPGRADE') = (target_plan_version_id IS NOT NULL)
);

CREATE TABLE billing_hosted_payment_sessions (
  checkout_id uuid PRIMARY KEY
    REFERENCES billing_checkouts(checkout_id) ON DELETE RESTRICT,
  collection_request_id uuid UNIQUE
    REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  billing_connection_id uuid NOT NULL
    REFERENCES billing_connections(billing_connection_id) ON DELETE RESTRICT,
  provider_session_id text UNIQUE,
  redirect_url text,
  payment_method_binding_id uuid NOT NULL
    REFERENCES payment_method_bindings(payment_method_binding_id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('CREATING','OPEN','COMPLETED','EXPIRED')),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((provider_session_id IS NULL) = (redirect_url IS NULL))
);

CREATE TRIGGER trg_billing_hosted_payment_sessions_updated_at
BEFORE UPDATE ON billing_hosted_payment_sessions
FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE INDEX ix_billing_hosted_payment_sessions_provider_session
  ON billing_hosted_payment_sessions(provider_session_id)
  WHERE provider_session_id IS NOT NULL;
CREATE UNIQUE INDEX ix_billing_hosted_payment_sessions_collection
  ON billing_hosted_payment_sessions(collection_request_id)
  WHERE collection_request_id IS NOT NULL;
