ALTER TABLE payment_method_setup_sessions
  ADD COLUMN display_name text CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 50);
