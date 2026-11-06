ALTER TABLE payment_method_bindings
  ADD COLUMN display_name text CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 50);
