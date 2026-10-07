CREATE TABLE account_billing_configs (
  account_id uuid PRIMARY KEY REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  direct_credit_enabled boolean NOT NULL DEFAULT true,
  recurring_credit_enabled boolean NOT NULL DEFAULT true,
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_account_billing_configs_updated_at
BEFORE UPDATE ON account_billing_configs FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE direct_credits (
  direct_credit_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  transaction_id text NOT NULL CHECK (length(transaction_id) BETWEEN 1 AND 255),
  credit_units bigint NOT NULL CHECK (credit_units > 0),
  external_reference text CHECK (length(external_reference) BETWEEN 1 AND 500),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_id,transaction_id)
);

CREATE TABLE customer_wallet_entries (
  customer_wallet_entry_id uuid PRIMARY KEY,
  customer_wallet_id uuid NOT NULL REFERENCES customer_wallets(wallet_id) ON DELETE RESTRICT,
  customer_id uuid NOT NULL,
  entry_sequence bigint NOT NULL CHECK (entry_sequence >= 1),
  entry_type text NOT NULL CHECK (entry_type IN (
    'DEBIT','DIRECT_CREDIT','SUBSCRIPTION_CREDIT','CREDIT_EXPIRY_FORFEITURE',
    'VOUCHER_CREDIT','COMPENSATION'
  )),
  source_channel text NOT NULL,
  signed_credit_units bigint NOT NULL CHECK (signed_credit_units <> 0),
  balance_before_credit_units bigint NOT NULL,
  balance_after_credit_units bigint NOT NULL,
  transaction_id text,
  description text CHECK (length(description) BETWEEN 1 AND 500),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata)='object'),
  request_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_wallet_id,entry_sequence),
  UNIQUE (customer_id,transaction_id),
  CHECK (balance_after_credit_units = balance_before_credit_units + signed_credit_units)
);

CREATE INDEX idx_customer_wallet_entries_statement
  ON customer_wallet_entries(customer_wallet_id,entry_sequence DESC);

CREATE TABLE credit_lots (
  credit_lot_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  granting_entry_id uuid NOT NULL UNIQUE REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  source_kind text NOT NULL CHECK (source_kind IN ('DIRECT','SUBSCRIPTION','VOUCHER','COMPENSATION','ON_DEMAND')),
  original_credit_units bigint NOT NULL CHECK (original_credit_units > 0),
  remaining_credit_units bigint NOT NULL CHECK (remaining_credit_units >= 0),
  expires_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (remaining_credit_units <= original_credit_units)
);

CREATE TABLE wallet_transaction_references (
  wallet_transaction_reference_id uuid PRIMARY KEY,
  customer_wallet_entry_id uuid NOT NULL REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  reference_kind text NOT NULL CHECK (reference_kind IN ('DIRECT_CREDIT','CREDIT_LOT','EXTERNAL')),
  direct_credit_id uuid REFERENCES direct_credits(direct_credit_id) ON DELETE RESTRICT,
  credit_lot_id uuid REFERENCES credit_lots(credit_lot_id) ON DELETE RESTRICT,
  external_reference text,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (
    (reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND credit_lot_id IS NULL AND external_reference IS NULL)
    OR
    (reference_kind='CREDIT_LOT' AND direct_credit_id IS NULL AND credit_lot_id IS NOT NULL AND external_reference IS NULL)
    OR
    (reference_kind='EXTERNAL' AND direct_credit_id IS NULL AND credit_lot_id IS NULL AND external_reference IS NOT NULL)
  )
);

CREATE UNIQUE INDEX uq_wallet_reference_direct
  ON wallet_transaction_references(customer_wallet_entry_id,direct_credit_id)
  WHERE direct_credit_id IS NOT NULL;
CREATE UNIQUE INDEX uq_wallet_reference_lot
  ON wallet_transaction_references(customer_wallet_entry_id,credit_lot_id)
  WHERE credit_lot_id IS NOT NULL;
CREATE UNIQUE INDEX uq_wallet_reference_external
  ON wallet_transaction_references(customer_wallet_entry_id,external_reference)
  WHERE external_reference IS NOT NULL;

CREATE OR REPLACE FUNCTION reject_credit_history_mutation()
RETURNS TRIGGER AS $$
BEGIN
  RAISE EXCEPTION 'credit ledger history is append-only'
    USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_direct_credits_append_only
BEFORE UPDATE OR DELETE ON direct_credits
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_customer_wallet_entries_append_only
BEFORE UPDATE OR DELETE ON customer_wallet_entries
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_wallet_transaction_references_append_only
BEFORE UPDATE OR DELETE ON wallet_transaction_references
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

INSERT INTO account_billing_configs (account_id)
SELECT account_id FROM account_projections
ON CONFLICT DO NOTHING;
