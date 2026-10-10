CREATE TABLE billing_default_stripe_credentials (
  singleton_id smallint PRIMARY KEY CHECK (singleton_id = 1),
  environment text NOT NULL CHECK (environment IN ('TEST','LIVE')),
  provider_account_reference text NOT NULL CHECK (length(provider_account_reference) BETWEEN 1 AND 255),
  api_secret_reference text NOT NULL CHECK (length(api_secret_reference) BETWEEN 1 AND 255),
  webhook_secret_reference text CHECK (webhook_secret_reference IS NULL OR length(webhook_secret_reference) BETWEEN 1 AND 255),
  configuration_version integer NOT NULL DEFAULT 1,
  updated_at timestamptz NOT NULL DEFAULT now()
);
