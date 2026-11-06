CREATE FUNCTION protect_credit_lot_origin() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'credit_lot % must preserve its origin; deletion is forbidden', OLD.credit_lot_id
      USING ERRCODE = 'check_violation';
  END IF;
  IF ROW(NEW.credit_lot_id, NEW.customer_id, NEW.granting_entry_id,
         NEW.original_credit_units, NEW.created_at)
     IS DISTINCT FROM ROW(OLD.credit_lot_id, OLD.customer_id, OLD.granting_entry_id,
                          OLD.original_credit_units, OLD.created_at) THEN
    RAISE EXCEPTION 'credit_lot % must preserve its immutable origin', OLD.credit_lot_id
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END;
$$;

CREATE TRIGGER trg_credit_lot_origin
BEFORE UPDATE OR DELETE ON credit_lots
FOR EACH ROW EXECUTE FUNCTION protect_credit_lot_origin();
