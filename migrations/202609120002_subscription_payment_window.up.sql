ALTER TABLE subscriptions
  ADD COLUMN payment_completion_window interval NOT NULL DEFAULT interval '15 minutes',
  ADD CONSTRAINT ck_subscription_payment_completion_window
  CHECK (payment_completion_window > interval '0 seconds');
