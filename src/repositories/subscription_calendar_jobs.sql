WITH candidate AS (
  SELECT customer_plan_cycle_id FROM subscription_calendar_jobs
  WHERE completed_at IS NULL AND due_at<=$1 AND retry_at<=clock_timestamp()
    AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
  ORDER BY due_at,customer_plan_cycle_id FOR UPDATE SKIP LOCKED LIMIT 1
), claimed AS (
  UPDATE subscription_calendar_jobs j SET lease_token=$2,
    lease_expires_at=clock_timestamp()+interval '60 seconds',attempts=attempts+1
  FROM candidate WHERE j.customer_plan_cycle_id=candidate.customer_plan_cycle_id
  RETURNING j.customer_plan_cycle_id
)
SELECT cy.customer_plan_cycle_id,c.customer_plan_id,c.anchor_at,
  cy.cycle_ordinal-c.anchor_cycle_ordinal+1 cycle_ordinal,p.recurrence
FROM claimed JOIN customer_plan_cycles cy USING(customer_plan_cycle_id)
JOIN customer_plans c ON c.customer_plan_id=cy.customer_plan_id
JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id
