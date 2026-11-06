DROP TRIGGER trg_validate_item_wallet_parent ON wallets;
DROP FUNCTION validate_item_wallet_parent();
DROP TRIGGER trg_wallet_lifecycle_append_only ON wallet_lifecycle_events;
DROP TRIGGER trg_wallets_append_only ON wallets;
DROP FUNCTION reject_wallet_mutation();
DROP TABLE wallet_provisioning;
DROP TABLE wallet_effective_states;
DROP TABLE wallet_lifecycle_events;
DROP TABLE item_wallets;
DROP TABLE customer_wallets;
DROP TABLE wallets;
DELETE FROM catalog_scope_current
WHERE scope_version='00000000-0000-0000-0000-000000000001';
DELETE FROM catalog_scope_versions
WHERE scope_version='00000000-0000-0000-0000-000000000001';
