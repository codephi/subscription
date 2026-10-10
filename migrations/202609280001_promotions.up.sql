CREATE TABLE vouchers (
  voucher_id uuid PRIMARY KEY,
  code text NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_-]{1,64}$'),
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
  description text CHECK (length(description) BETWEEN 1 AND 1000),
  credit_units bigint NOT NULL CHECK (credit_units > 0),
  status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','DISABLED','ARCHIVED')),
  valid_from timestamptz,
  valid_until timestamptz,
  max_total_uses bigint CHECK (max_total_uses > 0),
  max_uses_per_account bigint CHECK (max_uses_per_account > 0),
  version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (valid_until IS NULL OR valid_from IS NULL OR valid_until > valid_from)
);

CREATE TABLE coupons (
  coupon_id uuid PRIMARY KEY,
  code text NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_-]{1,64}$'),
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
  description text CHECK (length(description) BETWEEN 1 AND 1000),
  discount_kind text NOT NULL CHECK (discount_kind IN ('PERCENTAGE','FIXED')),
  discount_value bigint NOT NULL CHECK (discount_value > 0),
  currency text,
  applies_to_initial boolean NOT NULL,
  applies_to_on_demand boolean NOT NULL,
  status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','DISABLED','ARCHIVED')),
  valid_from timestamptz,
  valid_until timestamptz,
  max_total_uses bigint CHECK (max_total_uses > 0),
  max_uses_per_account bigint CHECK (max_uses_per_account > 0),
  version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (applies_to_initial OR applies_to_on_demand),
  CHECK (valid_until IS NULL OR valid_from IS NULL OR valid_until > valid_from),
  CHECK ((discount_kind='PERCENTAGE' AND discount_value <= 10000 AND currency IS NULL)
      OR (discount_kind='FIXED' AND currency ~ '^[A-Z]{3}$'))
);

ALTER TABLE billing_checkouts
  ADD COLUMN coupon_code text,
  ADD COLUMN payment_method_binding_id uuid REFERENCES payment_method_bindings(payment_method_binding_id) ON DELETE RESTRICT,
  ADD COLUMN completed_without_payment boolean NOT NULL DEFAULT false,
  ADD COLUMN base_amount_minor bigint,
  ADD COLUMN discount_amount_minor bigint NOT NULL DEFAULT 0 CHECK (discount_amount_minor >= 0),
  ADD COLUMN checkout_currency text CHECK (checkout_currency IS NULL OR checkout_currency ~ '^[A-Z]{3}$'),
  ADD COLUMN coupon_id uuid REFERENCES coupons(coupon_id) ON DELETE RESTRICT;

ALTER TABLE collection_requests
  ADD COLUMN coupon_id uuid REFERENCES coupons(coupon_id) ON DELETE RESTRICT,
  ADD COLUMN coupon_code text CHECK (coupon_code IS NULL OR coupon_code ~ '^[A-Z0-9_-]{1,64}$'),
  ADD COLUMN base_amount_minor bigint CHECK (base_amount_minor IS NULL OR base_amount_minor > 0),
  ADD COLUMN discount_amount_minor bigint NOT NULL DEFAULT 0 CHECK (discount_amount_minor >= 0),
  ADD COLUMN coupon_version bigint,
  ADD CONSTRAINT collection_coupon_snapshot_check CHECK (
    (coupon_id IS NULL AND coupon_code IS NULL AND base_amount_minor IS NULL AND discount_amount_minor=0 AND coupon_version IS NULL)
    OR (coupon_id IS NOT NULL AND coupon_code IS NOT NULL AND base_amount_minor > 0 AND coupon_version > 0
      AND discount_amount_minor > 0 AND base_amount_minor=discount_amount_minor+amount_minor)
  );

CREATE TABLE promotion_usage_counters (
  promotion_kind text NOT NULL CHECK (promotion_kind IN ('VOUCHER','COUPON')),
  promotion_id uuid NOT NULL,
  account_id uuid NOT NULL REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  completed_uses bigint NOT NULL DEFAULT 0 CHECK (completed_uses >= 0),
  reserved_uses bigint NOT NULL DEFAULT 0 CHECK (reserved_uses >= 0),
  PRIMARY KEY (promotion_kind,promotion_id,account_id)
);

