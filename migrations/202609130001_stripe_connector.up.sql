ALTER TABLE billing_connections
  ADD COLUMN webhook_secret_reference text;

UPDATE billing_connections
SET webhook_secret_reference=secret_reference
WHERE webhook_secret_reference IS NULL;

ALTER TABLE billing_connections
  ADD CONSTRAINT billing_connections_webhook_secret_reference_length
    CHECK (webhook_secret_reference IS NULL OR length(webhook_secret_reference) BETWEEN 1 AND 255);

ALTER TABLE customer_wallet_entries
  DROP CONSTRAINT customer_wallet_entries_entry_type_check;
ALTER TABLE customer_wallet_entries
  ADD CONSTRAINT customer_wallet_entries_entry_type_check CHECK (entry_type IN (
    'DEBIT','DIRECT_CREDIT','SUBSCRIPTION_CREDIT','ON_DEMAND_CREDIT',
    'CREDIT_EXPIRY_FORFEITURE','VOUCHER_CREDIT','COMPENSATION'
  ));

CREATE TABLE billing_plan_upgrade_contexts (
  collection_request_id uuid PRIMARY KEY
    REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  previous_plan_version_id uuid NOT NULL
    REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  actor_reference text NOT NULL CHECK (length(actor_reference) BETWEEN 1 AND 255),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_billing_plan_upgrade_contexts_append_only
BEFORE UPDATE OR DELETE ON billing_plan_upgrade_contexts
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

CREATE TABLE external_refund_observations (
  external_refund_observation_id uuid PRIMARY KEY,
  billing_connection_id uuid NOT NULL
    REFERENCES billing_connections(billing_connection_id) ON DELETE RESTRICT,
  account_id uuid NOT NULL REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  provider text NOT NULL,
  provider_event_id text NOT NULL,
  provider_payment_id text NOT NULL,
  amount_minor bigint NOT NULL CHECK (amount_minor > 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  payload_sha256 text NOT NULL CHECK (payload_sha256 ~ '^[0-9a-f]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider,provider_event_id)
);

CREATE TRIGGER trg_external_refund_observations_append_only
BEFORE UPDATE OR DELETE ON external_refund_observations
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
