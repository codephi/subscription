CREATE TABLE provider_managed_subscriptions (
  provider_managed_subscription_id uuid PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  billing_connection_id uuid NOT NULL REFERENCES billing_connections(billing_connection_id) ON DELETE RESTRICT,
  payment_method_binding_id uuid NOT NULL REFERENCES payment_method_bindings(payment_method_binding_id) ON DELETE RESTRICT,
  provider text NOT NULL,
  provider_subscription_reference text NOT NULL,
  provider_customer_reference text NOT NULL,
  status text NOT NULL CHECK (status IN ('INCOMPLETE','ACTIVE','PAST_DUE','CANCELED','UNPAID')),
  current_period_start timestamptz,
  current_period_end timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider,provider_subscription_reference),
  UNIQUE (customer_plan_id)
);

CREATE INDEX ix_provider_managed_subscriptions_due
  ON provider_managed_subscriptions(current_period_end)
  WHERE status='ACTIVE';
