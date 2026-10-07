DROP TABLE billing_integration_customer_operations;
DROP INDEX uq_billing_connection_provider_environment;
DROP INDEX uq_billing_connection_legacy_scope;
ALTER TABLE billing_connections
  ADD CONSTRAINT billing_connections_scope_unique
    UNIQUE (account_id,provider,external_account_reference);
ALTER TABLE billing_connections
  DROP COLUMN configuration_version,
  DROP COLUMN provider_customer_reference,
  DROP COLUMN provider_account_reference,
  DROP COLUMN environment,
  DROP CONSTRAINT billing_connections_status_check,
  ADD CONSTRAINT billing_connections_status_check
    CHECK (status IN ('ACTIVE','BLOCKED','REVOKED'));
