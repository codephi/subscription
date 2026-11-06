INSERT INTO catalog_scope_versions (scope_version,fingerprint)
VALUES ('00000000-0000-0000-0000-000000000001','e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
ON CONFLICT (fingerprint) DO NOTHING;

INSERT INTO catalog_scope_current (singleton,scope_version)
SELECT true,scope_version FROM catalog_scope_versions
WHERE fingerprint='e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'
ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE wallets (
  wallet_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  wallet_type text NOT NULL CHECK (wallet_type IN ('CUSTOMER','ITEM')),
  parent_customer_wallet_id uuid REFERENCES wallets(wallet_id) ON DELETE RESTRICT,
  item_id uuid REFERENCES items(item_id) ON DELETE RESTRICT,
  provisioning_scope_version uuid NOT NULL REFERENCES catalog_scope_versions(scope_version) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (
    (wallet_type='CUSTOMER' AND parent_customer_wallet_id IS NULL AND item_id IS NULL)
    OR
    (wallet_type='ITEM' AND parent_customer_wallet_id IS NOT NULL AND item_id IS NOT NULL)
  )
);

CREATE UNIQUE INDEX uq_wallet_customer
  ON wallets(customer_id) WHERE wallet_type='CUSTOMER';
CREATE UNIQUE INDEX uq_wallet_customer_item
  ON wallets(customer_id,item_id) WHERE wallet_type='ITEM';

CREATE TABLE customer_wallets (
  wallet_id uuid PRIMARY KEY REFERENCES wallets(wallet_id) ON DELETE RESTRICT,
  balance_credit_units bigint NOT NULL DEFAULT 0,
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE item_wallets (
  wallet_id uuid PRIMARY KEY REFERENCES wallets(wallet_id) ON DELETE RESTRICT,
  total_received_item_units bigint NOT NULL DEFAULT 0 CHECK (total_received_item_units >= 0),
  total_converted_item_units bigint NOT NULL DEFAULT 0 CHECK (total_converted_item_units >= 0),
  pending_item_units bigint NOT NULL DEFAULT 0 CHECK (pending_item_units >= 0),
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (total_converted_item_units + pending_item_units = total_received_item_units)
);

CREATE TABLE wallet_lifecycle_events (
  wallet_lifecycle_event_id uuid PRIMARY KEY,
  wallet_id uuid NOT NULL REFERENCES wallets(wallet_id) ON DELETE RESTRICT,
  sequence bigint NOT NULL CHECK (sequence >= 1),
  previous_status text CHECK (previous_status IN ('PROVISIONING','ACTIVE','DISABLED','ERROR')),
  new_status text NOT NULL CHECK (new_status IN ('PROVISIONING','ACTIVE','DISABLED','ERROR')),
  reason text NOT NULL,
  actor_reference text,
  correlation_id uuid NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (wallet_id,sequence)
);

CREATE TABLE wallet_effective_states (
  wallet_id uuid PRIMARY KEY REFERENCES wallets(wallet_id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('PROVISIONING','ACTIVE','DISABLED','ERROR')),
  lifecycle_sequence bigint NOT NULL CHECK (lifecycle_sequence >= 1),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE wallet_provisioning (
  customer_id uuid NOT NULL,
  scope_version uuid NOT NULL REFERENCES catalog_scope_versions(scope_version) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('PROVISIONING','ACTIVE','DISABLED','ERROR')),
  expected_item_wallets bigint NOT NULL CHECK (expected_item_wallets >= 0),
  materialized_item_wallets bigint NOT NULL CHECK (materialized_item_wallets >= 0),
  error_detail text,
  started_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (customer_id,scope_version)
);

CREATE TRIGGER trg_customer_wallets_updated_at
BEFORE UPDATE ON customer_wallets FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_item_wallets_updated_at
BEFORE UPDATE ON item_wallets FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_wallet_provisioning_updated_at
BEFORE UPDATE ON wallet_provisioning FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE OR REPLACE FUNCTION reject_wallet_mutation()
RETURNS TRIGGER AS $$
BEGIN
  RAISE EXCEPTION 'wallet and wallet lifecycle history are append-only'
    USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_wallets_append_only
BEFORE UPDATE OR DELETE ON wallets
FOR EACH ROW EXECUTE FUNCTION reject_wallet_mutation();
CREATE TRIGGER trg_wallet_lifecycle_append_only
BEFORE UPDATE OR DELETE ON wallet_lifecycle_events
FOR EACH ROW EXECUTE FUNCTION reject_wallet_mutation();

CREATE OR REPLACE FUNCTION validate_item_wallet_parent()
RETURNS TRIGGER AS $$
DECLARE
  parent_customer uuid;
  parent_type text;
BEGIN
  IF NEW.wallet_type = 'ITEM' THEN
    SELECT customer_id,wallet_type INTO parent_customer,parent_type
      FROM wallets WHERE wallet_id=NEW.parent_customer_wallet_id;
    IF parent_customer IS DISTINCT FROM NEW.customer_id OR parent_type IS DISTINCT FROM 'CUSTOMER' THEN
      RAISE EXCEPTION 'item wallet parent must be the customer wallet for customer %', NEW.customer_id
        USING ERRCODE = 'check_violation';
    END IF;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_validate_item_wallet_parent
BEFORE INSERT ON wallets
FOR EACH ROW EXECUTE FUNCTION validate_item_wallet_parent();
