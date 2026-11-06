# ADR 0002: idempotência e concorrência

## Decisão

Operações externas com efeito financeiro ou de consumo reservam, dentro da
mesma transação, `(account_id, idempotency_key)` e
`(account_id, transaction_id)`. Qualquer reutilização retorna `409` e nunca
repete o corpo de sucesso.

Quando existe uma reserva concluída, o conflito inclui `existing_operation` com
`account_id`, `operation_kind`, `resource_id` e o `transaction_id` original.
Essa referência é resolvida exclusivamente no account da chamada, inclusive
quando a chave é reutilizada com outro payload/transação. O cliente consulta a
transação original após perder a resposta; o erro não devolve seu payload,
metadata nem dados de outro account. Reserva sem recurso concluído mantém o
409 sem referência.

O PostgreSQL opera em `READ COMMITTED` com constraints únicas e locks de linha.
A ordem de aquisição segue a seção 7 do plano técnico: idempotência, recurso
exclusivo da operação, acumuladores ordenados, customer wallet e lançamentos.

## Consequências

- chamadas concorrentes esperam o commit ou rollback da dona da chave;
- falhas revertem todos os efeitos da operação;
- retries técnicos internos usam chaves naturais determinísticas separadas do
  contrato HTTP externo.
