CREATE TABLE users (
  user_id TEXT PRIMARY KEY,
  username TEXT NOT NULL COLLATE NOCASE UNIQUE,
  password_hash TEXT NOT NULL,
  account_id TEXT NOT NULL UNIQUE,
  created_event_id TEXT NOT NULL UNIQUE,
  activated_event_id TEXT NOT NULL UNIQUE,
  correlation_id TEXT NOT NULL,
  event_occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  plan_model TEXT CHECK (plan_model IS NULL OR plan_model IN ('PREPAID','SUBSCRIPTION')),
  customer_plan_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE sessions (
  session_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX ix_sessions_user ON sessions(user_id);

CREATE TABLE catalog_settings (
  setting_key TEXT PRIMARY KEY,
  setting_value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE executions (
  execution_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  transaction_id TEXT NOT NULL,
  task_name TEXT NOT NULL,
  result_text TEXT,
  credits_debited TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','COMPLETED','REJECTED')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (user_id,transaction_id)
);
CREATE TABLE checkouts (
  checkout_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  transaction_id TEXT NOT NULL UNIQUE,
  subscription_checkout_id TEXT NOT NULL UNIQUE,
  checkout_kind TEXT NOT NULL,
  status TEXT NOT NULL,
  amount_minor INTEGER,
  currency TEXT,
  granted_credit_units INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
