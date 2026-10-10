ALTER TABLE billing_checkouts
  ADD COLUMN save_payment_method boolean NOT NULL DEFAULT false;
