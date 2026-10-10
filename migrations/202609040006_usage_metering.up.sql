ALTER TABLE item_wallets
  ADD COLUMN total_converted_blocks bigint NOT NULL DEFAULT 0 CHECK (total_converted_blocks >= 0),
  ADD COLUMN pending_price_version_id uuid REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  ADD COLUMN pending_unit_block_size bigint,
  ADD COLUMN pending_credit_units bigint,
  ADD COLUMN last_usage_event_id uuid,
  ADD CHECK (
    (pending_item_units=0 AND pending_price_version_id IS NULL AND pending_unit_block_size IS NULL AND pending_credit_units IS NULL)
    OR
    (pending_item_units>0 AND pending_price_version_id IS NOT NULL AND pending_unit_block_size>pending_item_units AND pending_credit_units>0)
  );

CREATE TABLE usage_events (
  usage_event_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  item_wallet_id uuid NOT NULL REFERENCES item_wallets(wallet_id) ON DELETE RESTRICT,
  transaction_id text NOT NULL,
  product_id uuid NOT NULL REFERENCES products(product_id) ON DELETE RESTRICT,
  item_id uuid NOT NULL REFERENCES items(item_id) ON DELETE RESTRICT,
  item_units bigint NOT NULL CHECK (item_units > 0),
  expected_price_version_id uuid REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  occurred_at timestamptz,
  accepted_at timestamptz NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata)='object'),
  pending_item_units_before bigint NOT NULL,
  converted_item_units bigint NOT NULL,
  converted_blocks bigint NOT NULL,
  pending_item_units_after bigint NOT NULL,
  unit_offset_start bigint NOT NULL,
  unit_offset_end bigint NOT NULL,
  debited_credit_units bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_id,transaction_id),
  CHECK (unit_offset_end=unit_offset_start+item_units)
);

CREATE TABLE debits (
  debit_id uuid PRIMARY KEY,
  usage_event_id uuid NOT NULL UNIQUE REFERENCES usage_events(usage_event_id) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
  customer_wallet_entry_id uuid NOT NULL UNIQUE REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  debited_credit_units bigint NOT NULL CHECK (debited_credit_units > 0),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE item_wallet_entries (
  item_wallet_entry_id uuid PRIMARY KEY,
  item_wallet_id uuid NOT NULL REFERENCES item_wallets(wallet_id) ON DELETE RESTRICT,
  usage_event_id uuid NOT NULL UNIQUE REFERENCES usage_events(usage_event_id) ON DELETE RESTRICT,
  transaction_id text NOT NULL,
  received_item_units bigint NOT NULL CHECK (received_item_units > 0),
  total_received_item_units_before bigint NOT NULL,
  total_received_item_units_after bigint NOT NULL,
  converted_item_units bigint NOT NULL,
  converted_blocks bigint NOT NULL,
  pending_item_units_after bigint NOT NULL,
  emitted_debited_credit_units bigint NOT NULL,
  unit_offset_start bigint NOT NULL,
  unit_offset_end bigint NOT NULL,
  debit_id uuid REFERENCES debits(debit_id) ON DELETE RESTRICT,
  customer_wallet_entry_id uuid REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  accepted_at timestamptz NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (item_wallet_id,transaction_id),
  CHECK ((debit_id IS NULL)=(customer_wallet_entry_id IS NULL)),
  CHECK (total_received_item_units_after=total_received_item_units_before+received_item_units)
);

CREATE TABLE pricing_accumulators (
  pricing_accumulator_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  item_id uuid NOT NULL REFERENCES items(item_id) ON DELETE RESTRICT,
  price_version_id uuid NOT NULL REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  cycle_key text NOT NULL,
  accumulated_converted_item_units bigint NOT NULL DEFAULT 0,
  converted_blocks bigint NOT NULL DEFAULT 0,
  version bigint NOT NULL DEFAULT 1,
  UNIQUE (customer_id,item_id,price_version_id,cycle_key)
);

CREATE TABLE billing_blocks (
  billing_block_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  item_wallet_id uuid NOT NULL REFERENCES item_wallets(wallet_id) ON DELETE RESTRICT,
  item_id uuid NOT NULL REFERENCES items(item_id) ON DELETE RESTRICT,
  global_block_sequence bigint NOT NULL,
  price_version_id uuid NOT NULL REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  price_block_ordinal bigint NOT NULL,
  cycle_key text NOT NULL,
  accumulated_units_before bigint NOT NULL,
  accumulated_units_after bigint NOT NULL,
  unit_block_size bigint NOT NULL CHECK (unit_block_size > 0),
  debited_credit_units bigint NOT NULL CHECK (debited_credit_units > 0),
  unit_offset_start bigint NOT NULL,
  unit_offset_end bigint NOT NULL,
  usage_event_id uuid NOT NULL REFERENCES usage_events(usage_event_id) ON DELETE RESTRICT,
  item_wallet_entry_id uuid NOT NULL REFERENCES item_wallet_entries(item_wallet_entry_id) ON DELETE RESTRICT,
  debit_id uuid NOT NULL REFERENCES debits(debit_id) ON DELETE RESTRICT,
  customer_wallet_entry_id uuid NOT NULL REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_id,item_id,global_block_sequence),
  UNIQUE (customer_id,item_id,price_version_id,cycle_key,price_block_ordinal),
  CHECK (unit_offset_end=unit_offset_start+unit_block_size),
  CHECK (accumulated_units_after=accumulated_units_before+unit_block_size)
);

CREATE TABLE credit_lot_allocations (
  credit_lot_allocation_id uuid PRIMARY KEY,
  debit_id uuid NOT NULL REFERENCES debits(debit_id) ON DELETE RESTRICT,
  credit_lot_id uuid NOT NULL REFERENCES credit_lots(credit_lot_id) ON DELETE RESTRICT,
  allocated_credit_units bigint NOT NULL CHECK (allocated_credit_units > 0),
  allocation_ordinal bigint NOT NULL CHECK (allocation_ordinal >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (debit_id,credit_lot_id),
  UNIQUE (debit_id,allocation_ordinal)
);

CREATE TRIGGER trg_usage_events_append_only BEFORE UPDATE OR DELETE ON usage_events
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_debits_append_only BEFORE UPDATE OR DELETE ON debits
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_item_wallet_entries_append_only BEFORE UPDATE OR DELETE ON item_wallet_entries
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_billing_blocks_append_only BEFORE UPDATE OR DELETE ON billing_blocks
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_credit_lot_allocations_append_only BEFORE UPDATE OR DELETE ON credit_lot_allocations
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
