# ADR 0002: idempotência e concorrência

## Decisão

Operações externas com efeito financeiro ou de consumo reservam, dentro da
mesma transação, `(workspace_id, idempotency_key)` e
`(workspace_id, transaction_id)`. Qualquer reutilização retorna `409` e nunca
repete o corpo de sucesso.

O PostgreSQL opera em `READ COMMITTED` com constraints únicas e locks de linha.
A ordem de aquisição segue a seção 7 do plano técnico: idempotência, recurso
exclusivo da operação, acumuladores ordenados, customer wallet e lançamentos.

## Consequências

- chamadas concorrentes esperam o commit ou rollback da dona da chave;
- falhas revertem todos os efeitos da operação;
- retries técnicos internos usam chaves naturais determinísticas separadas do
  contrato HTTP externo.
