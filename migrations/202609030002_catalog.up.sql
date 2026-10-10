CREATE TABLE products (
  product_id uuid PRIMARY KEY,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  description text,
  usage_model text NOT NULL CHECK (usage_model IN ('CREDIT_METERED', 'ENTITLEMENT_ONLY')),
  status text NOT NULL CHECK (status IN ('ACTIVE', 'INACTIVE', 'ARCHIVED')),
  published_at timestamptz,
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_products_updated_at
BEFORE UPDATE ON products FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE items (
  item_id uuid PRIMARY KEY,
  product_id uuid NOT NULL REFERENCES products(product_id) ON DELETE RESTRICT,
  parent_item_id uuid REFERENCES items(item_id) ON DELETE RESTRICT,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  unit_name text,
  quantity_scale bigint,
  status text NOT NULL CHECK (status IN ('ACTIVE', 'INACTIVE', 'ARCHIVED')),
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((unit_name IS NULL) = (quantity_scale IS NULL)),
  CHECK (quantity_scale IS NULL OR quantity_scale > 0)
);

CREATE INDEX idx_items_product ON items(product_id);
CREATE TRIGGER trg_items_updated_at
BEFORE UPDATE ON items FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE price_versions (
  price_version_id uuid PRIMARY KEY,
  item_id uuid NOT NULL REFERENCES items(item_id) ON DELETE RESTRICT,
  pricing_model text NOT NULL CHECK (pricing_model IN ('unit', 'tiered')),
  unit_block_size bigint,
  credit_units bigint,
  effective_from timestamptz NOT NULL,
  effective_until timestamptz,
  accumulation_anchor_at timestamptz,
  accumulation_recurrence_rule text,
  state text NOT NULL CHECK (state IN ('DRAFT', 'SCHEDULED', 'ACTIVE', 'RETIRED')),
  published_at timestamptz,
  version bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (effective_until IS NULL OR effective_until > effective_from),
  CHECK (
    (pricing_model = 'unit' AND unit_block_size > 0 AND credit_units > 0
      AND accumulation_anchor_at IS NULL AND accumulation_recurrence_rule IS NULL)
    OR
    (pricing_model = 'tiered' AND unit_block_size IS NULL AND credit_units IS NULL)
  ),
  CHECK ((accumulation_anchor_at IS NULL) = (accumulation_recurrence_rule IS NULL))
);

CREATE INDEX idx_price_versions_item_effective
  ON price_versions(item_id, effective_from DESC);

CREATE UNIQUE INDEX uq_price_versions_item_active_from
  ON price_versions(item_id, effective_from)
  WHERE state IN ('SCHEDULED', 'ACTIVE');

CREATE TABLE price_tiers (
  price_version_id uuid NOT NULL REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  position integer NOT NULL CHECK (position >= 0),
  from_accumulated_units bigint NOT NULL CHECK (from_accumulated_units >= 0),
  to_accumulated_units bigint,
  unit_block_size bigint NOT NULL CHECK (unit_block_size > 0),
  credit_units bigint NOT NULL CHECK (credit_units > 0),
  PRIMARY KEY (price_version_id, position),
  CHECK (to_accumulated_units IS NULL OR to_accumulated_units > from_accumulated_units)
);

CREATE TABLE catalog_scope_versions (
  scope_version uuid PRIMARY KEY,
  fingerprint text NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE catalog_scope_items (
  scope_version uuid NOT NULL REFERENCES catalog_scope_versions(scope_version) ON DELETE RESTRICT,
  item_id uuid NOT NULL REFERENCES items(item_id) ON DELETE RESTRICT,
  price_version_id uuid NOT NULL REFERENCES price_versions(price_version_id) ON DELETE RESTRICT,
  PRIMARY KEY (scope_version, item_id),
  UNIQUE (scope_version, price_version_id)
);

CREATE TABLE catalog_scope_current (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  scope_version uuid NOT NULL REFERENCES catalog_scope_versions(scope_version) ON DELETE RESTRICT,
  selected_at timestamptz NOT NULL DEFAULT now()
);

CREATE OR REPLACE FUNCTION protect_published_price_version()
RETURNS TRIGGER AS $$
BEGIN
  IF OLD.state <> 'DRAFT' THEN
    RAISE EXCEPTION 'published price_version % is immutable', OLD.price_version_id
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_protect_published_price_version
BEFORE UPDATE OR DELETE ON price_versions
FOR EACH ROW EXECUTE FUNCTION protect_published_price_version();

CREATE OR REPLACE FUNCTION protect_published_price_tier()
RETURNS TRIGGER AS $$
DECLARE
  owning_price_id uuid := COALESCE(NEW.price_version_id, OLD.price_version_id);
  owning_state text;
BEGIN
  SELECT state INTO owning_state FROM price_versions WHERE price_version_id = owning_price_id;
  IF owning_state <> 'DRAFT' THEN
    RAISE EXCEPTION 'tiers for published price_version % are immutable', owning_price_id
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_protect_published_price_tier
BEFORE INSERT OR UPDATE OR DELETE ON price_tiers
FOR EACH ROW EXECUTE FUNCTION protect_published_price_tier();
