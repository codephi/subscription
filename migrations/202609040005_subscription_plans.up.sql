CREATE TABLE subscriptions (
  subscription_id uuid PRIMARY KEY,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  subscription_model text NOT NULL CHECK (subscription_model IN ('CREDIT_STRICT','CREDIT_FLEXIBLE','ENTITLEMENT_ONLY')),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE subscription_plan_versions (
  plan_version_id uuid PRIMARY KEY,
  subscription_id uuid NOT NULL REFERENCES subscriptions(subscription_id) ON DELETE RESTRICT,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  commercial_model text NOT NULL CHECK (commercial_model IN ('FREE','PAID')),
  price_amount_minor bigint,
  currency text,
  recurrence text NOT NULL CHECK (recurrence IN ('NONE','WEEKLY','MONTHLY','QUARTERLY','ANNUALLY')),
  admission_policy text NOT NULL CHECK (admission_policy IN ('OPEN','APPROVAL_REQUIRED')),
  accepted_payment_methods text[] NOT NULL DEFAULT '{}',
  granted_credit_units bigint NOT NULL CHECK (granted_credit_units >= 0),
  published_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz,
  revocation_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (
    (commercial_model='FREE' AND price_amount_minor IS NULL AND currency IS NULL)
    OR
    (commercial_model='PAID' AND price_amount_minor > 0 AND currency ~ '^[A-Z]{3}$')
  )
);

CREATE TABLE subscription_plan_products (
  plan_version_id uuid NOT NULL REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  product_id uuid NOT NULL REFERENCES products(product_id) ON DELETE RESTRICT,
  PRIMARY KEY (plan_version_id,product_id)
);

CREATE TABLE on_demand_plans (
  on_demand_plan_id uuid PRIMARY KEY,
  subscription_id uuid NOT NULL REFERENCES subscriptions(subscription_id) ON DELETE RESTRICT,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  price_amount_minor bigint NOT NULL CHECK (price_amount_minor > 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  credit_units bigint NOT NULL CHECK (credit_units > 0),
  published_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE customer_plans (
  customer_plan_id uuid PRIMARY KEY,
  customer_id uuid NOT NULL,
  plan_version_id uuid NOT NULL REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  commercial_status text NOT NULL CHECK (commercial_status IN (
    'PENDING_PAYMENT','ACTIVE','ACTIVE_PAID','PAST_DUE','CANCELED','EXPIRED','REVOKED'
  )),
  activation_status text NOT NULL CHECK (activation_status IN (
    'PENDING_INITIAL_PAYMENT','PENDING_CARD_VALIDATION','ACTIVATED','FAILED'
  )),
  renewal_status text NOT NULL CHECK (renewal_status IN ('CURRENT','RENEWAL_INACTIVE')),
  anchor_at timestamptz NOT NULL,
  cancel_at_period_end boolean NOT NULL DEFAULT false,
  ended_at timestamptz,
  end_reason text,
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_customer_plans_updated_at
BEFORE UPDATE ON customer_plans FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE active_customer_plan_slots (
  customer_id uuid NOT NULL,
  subscription_id uuid NOT NULL REFERENCES subscriptions(subscription_id) ON DELETE RESTRICT,
  customer_plan_id uuid NOT NULL UNIQUE REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (customer_id,subscription_id)
);

CREATE TABLE customer_plan_cycles (
  customer_plan_cycle_id uuid PRIMARY KEY,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  cycle_ordinal bigint NOT NULL CHECK (cycle_ordinal >= 1),
  current_period_start timestamptz NOT NULL,
  current_period_end timestamptz,
  granted_credit_units bigint NOT NULL CHECK (granted_credit_units >= 0),
  status text NOT NULL CHECK (status IN ('ACTIVE','COMPLETED','CANCELED')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_plan_id,cycle_ordinal),
  CHECK (current_period_end IS NULL OR current_period_end > current_period_start)
);

CREATE UNIQUE INDEX uq_customer_plan_active_cycle
  ON customer_plan_cycles(customer_plan_id) WHERE status='ACTIVE';

CREATE TABLE customer_plan_entitlements (
  customer_plan_entitlement_id uuid PRIMARY KEY,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  product_id uuid NOT NULL REFERENCES products(product_id) ON DELETE RESTRICT,
  effective_from timestamptz NOT NULL,
  effective_until timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (effective_until IS NULL OR effective_until > effective_from)
);

CREATE UNIQUE INDEX uq_customer_plan_active_entitlement
  ON customer_plan_entitlements(customer_plan_id,product_id) WHERE effective_until IS NULL;

CREATE TABLE customer_plan_transitions (
  plan_transition_id uuid PRIMARY KEY,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  previous_plan_version_id uuid NOT NULL REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  new_plan_version_id uuid NOT NULL REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT,
  transition_kind text NOT NULL CHECK (transition_kind IN ('UPGRADE','DOWNGRADE')),
  transaction_id text NOT NULL,
  actor_reference text NOT NULL,
  previous_anchor_at timestamptz NOT NULL,
  previous_period_end timestamptz,
  new_anchor_at timestamptz NOT NULL,
  new_period_end timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (customer_plan_id,transaction_id),
  CHECK (previous_plan_version_id <> new_plan_version_id)
);

CREATE TABLE credit_lot_reclassifications (
  credit_lot_reclassification_id uuid PRIMARY KEY,
  credit_lot_id uuid NOT NULL REFERENCES credit_lots(credit_lot_id) ON DELETE RESTRICT,
  customer_plan_cycle_id uuid NOT NULL REFERENCES customer_plan_cycles(customer_plan_cycle_id) ON DELETE RESTRICT,
  plan_transition_id uuid NOT NULL REFERENCES customer_plan_transitions(plan_transition_id) ON DELETE RESTRICT,
  preserved_credit_units bigint NOT NULL CHECK (preserved_credit_units > 0),
  previous_source_kind text NOT NULL CHECK (previous_source_kind='SUBSCRIPTION'),
  previous_expires_at timestamptz,
  new_source_kind text NOT NULL CHECK (new_source_kind='ON_DEMAND'),
  actor_reference text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (credit_lot_id,plan_transition_id)
);

CREATE TRIGGER trg_customer_plan_transitions_append_only
BEFORE UPDATE OR DELETE ON customer_plan_transitions
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
CREATE TRIGGER trg_credit_lot_reclassifications_append_only
BEFORE UPDATE OR DELETE ON credit_lot_reclassifications
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

ALTER TABLE wallet_transaction_references
  ADD COLUMN customer_plan_id uuid REFERENCES customer_plans(customer_plan_id) ON DELETE RESTRICT,
  ADD COLUMN customer_plan_cycle_id uuid REFERENCES customer_plan_cycles(customer_plan_cycle_id) ON DELETE RESTRICT,
  ADD COLUMN plan_version_id uuid REFERENCES subscription_plan_versions(plan_version_id) ON DELETE RESTRICT;

ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_check;
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
ALTER TABLE wallet_transaction_references DROP CONSTRAINT wallet_transaction_references_reference_kind_check;
ALTER TABLE wallet_transaction_references ADD CHECK (reference_kind IN (
  'DIRECT_CREDIT','CREDIT_LOT','EXTERNAL','CUSTOMER_PLAN','CUSTOMER_PLAN_CYCLE','PLAN_VERSION'
));

CREATE OR REPLACE FUNCTION protect_published_commercial_catalog()
RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'published commercial catalog is immutable' USING ERRCODE='check_violation';
  END IF;
  IF TG_TABLE_NAME='subscription_plan_versions'
     AND (NEW.revoked_at IS DISTINCT FROM OLD.revoked_at OR NEW.revocation_reason IS DISTINCT FROM OLD.revocation_reason)
     AND ROW(NEW.name,NEW.subscription_id,NEW.commercial_model,NEW.price_amount_minor,NEW.currency,
       NEW.recurrence,NEW.admission_policy,NEW.accepted_payment_methods,NEW.granted_credit_units)
       IS NOT DISTINCT FROM ROW(OLD.name,OLD.subscription_id,OLD.commercial_model,OLD.price_amount_minor,OLD.currency,
       OLD.recurrence,OLD.admission_policy,OLD.accepted_payment_methods,OLD.granted_credit_units) THEN
    RETURN NEW;
  END IF;
  RAISE EXCEPTION 'published commercial catalog is immutable' USING ERRCODE='check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_subscription_plan_versions_immutable
BEFORE UPDATE OR DELETE ON subscription_plan_versions
FOR EACH ROW EXECUTE FUNCTION protect_published_commercial_catalog();
CREATE TRIGGER trg_subscription_plan_products_immutable
BEFORE UPDATE OR DELETE ON subscription_plan_products
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
