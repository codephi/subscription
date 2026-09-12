ALTER TABLE billing_connections
  ADD CONSTRAINT uq_billing_connection_workspace
  UNIQUE (billing_connection_id,workspace_id);

ALTER TABLE customer_plans
  ADD CONSTRAINT uq_customer_plan_customer
  UNIQUE (customer_plan_id,customer_id);

ALTER TABLE payment_method_bindings
  ADD CONSTRAINT uq_payment_binding_scope
  UNIQUE (payment_method_binding_id,workspace_id,customer_id),
  ADD CONSTRAINT fk_payment_binding_connection_scope
  FOREIGN KEY (billing_connection_id,workspace_id)
  REFERENCES billing_connections(billing_connection_id,workspace_id) ON DELETE RESTRICT,
  ADD CONSTRAINT fk_payment_binding_customer_plan_scope
  FOREIGN KEY (customer_plan_id,customer_id)
  REFERENCES customer_plans(customer_plan_id,customer_id) ON DELETE RESTRICT;

ALTER TABLE collection_requests
  ADD CONSTRAINT fk_collection_payment_binding_scope
  FOREIGN KEY (payment_method_binding_id,workspace_id,customer_id)
  REFERENCES payment_method_bindings(payment_method_binding_id,workspace_id,customer_id) ON DELETE RESTRICT,
  ADD CONSTRAINT fk_collection_customer_plan_scope
  FOREIGN KEY (customer_plan_id,customer_id)
  REFERENCES customer_plans(customer_plan_id,customer_id) ON DELETE RESTRICT;
