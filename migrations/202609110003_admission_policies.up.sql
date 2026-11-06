CREATE TABLE subscription_admission_policies (
  policy_version_id uuid PRIMARY KEY,
  policy_id uuid NOT NULL,
  version bigint NOT NULL CHECK (version > 0),
  required_facts text[] NOT NULL CHECK (cardinality(required_facts) > 0
    AND required_facts <@ ARRAY['EMAIL_VERIFIED','IDENTITY_VERIFIED']::text[]),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(policy_id,version)
);
CREATE TRIGGER trg_admission_policy_immutable BEFORE UPDATE OR DELETE
ON subscription_admission_policies FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

ALTER TABLE subscription_plan_versions ADD COLUMN admission_policy_version_id uuid
  REFERENCES subscription_admission_policies(policy_version_id);
ALTER TABLE subscription_plan_versions ADD CONSTRAINT admission_policy_reference_shape
  CHECK (admission_policy_version_id IS NULL OR admission_policy='APPROVAL_REQUIRED');
CREATE FUNCTION protect_admission_policy_reference() RETURNS trigger AS $$
BEGIN
  IF NEW.admission_policy_version_id IS DISTINCT FROM OLD.admission_policy_version_id THEN
    RAISE EXCEPTION 'published admission policy reference is immutable' USING ERRCODE='check_violation';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_admission_reference_immutable BEFORE UPDATE ON subscription_plan_versions
FOR EACH ROW EXECUTE FUNCTION protect_admission_policy_reference();

CREATE TABLE subscription_admission_evidence (
  event_id uuid PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES account_projections(account_id),
  policy_version_id uuid NOT NULL REFERENCES subscription_admission_policies(policy_version_id),
  sequence bigint NOT NULL CHECK (sequence > 0),
  verified_facts text[] NOT NULL CHECK (verified_facts <@ ARRAY['EMAIL_VERIFIED','IDENTITY_VERIFIED']::text[]),
  evidence_reference text NOT NULL CHECK (length(evidence_reference) BETWEEN 1 AND 255),
  valid_until timestamptz NOT NULL,
  request_hash text NOT NULL,
  received_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(account_id,policy_version_id,sequence)
);
CREATE TRIGGER trg_admission_evidence_immutable BEFORE UPDATE OR DELETE
ON subscription_admission_evidence FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();

CREATE TABLE subscription_admission_decisions (
  decision_id uuid PRIMARY KEY,
  customer_plan_id uuid NOT NULL REFERENCES customer_plans(customer_plan_id),
  plan_transition_id uuid REFERENCES customer_plan_transitions(plan_transition_id),
  plan_version_id uuid NOT NULL REFERENCES subscription_plan_versions(plan_version_id),
  evidence_event_id uuid NOT NULL REFERENCES subscription_admission_evidence(event_id),
  decided_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TRIGGER trg_admission_decision_immutable BEFORE UPDATE OR DELETE
ON subscription_admission_decisions FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
