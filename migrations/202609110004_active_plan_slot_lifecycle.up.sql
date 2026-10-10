CREATE FUNCTION sync_active_customer_plan_slot() RETURNS trigger AS $$
BEGIN
  IF NEW.commercial_status IN ('ACTIVE','ACTIVE_PAID')
     AND NEW.renewal_status='CURRENT' THEN
    INSERT INTO active_customer_plan_slots(customer_id,subscription_id,customer_plan_id)
    SELECT NEW.customer_id,p.subscription_id,NEW.customer_plan_id
    FROM subscription_plan_versions p WHERE p.plan_version_id=NEW.plan_version_id
    ON CONFLICT(customer_id,subscription_id) DO UPDATE SET customer_plan_id=EXCLUDED.customer_plan_id
    WHERE active_customer_plan_slots.customer_plan_id=EXCLUDED.customer_plan_id;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'active customer plan already exists for account %',NEW.customer_id
        USING ERRCODE='unique_violation';
    END IF;
  ELSE
    DELETE FROM active_customer_plan_slots WHERE customer_plan_id=NEW.customer_plan_id;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_active_customer_plan_slot_lifecycle
AFTER UPDATE OF commercial_status,renewal_status ON customer_plans
FOR EACH ROW EXECUTE FUNCTION sync_active_customer_plan_slot();

DELETE FROM active_customer_plan_slots slot USING customer_plans plan
WHERE plan.customer_plan_id=slot.customer_plan_id
  AND (plan.commercial_status NOT IN ('ACTIVE','ACTIVE_PAID') OR plan.renewal_status<>'CURRENT');

DROP TRIGGER trg_subscription_plan_products_immutable ON subscription_plan_products;
CREATE FUNCTION protect_published_plan_products() RETURNS trigger AS $$
BEGIN
  IF TG_OP='INSERT' AND EXISTS(
    SELECT 1 FROM subscription_plan_versions p
    WHERE p.plan_version_id=NEW.plan_version_id AND p.created_at=transaction_timestamp()
  ) THEN
    RETURN NEW;
  END IF;
  RAISE EXCEPTION 'published plan products are immutable' USING ERRCODE='check_violation';
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_subscription_plan_products_immutable
BEFORE INSERT OR UPDATE OR DELETE ON subscription_plan_products
FOR EACH ROW EXECUTE FUNCTION protect_published_plan_products();
