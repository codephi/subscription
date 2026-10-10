ALTER TABLE payment_method_bindings
  DROP CONSTRAINT payment_method_card_summary_shape,
  DROP COLUMN card_brand,
  DROP COLUMN card_last_four,
  DROP COLUMN card_exp_month,
  DROP COLUMN card_exp_year;
