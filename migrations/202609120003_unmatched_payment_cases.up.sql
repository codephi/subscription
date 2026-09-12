CREATE TABLE unmatched_payment_cases (
  unmatched_payment_case_id uuid PRIMARY KEY,
  workspace_id uuid NOT NULL REFERENCES workspace_projections(workspace_id) ON DELETE RESTRICT,
  billing_connection_id uuid NOT NULL,
  provider text NOT NULL CHECK (length(provider) BETWEEN 1 AND 50),
  provider_event_id text NOT NULL CHECK (length(provider_event_id) BETWEEN 1 AND 255),
  provider_payment_id text NOT NULL CHECK (length(provider_payment_id) BETWEEN 1 AND 255),
  amount_minor bigint NOT NULL CHECK (amount_minor > 0),
  currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
  reason text NOT NULL CHECK (reason='COLLECTION_REQUEST_NOT_FOUND'),
  candidate_customer_plan_id uuid,
  evidence jsonb NOT NULL,
  status text NOT NULL CHECK (status IN ('OPEN','RECONCILED','CLOSED_WITH_JUSTIFICATION')),
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (billing_connection_id,workspace_id)
    REFERENCES billing_connections(billing_connection_id,workspace_id) ON DELETE RESTRICT,
  FOREIGN KEY (candidate_customer_plan_id,workspace_id)
    REFERENCES customer_plans(customer_plan_id,customer_id) ON DELETE RESTRICT,
  UNIQUE (provider,provider_event_id),
  UNIQUE (provider,provider_payment_id)
);

CREATE TABLE unmatched_payment_case_events (
  unmatched_payment_case_event_id uuid PRIMARY KEY,
  unmatched_payment_case_id uuid NOT NULL REFERENCES unmatched_payment_cases(unmatched_payment_case_id) ON DELETE RESTRICT,
  sequence bigint NOT NULL CHECK (sequence > 0),
  event_type text NOT NULL CHECK (event_type IN ('OPENED','RECONCILED','CLOSED_WITH_JUSTIFICATION')),
  actor_reference text,
  evidence jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (unmatched_payment_case_id,sequence)
);

CREATE OR REPLACE FUNCTION protect_unmatched_payment_case()
RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP='DELETE' OR to_jsonb(NEW)-'status' <> to_jsonb(OLD)-'status' THEN
    RAISE EXCEPTION 'unmatched payment evidence and identity are immutable'
      USING ERRCODE='check_violation';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_unmatched_payment_case_protected
BEFORE UPDATE OR DELETE ON unmatched_payment_cases
FOR EACH ROW EXECUTE FUNCTION protect_unmatched_payment_case();

CREATE TRIGGER trg_unmatched_payment_case_events_append_only
BEFORE UPDATE OR DELETE ON unmatched_payment_case_events
FOR EACH ROW EXECUTE FUNCTION reject_credit_history_mutation();
