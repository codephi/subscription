CREATE TABLE subscription_calendar_jobs (
  customer_plan_cycle_id uuid PRIMARY KEY REFERENCES customer_plan_cycles(customer_plan_cycle_id),
  due_at timestamptz NOT NULL,
  retry_at timestamptz NOT NULL DEFAULT '-infinity',
  lease_token uuid,
  lease_expires_at timestamptz,
  attempts bigint NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  last_error_code text,
  completed_at timestamptz,
  CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL))
);
CREATE INDEX ix_subscription_calendar_jobs_due ON subscription_calendar_jobs(due_at)
  WHERE completed_at IS NULL;

CREATE FUNCTION sync_subscription_calendar_job() RETURNS trigger AS $$
BEGIN
  IF NEW.status <> 'ACTIVE' THEN
    UPDATE subscription_calendar_jobs SET completed_at=clock_timestamp(),
      lease_token=NULL,lease_expires_at=NULL
    WHERE customer_plan_cycle_id=NEW.customer_plan_cycle_id AND completed_at IS NULL;
    RETURN NEW;
  END IF;
  INSERT INTO subscription_calendar_jobs(customer_plan_cycle_id,due_at)
  SELECT NEW.customer_plan_cycle_id,NEW.current_period_end
  FROM customer_plans c JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id
  WHERE c.customer_plan_id=NEW.customer_plan_id AND p.commercial_model='FREE'
    AND NEW.current_period_end IS NOT NULL
  ON CONFLICT DO NOTHING;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_subscription_calendar_jobs
AFTER INSERT OR UPDATE OF status ON customer_plan_cycles
FOR EACH ROW EXECUTE FUNCTION sync_subscription_calendar_job();

-- Existing materialized cycles are the durable source of calendar work.
INSERT INTO subscription_calendar_jobs(customer_plan_cycle_id,due_at)
SELECT cy.customer_plan_cycle_id,cy.current_period_end
FROM customer_plan_cycles cy JOIN customer_plans c ON c.customer_plan_id=cy.customer_plan_id
JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id
WHERE cy.status='ACTIVE' AND cy.current_period_end IS NOT NULL AND p.commercial_model='FREE';
