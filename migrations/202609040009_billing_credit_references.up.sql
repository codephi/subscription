CREATE TABLE billing_credit_grant_references (
  billing_credit_grant_reference_id uuid PRIMARY KEY,
  customer_wallet_entry_id uuid NOT NULL UNIQUE
    REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  collection_request_id uuid NOT NULL UNIQUE
    REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  billing_payment_id uuid NOT NULL UNIQUE
    REFERENCES billing_payments(billing_payment_id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_billing_credit_grant_references_append_only
BEFORE UPDATE OR DELETE ON billing_credit_grant_references
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
