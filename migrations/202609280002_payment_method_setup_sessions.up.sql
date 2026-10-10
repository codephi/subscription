CREATE TABLE payment_method_setup_sessions (
  payment_method_setup_id uuid PRIMARY KEY,
  provider_setup_id text NOT NULL CHECK (length(provider_setup_id) BETWEEN 1 AND 255),
  billing_connection_id uuid NOT NULL,
  account_id uuid NOT NULL,
  customer_plan_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (billing_connection_id, account_id)
    REFERENCES billing_connections(billing_connection_id, account_id) ON DELETE RESTRICT,
  FOREIGN KEY (customer_plan_id, account_id)
    REFERENCES customer_plans(customer_plan_id, customer_id) ON DELETE RESTRICT,
  UNIQUE (billing_connection_id, provider_setup_id)
);

CREATE INDEX idx_payment_method_setup_sessions_account
  ON payment_method_setup_sessions(account_id, customer_plan_id, payment_method_setup_id);
