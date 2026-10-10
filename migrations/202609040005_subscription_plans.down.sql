DROP TRIGGER trg_subscription_plan_products_immutable ON subscription_plan_products;
DROP TRIGGER trg_subscription_plan_versions_immutable ON subscription_plan_versions;
DROP FUNCTION protect_published_commercial_catalog();
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_check;
ALTER TABLE wallet_transaction_references DROP COLUMN plan_version_id;
ALTER TABLE wallet_transaction_references DROP COLUMN customer_plan_cycle_id;
ALTER TABLE wallet_transaction_references DROP COLUMN customer_plan_id;
ALTER TABLE wallet_transaction_references ADD CHECK (
  (reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND credit_lot_id IS NULL AND external_reference IS NULL)
  OR (reference_kind='CREDIT_LOT' AND direct_credit_id IS NULL AND credit_lot_id IS NOT NULL AND external_reference IS NULL)
  OR (reference_kind='EXTERNAL' AND direct_credit_id IS NULL AND credit_lot_id IS NULL AND external_reference IS NOT NULL)
);
ALTER TABLE wallet_transaction_references ADD CHECK (
  reference_kind IN ('DIRECT_CREDIT','CREDIT_LOT','EXTERNAL')
);
DROP TABLE customer_plan_entitlements;
DROP TABLE credit_lot_reclassifications;
DROP TABLE customer_plan_transitions;
DROP TABLE customer_plan_cycles;
DROP TABLE active_customer_plan_slots;
DROP TABLE customer_plans;
DROP TABLE on_demand_plans;
DROP TABLE subscription_plan_products;
DROP TABLE subscription_plan_versions;
DROP TABLE subscriptions;
