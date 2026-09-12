ALTER TABLE subscriptions
  DROP CONSTRAINT IF EXISTS ck_subscription_payment_completion_window,
  DROP COLUMN IF EXISTS payment_completion_window;
