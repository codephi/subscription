DROP TRIGGER IF EXISTS trg_collection_coupon_usage ON collection_requests;
DROP FUNCTION IF EXISTS synchronize_coupon_collection_usage();
ALTER TABLE collection_requests DROP CONSTRAINT IF EXISTS collection_coupon_snapshot_check;
ALTER TABLE collection_requests DROP COLUMN IF EXISTS coupon_id, DROP COLUMN IF EXISTS coupon_code,
  DROP COLUMN IF EXISTS base_amount_minor,
  DROP COLUMN IF EXISTS discount_amount_minor, DROP COLUMN IF EXISTS coupon_version;
ALTER TABLE billing_checkouts DROP COLUMN IF EXISTS coupon_id, DROP COLUMN IF EXISTS checkout_currency,
  DROP COLUMN IF EXISTS discount_amount_minor, DROP COLUMN IF EXISTS base_amount_minor,
  DROP COLUMN IF EXISTS completed_without_payment, DROP COLUMN IF EXISTS payment_method_binding_id,
  DROP COLUMN IF EXISTS coupon_code;
DROP TRIGGER IF EXISTS trg_promotion_history_append_only ON promotion_history;
DROP TRIGGER IF EXISTS trg_coupon_reservations_append_only ON coupon_checkout_reservations;
DROP TRIGGER IF EXISTS trg_voucher_redemptions_append_only ON voucher_redemptions;
DROP TRIGGER IF EXISTS trg_coupons_updated_at ON coupons;
DROP TRIGGER IF EXISTS trg_vouchers_updated_at ON vouchers;
DROP TABLE IF EXISTS promotion_history;
DROP TABLE IF EXISTS coupon_checkout_reservations;
DROP TABLE IF EXISTS voucher_redemptions;
DROP TABLE IF EXISTS promotion_usage_counters;
DROP INDEX IF EXISTS uq_wallet_reference_voucher;
DROP INDEX IF EXISTS uq_wallet_reference_coupon;
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_shape_check;
ALTER TABLE wallet_transaction_references DROP COLUMN voucher_id, DROP COLUMN coupon_id;
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references ADD CONSTRAINT wallet_transaction_references_reference_kind_check
  CHECK (reference_kind IN ('DIRECT_CREDIT','CREDIT_LOT','EXTERNAL','CUSTOMER_PLAN','CUSTOMER_PLAN_CYCLE',
    'PLAN_VERSION','USAGE_EVENT','DEBIT','PRODUCT','ITEM','ITEM_WALLET'));
ALTER TABLE wallet_transaction_references ADD CONSTRAINT wallet_transaction_references_check
  CHECK ((reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND num_nonnulls(credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='CREDIT_LOT' AND credit_lot_id IS NOT NULL AND num_nonnulls(direct_credit_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='EXTERNAL' AND external_reference IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='CUSTOMER_PLAN' AND customer_plan_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='PLAN_VERSION' AND plan_version_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='USAGE_EVENT' AND usage_event_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,debit_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='DEBIT' AND debit_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,product_id,item_id,item_wallet_id)=0)
      OR (reference_kind='PRODUCT' AND product_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,item_id,item_wallet_id)=0)
      OR (reference_kind='ITEM' AND item_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_wallet_id)=0)
      OR (reference_kind='ITEM_WALLET' AND item_wallet_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id)=0));
ALTER TABLE customer_wallet_entries DROP CONSTRAINT customer_wallet_entries_entry_type_check;
ALTER TABLE customer_wallet_entries ADD CONSTRAINT customer_wallet_entries_entry_type_check
  CHECK (entry_type IN ('DEBIT','DIRECT_CREDIT','SUBSCRIPTION_CREDIT','CREDIT_EXPIRY_FORFEITURE',
    'ON_DEMAND_CREDIT','VOUCHER_CREDIT','COMPENSATION'));
DROP TABLE IF EXISTS coupons;
DROP TABLE IF EXISTS vouchers;
