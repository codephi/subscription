WITH candidate_plans AS (
  SELECT c.customer_plan_id,c.created_at,c.commercial_status,c.renewal_status,
    c.commercial_status IN ('ACTIVE','ACTIVE_PAID','PAST_DUE')
      AND c.activation_status='ACTIVATED' AS usable,
    EXISTS(SELECT 1 FROM subscription_plan_products p
      WHERE p.plan_version_id=c.plan_version_id AND p.product_id=$2) AS includes_product,
    EXISTS(SELECT 1 FROM customer_plan_entitlements e
      WHERE e.customer_plan_id=c.customer_plan_id AND e.product_id=$2
        AND e.effective_from<=$3
        AND (e.effective_until IS NULL OR e.effective_until>$3)) AS effective_entitlement
  FROM customer_plans c WHERE c.customer_id=$1
)
SELECT commercial_status,renewal_status,usable,
  usable AND effective_entitlement AS entitled
FROM candidate_plans
-- An unrelated or pending contract must not hide a valid grant for this product.
ORDER BY (usable AND effective_entitlement) DESC,includes_product DESC,
  usable DESC,created_at DESC,customer_plan_id DESC
LIMIT 1