CREATE TABLE voucher_redemptions (
  voucher_redemption_id uuid PRIMARY KEY,
  voucher_id uuid NOT NULL REFERENCES vouchers(voucher_id) ON DELETE RESTRICT,
  account_id uuid NOT NULL REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  transaction_id text NOT NULL CHECK (length(transaction_id) BETWEEN 1 AND 255),
  customer_wallet_entry_id uuid NOT NULL UNIQUE REFERENCES customer_wallet_entries(customer_wallet_entry_id) ON DELETE RESTRICT,
  credit_lot_id uuid NOT NULL UNIQUE REFERENCES credit_lots(credit_lot_id) ON DELETE RESTRICT,
  credit_units bigint NOT NULL CHECK (credit_units > 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(account_id,transaction_id)
);

CREATE TABLE coupon_checkout_reservations (
  coupon_checkout_reservation_id uuid PRIMARY KEY,
  coupon_id uuid NOT NULL REFERENCES coupons(coupon_id) ON DELETE RESTRICT,
  account_id uuid NOT NULL REFERENCES account_projections(account_id) ON DELETE RESTRICT,
  checkout_id uuid NOT NULL UNIQUE REFERENCES billing_checkouts(checkout_id) ON DELETE RESTRICT,
  collection_request_id uuid UNIQUE REFERENCES collection_requests(collection_request_id) ON DELETE RESTRICT,
  checkout_kind text NOT NULL CHECK (checkout_kind IN ('INITIAL','ON_DEMAND')),
  base_amount_minor bigint NOT NULL CHECK (base_amount_minor > 0),
  discount_amount_minor bigint NOT NULL CHECK (discount_amount_minor > 0),
  final_amount_minor bigint NOT NULL CHECK (final_amount_minor >= 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  discount_kind text NOT NULL CHECK (discount_kind IN ('PERCENTAGE','FIXED')),
  discount_value bigint NOT NULL CHECK (discount_value > 0),
  coupon_version bigint NOT NULL CHECK (coupon_version > 0),
  status text NOT NULL CHECK (status IN ('RESERVED','COMPLETED','RELEASED')),
  created_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  CHECK (base_amount_minor = discount_amount_minor + final_amount_minor)
);

CREATE TABLE promotion_history (
  promotion_history_id uuid PRIMARY KEY,
  promotion_kind text NOT NULL CHECK (promotion_kind IN ('VOUCHER','COUPON')),
  promotion_id uuid NOT NULL,
  version bigint NOT NULL,
  action text NOT NULL,
  actor_reference text,
  before_snapshot jsonb NOT NULL CHECK (jsonb_typeof(before_snapshot)='object'),
  after_snapshot jsonb NOT NULL CHECK (jsonb_typeof(after_snapshot)='object'),
  occurred_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(promotion_kind,promotion_id,version)
);

CREATE INDEX idx_voucher_redemptions_history ON voucher_redemptions(voucher_id,created_at DESC);
CREATE INDEX idx_coupon_reservations_history ON coupon_checkout_reservations(coupon_id,created_at DESC);
CREATE INDEX idx_promotion_history_resource ON promotion_history(promotion_kind,promotion_id,version DESC);

ALTER TABLE wallet_transaction_references
  ADD COLUMN voucher_id uuid REFERENCES vouchers(voucher_id) ON DELETE RESTRICT,
  ADD COLUMN coupon_id uuid REFERENCES coupons(coupon_id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX uq_wallet_reference_voucher
  ON wallet_transaction_references(customer_wallet_entry_id,voucher_id) WHERE voucher_id IS NOT NULL;
CREATE UNIQUE INDEX uq_wallet_reference_coupon
  ON wallet_transaction_references(customer_wallet_entry_id,coupon_id) WHERE coupon_id IS NOT NULL;

ALTER TABLE customer_wallet_entries DROP CONSTRAINT customer_wallet_entries_entry_type_check;
ALTER TABLE customer_wallet_entries ADD CONSTRAINT customer_wallet_entries_entry_type_check
  CHECK (entry_type IN ('DEBIT','DIRECT_CREDIT','SUBSCRIPTION_CREDIT','CREDIT_EXPIRY_FORFEITURE',
    'ON_DEMAND_CREDIT','VOUCHER_CREDIT','COMPENSATION'));
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references ADD CONSTRAINT wallet_transaction_references_reference_kind_check
  CHECK (reference_kind IN ('DIRECT_CREDIT','CREDIT_LOT','EXTERNAL','CUSTOMER_PLAN','CUSTOMER_PLAN_CYCLE',
    'PLAN_VERSION','USAGE_EVENT','DEBIT','PRODUCT','ITEM','ITEM_WALLET','VOUCHER','COUPON'));
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_check;
ALTER TABLE wallet_transaction_references ADD CONSTRAINT wallet_transaction_references_shape_check
  CHECK ((reference_kind='DIRECT_CREDIT' AND direct_credit_id IS NOT NULL AND num_nonnulls(credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='CREDIT_LOT' AND credit_lot_id IS NOT NULL AND num_nonnulls(direct_credit_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='EXTERNAL' AND external_reference IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='CUSTOMER_PLAN' AND customer_plan_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='CUSTOMER_PLAN_CYCLE' AND customer_plan_cycle_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='PLAN_VERSION' AND plan_version_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='USAGE_EVENT' AND usage_event_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,debit_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='DEBIT' AND debit_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,product_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='PRODUCT' AND product_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,item_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='ITEM' AND item_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_wallet_id,voucher_id,coupon_id)=0)
      OR (reference_kind='ITEM_WALLET' AND item_wallet_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,voucher_id,coupon_id)=0)
      OR (reference_kind='VOUCHER' AND voucher_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,coupon_id)=0)
      OR (reference_kind='COUPON' AND coupon_id IS NOT NULL AND num_nonnulls(direct_credit_id,credit_lot_id,external_reference,customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id,product_id,item_id,item_wallet_id,voucher_id)=0));

CREATE TRIGGER trg_vouchers_updated_at BEFORE UPDATE ON vouchers FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_coupons_updated_at BEFORE UPDATE ON coupons FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_voucher_redemptions_append_only BEFORE UPDATE OR DELETE ON voucher_redemptions FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_coupon_reservations_append_only BEFORE DELETE ON coupon_checkout_reservations FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_promotion_history_append_only BEFORE UPDATE OR DELETE ON promotion_history FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

CREATE OR REPLACE FUNCTION synchronize_coupon_collection_usage()
RETURNS TRIGGER AS $$
DECLARE reservation coupon_checkout_reservations%ROWTYPE;
BEGIN
  IF OLD.status=NEW.status THEN RETURN NEW; END IF;
  IF NEW.status='PAID' THEN
    UPDATE coupon_checkout_reservations SET status='COMPLETED',completed_at=clock_timestamp()
      WHERE collection_request_id=NEW.collection_request_id AND status='RESERVED'
      RETURNING * INTO reservation;
    IF FOUND THEN
      UPDATE promotion_usage_counters SET reserved_uses=reserved_uses-1,completed_uses=completed_uses+1
        WHERE promotion_kind='COUPON' AND promotion_id=reservation.coupon_id AND account_id=reservation.account_id;
    END IF;
  ELSIF NEW.status IN ('EXPIRED','EXHAUSTED','CANCELED') THEN
    UPDATE coupon_checkout_reservations SET status='RELEASED'
      WHERE collection_request_id=NEW.collection_request_id AND status='RESERVED'
      RETURNING * INTO reservation;
    IF FOUND THEN
      UPDATE promotion_usage_counters SET reserved_uses=reserved_uses-1
        WHERE promotion_kind='COUPON' AND promotion_id=reservation.coupon_id AND account_id=reservation.account_id;
    END IF;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_collection_coupon_usage
AFTER UPDATE OF status ON collection_requests
FOR EACH ROW EXECUTE FUNCTION synchronize_coupon_collection_usage();
