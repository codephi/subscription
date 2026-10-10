ALTER TABLE payment_method_bindings
  ADD COLUMN card_brand text,
  ADD COLUMN card_last_four char(4),
  ADD COLUMN card_exp_month smallint,
  ADD COLUMN card_exp_year smallint;

ALTER TABLE payment_method_bindings
  ADD CONSTRAINT payment_method_card_summary_shape CHECK (
    (card_brand IS NULL AND card_last_four IS NULL AND card_exp_month IS NULL AND card_exp_year IS NULL)
    OR (card_brand IS NOT NULL AND length(card_brand) BETWEEN 1 AND 32
      AND card_last_four IS NOT NULL AND card_last_four ~ '^[0-9]{4}$'
      AND card_exp_month IS NOT NULL AND card_exp_month BETWEEN 1 AND 12
      AND card_exp_year IS NOT NULL
      AND card_exp_year BETWEEN 2000 AND 9999)
  );
