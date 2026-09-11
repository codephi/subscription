SELECT t.operation_kind, t.resource_id, t.transaction_id
FROM transaction_reservations t
WHERE t.workspace_id = $1 AND t.resource_id IS NOT NULL
  AND (
      (NOT $3 AND t.transaction_id = $2)
      OR ($3 AND EXISTS (
          SELECT 1 FROM idempotency_records i
          WHERE i.workspace_id = t.workspace_id AND i.idempotency_key = $2
            AND i.operation_kind = t.operation_kind AND i.resource_id = t.resource_id
      ))
  )
ORDER BY t.created_at, t.transaction_id
LIMIT 1
