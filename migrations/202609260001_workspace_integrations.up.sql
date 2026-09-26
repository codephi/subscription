ALTER TABLE billing_connections
  DROP CONSTRAINT billing_connections_status_check,
  ADD CONSTRAINT billing_connections_status_check
    CHECK (status IN ('ACTIVE','BLOCKED','REVOKED','PENDING_SETUP')),
  ADD COLUMN environment text CHECK (environment IN ('TEST','LIVE')),
  ADD COLUMN provider_account_reference text,
  ADD COLUMN provider_customer_reference text,
  ADD COLUMN configuration_version integer NOT NULL DEFAULT 1;

DO $$
DECLARE duplicate_scope_constraint text;
BEGIN
  SELECT conname INTO duplicate_scope_constraint FROM pg_constraint
  WHERE conrelid='billing_connections'::regclass AND contype='u'
    AND pg_get_constraintdef(oid)='UNIQUE (workspace_id, provider, external_account_reference)';
  IF duplicate_scope_constraint IS NOT NULL THEN
    EXECUTE format('ALTER TABLE billing_connections DROP CONSTRAINT %I', duplicate_scope_constraint);
  END IF;
END $$;

CREATE UNIQUE INDEX uq_billing_connection_legacy_scope
  ON billing_connections(workspace_id,provider,external_account_reference)
  WHERE environment IS NULL;
CREATE UNIQUE INDEX uq_billing_connection_provider_environment
  ON billing_connections(workspace_id,provider,environment,provider_account_reference)
  WHERE environment IS NOT NULL;

CREATE TABLE billing_integration_customer_operations (
  billing_connection_id uuid PRIMARY KEY
    REFERENCES billing_connections(billing_connection_id) ON DELETE RESTRICT,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('STARTED','COMPLETE')),
  provider_customer_reference text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((status='STARTED' AND provider_customer_reference IS NULL)
      OR (status='COMPLETE' AND provider_customer_reference IS NOT NULL))
);

CREATE TRIGGER trg_billing_integration_customer_operations_updated_at
BEFORE UPDATE ON billing_integration_customer_operations FOR EACH ROW EXECUTE FUNCTION set_updated_at();
