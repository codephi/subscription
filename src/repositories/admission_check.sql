SELECT p.admission_policy='OPEN' OR COALESCE(
  evidence.valid_until>clock_timestamp() AND policy.required_facts <@ evidence.verified_facts, false
) AS allowed, CASE WHEN p.admission_policy='OPEN' THEN NULL ELSE evidence.event_id END AS evidence_event_id
FROM subscription_plan_versions p
LEFT JOIN subscription_admission_policies policy ON policy.policy_version_id=p.admission_policy_version_id
LEFT JOIN LATERAL (
  SELECT e.event_id,e.verified_facts,e.valid_until FROM subscription_admission_evidence e
  WHERE e.account_id=$1 AND e.policy_version_id=p.admission_policy_version_id
  ORDER BY e.sequence DESC LIMIT 1
) evidence ON true
WHERE p.plan_version_id=$2
