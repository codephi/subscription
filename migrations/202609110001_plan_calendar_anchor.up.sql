ALTER TABLE customer_plans
  ADD COLUMN anchor_cycle_ordinal bigint NOT NULL DEFAULT 1 CHECK (anchor_cycle_ordinal >= 1);

-- A transition resets the calendar anchor, but historical cycle ordinals stay unique.
UPDATE customer_plans p
SET anchor_cycle_ordinal = COALESCE(
  (SELECT min(c.cycle_ordinal) FROM customer_plan_cycles c
   WHERE c.customer_plan_id=p.customer_plan_id AND c.current_period_start>=p.anchor_at), 1);
