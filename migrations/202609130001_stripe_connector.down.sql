DROP TABLE external_refund_observations;
DROP TABLE billing_plan_upgrade_contexts;

ALTER TABLE billing_connections
  DROP CONSTRAINT billing_connections_webhook_secret_reference_length,
  DROP COLUMN webhook_secret_reference;

ALTER TABLE customer_wallet_entries
  DROP CONSTRAINT customer_wallet_entries_entry_type_check;
ALTER TABLE customer_wallet_entries
  ADD CONSTRAINT customer_wallet_entries_entry_type_check CHECK (entry_type IN (
    'DEBIT','DIRECT_CREDIT','SUBSCRIPTION_CREDIT','CREDIT_EXPIRY_FORFEITURE',
    'VOUCHER_CREDIT','COMPENSATION'
  ));
