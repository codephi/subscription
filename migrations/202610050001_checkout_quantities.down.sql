DROP TABLE billing_hosted_payment_sessions;
ALTER TABLE billing_checkouts
  DROP CONSTRAINT billing_checkouts_kind_terms_check,
  DROP CONSTRAINT billing_checkouts_checkout_kind_check,
  ADD CONSTRAINT billing_checkouts_checkout_kind_check CHECK (checkout_kind IN ('INITIAL','ON_DEMAND')),
  ADD CONSTRAINT billing_checkouts_check CHECK ((checkout_kind='ON_DEMAND')=(on_demand_plan_id IS NOT NULL)),
  DROP COLUMN credit_quantity, DROP COLUMN success_url, DROP COLUMN cancel_url,
  DROP COLUMN target_plan_version_id;
ALTER TABLE collection_requests DROP COLUMN credit_quantity;
ALTER TABLE payment_method_bindings DROP CONSTRAINT payment_method_bindings_status_check;
ALTER TABLE payment_method_bindings ADD CONSTRAINT payment_method_bindings_status_check
  CHECK (status IN ('ACTIVE','REPLACED','DETACHED','INACTIVE','INVALID'));
