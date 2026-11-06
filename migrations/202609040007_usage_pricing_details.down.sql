ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_check;
ALTER TABLE wallet_transaction_references
  DROP COLUMN item_wallet_id,
  DROP COLUMN item_id,
  DROP COLUMN product_id,
  DROP COLUMN debit_id,
  DROP COLUMN usage_event_id;
ALTER TABLE wallet_transaction_references ADD CHECK (
  (reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND credit_lot_id IS NULL
    AND external_reference IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL)
  OR (reference_kind='CREDIT_LOT' AND direct_credit_id IS NULL AND credit_lot_id IS NOT NULL
    AND external_reference IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL)
  OR (reference_kind='EXTERNAL' AND direct_credit_id IS NULL AND credit_lot_id IS NULL
    AND external_reference IS NOT NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL)
  OR (reference_kind='CUSTOMER_PLAN' AND customer_plan_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_cycle_id IS NULL AND plan_version_id IS NULL)
  OR (reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL AND plan_version_id IS NULL)
  OR (reference_kind='PLAN_VERSION' AND plan_version_id IS NOT NULL AND direct_credit_id IS NULL
    AND credit_lot_id IS NULL AND external_reference IS NULL AND customer_plan_id IS NULL AND customer_plan_cycle_id IS NULL)
);
ALTER TABLE wallet_transaction_references ADD CHECK (reference_kind IN (
  'DIRECT_CREDIT','CREDIT_LOT','EXTERNAL','CUSTOMER_PLAN','CUSTOMER_PLAN_CYCLE','PLAN_VERSION'
));

DROP INDEX idx_usage_events_transaction;
DROP INDEX idx_item_wallet_entries_cursor;

ALTER TABLE billing_blocks
  DROP CONSTRAINT ck_billing_block_totals,
  DROP CONSTRAINT ck_billing_block_cycle_snapshot,
  DROP COLUMN accumulation_recurrence_rule,
  DROP COLUMN accumulation_anchor_at,
  DROP COLUMN cycle_end,
  DROP COLUMN cycle_start,
  DROP COLUMN tier_position;

ALTER TABLE pricing_accumulators
  DROP CONSTRAINT ck_pricing_accumulator_totals,
  DROP COLUMN updated_at,
  DROP COLUMN created_at;

ALTER TABLE item_wallet_entries DROP CONSTRAINT ck_item_wallet_entry_totals;
ALTER TABLE usage_events DROP CONSTRAINT ck_usage_event_totals;

ALTER TABLE item_wallets
  DROP CONSTRAINT ck_item_wallet_pending_tier,
  DROP CONSTRAINT fk_item_wallet_last_usage,
  DROP COLUMN pending_tier_position;
