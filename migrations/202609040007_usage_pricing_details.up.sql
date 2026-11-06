ALTER TABLE item_wallets
  ADD COLUMN pending_tier_position integer,
  ADD CONSTRAINT fk_item_wallet_last_usage
    FOREIGN KEY (last_usage_event_id) REFERENCES usage_events(usage_event_id) ON DELETE RESTRICT,
  ADD CONSTRAINT ck_item_wallet_pending_tier
    CHECK (
      (pending_item_units=0 AND pending_tier_position IS NULL)
      OR (pending_item_units>0 AND (pending_tier_position IS NULL OR pending_tier_position>=0))
    );

ALTER TABLE billing_blocks
  ADD COLUMN tier_position integer,
  ADD COLUMN cycle_start timestamptz,
  ADD COLUMN cycle_end timestamptz,
  ADD COLUMN accumulation_anchor_at timestamptz,
  ADD COLUMN accumulation_recurrence_rule text,
  ADD CONSTRAINT ck_billing_block_cycle_snapshot CHECK (
    (cycle_key='lifetime' AND cycle_start IS NULL AND cycle_end IS NULL
      AND accumulation_anchor_at IS NULL AND accumulation_recurrence_rule IS NULL)
    OR
    (cycle_key<>'lifetime' AND cycle_start IS NOT NULL AND cycle_end IS NOT NULL
      AND accumulation_anchor_at IS NOT NULL AND accumulation_recurrence_rule IS NOT NULL)
  );

ALTER TABLE pricing_accumulators
  ADD COLUMN created_at timestamptz NOT NULL DEFAULT now(),
  ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
  ADD CONSTRAINT ck_pricing_accumulator_totals CHECK (
    accumulated_converted_item_units>=0 AND converted_blocks>=0 AND version>=1
  );

ALTER TABLE usage_events ADD CONSTRAINT ck_usage_event_totals CHECK (
  pending_item_units_before>=0 AND converted_item_units>=0 AND converted_blocks>=0
  AND pending_item_units_after>=0 AND debited_credit_units>=0
  AND unit_offset_start>=0 AND unit_offset_end>unit_offset_start
);

ALTER TABLE item_wallet_entries ADD CONSTRAINT ck_item_wallet_entry_totals CHECK (
  total_received_item_units_before>=0 AND converted_item_units>=0 AND converted_blocks>=0
  AND pending_item_units_after>=0 AND emitted_debited_credit_units>=0
  AND unit_offset_start>=0 AND unit_offset_end>unit_offset_start
);

ALTER TABLE billing_blocks ADD CONSTRAINT ck_billing_block_totals CHECK (
  global_block_sequence>=1 AND price_block_ordinal>=1 AND accumulated_units_before>=0
  AND unit_offset_start>=0 AND unit_offset_end>unit_offset_start
);

CREATE INDEX idx_item_wallet_entries_cursor
  ON item_wallet_entries (item_wallet_id, total_received_item_units_after DESC);

CREATE INDEX idx_usage_events_transaction
  ON usage_events (customer_id, transaction_id);

ALTER TABLE wallet_transaction_references
  ADD COLUMN usage_event_id uuid REFERENCES usage_events(usage_event_id) ON DELETE RESTRICT,
  ADD COLUMN debit_id uuid REFERENCES debits(debit_id) ON DELETE RESTRICT,
  ADD COLUMN product_id uuid REFERENCES products(product_id) ON DELETE RESTRICT,
  ADD COLUMN item_id uuid REFERENCES items(item_id) ON DELETE RESTRICT,
  ADD COLUMN item_wallet_id uuid REFERENCES item_wallets(wallet_id) ON DELETE RESTRICT;

ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_check;
ALTER TABLE wallet_transaction_references ADD CHECK (
  (reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND credit_lot_id IS NULL
    AND external_reference IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL
    AND plan_version_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='CREDIT_LOT' AND credit_lot_id IS NOT NULL AND direct_credit_id IS NULL
    AND external_reference IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL
    AND plan_version_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='EXTERNAL' AND external_reference IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL
    AND plan_version_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='CUSTOMER_PLAN' AND customer_plan_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_cycle_id IS NULL
    AND plan_version_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND plan_version_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='PLAN_VERSION' AND plan_version_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND usage_event_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='USAGE_EVENT' AND usage_event_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL AND debit_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='DEBIT' AND debit_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL AND usage_event_id IS NULL
    AND product_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='PRODUCT' AND product_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL AND usage_event_id IS NULL
    AND debit_id IS NULL AND item_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='ITEM' AND item_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL AND usage_event_id IS NULL
    AND debit_id IS NULL AND product_id IS NULL AND item_wallet_id IS NULL)
  OR (reference_kind='ITEM_WALLET' AND item_wallet_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL
    AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL AND usage_event_id IS NULL
    AND debit_id IS NULL AND product_id IS NULL AND item_id IS NULL)
);
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references ADD CHECK (reference_kind IN (
  'DIRECT_CREDIT','CREDIT_LOT','EXTERNAL','CUSTOMER_PLAN','CUSTOMER_PLAN_CYCLE','PLAN_VERSION',
  'USAGE_EVENT','DEBIT','PRODUCT','ITEM','ITEM_WALLET'
));
