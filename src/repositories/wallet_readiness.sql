SELECT EXISTS (
    SELECT 1
    FROM catalog_scope_current c
    JOIN wallet_provisioning p ON p.scope_version = c.scope_version
    JOIN wallets parent ON parent.customer_id = p.customer_id AND parent.wallet_type = 'CUSTOMER'
    JOIN customer_wallets cw ON cw.wallet_id = parent.wallet_id
    JOIN wallet_effective_states ps ON ps.wallet_id = parent.wallet_id
    WHERE c.singleton AND p.customer_id = $1
      AND p.status = 'ACTIVE' AND ps.status = 'ACTIVE'
      AND p.expected_item_wallets = p.materialized_item_wallets
      AND p.expected_item_wallets = (
          SELECT count(*) FROM catalog_scope_items WHERE scope_version = c.scope_version
      )
      AND NOT EXISTS (
          SELECT 1 FROM catalog_scope_items expected
          WHERE expected.scope_version = c.scope_version
            AND NOT EXISTS (
                SELECT 1 FROM wallets w
                JOIN item_wallets iw ON iw.wallet_id = w.wallet_id
                JOIN wallet_effective_states ws ON ws.wallet_id = w.wallet_id
                WHERE w.customer_id = p.customer_id AND w.item_id = expected.item_id
                  AND w.wallet_type = 'ITEM' AND w.parent_customer_wallet_id = parent.wallet_id
                  AND ws.status = 'ACTIVE'
            )
      )
)
