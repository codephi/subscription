ALTER TABLE collection_requests
  DROP CONSTRAINT IF EXISTS fk_collection_customer_plan_scope,
  DROP CONSTRAINT IF EXISTS fk_collection_payment_binding_scope;

ALTER TABLE payment_method_bindings
  DROP CONSTRAINT IF EXISTS fk_payment_binding_customer_plan_scope,
  DROP CONSTRAINT IF EXISTS fk_payment_binding_connection_scope,
  DROP CONSTRAINT IF EXISTS uq_payment_binding_scope;

ALTER TABLE customer_plans
  DROP CONSTRAINT IF EXISTS uq_customer_plan_customer;

ALTER TABLE billing_connections
  DROP CONSTRAINT IF EXISTS uq_billing_connection_workspace;
