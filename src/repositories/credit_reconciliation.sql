WITH entries AS (
    SELECT e.*,
           lag(balance_after_credit_units, 1, 0::bigint) OVER (ORDER BY entry_sequence) AS previous_balance,
           row_number() OVER (ORDER BY entry_sequence) AS expected_sequence
    FROM customer_wallet_entries e WHERE e.customer_id = $1
)
SELECT cw.balance_credit_units,
       COALESCE((SELECT sum(signed_credit_units)::bigint FROM entries), 0) AS ledger_balance,
       NOT EXISTS (
           SELECT 1 FROM entries
           WHERE entry_sequence <> expected_sequence OR balance_before_credit_units <> previous_balance
       ) AS ledger_chain_valid,
       COALESCE((SELECT sum(remaining_credit_units)::bigint FROM credit_lots
                 WHERE customer_id = $1 AND (expires_at IS NULL OR expires_at > now())), 0) AS lot_balance
FROM wallets w JOIN customer_wallets cw ON cw.wallet_id = w.wallet_id
WHERE w.customer_id = $1 AND w.wallet_type = 'CUSTOMER'
