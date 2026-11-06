# Runbook do Billing no MVP

## Cobrança incerta

1. Consulte `GET /v1/admin/billing/operations` e localize a tentativa
   `UNCERTAIN` pelo `collection_request_id`.
2. Consulte o PaymentIntent no Stripe pela chave idempotente persistida.
3. Não crie outra tentativa. Reenvie o evento original pelo replay do Stripe.
4. Confirme que a inbox terminou em `APPLIED`, `REJECTED` ou `UNMATCHED` e que
   há no máximo uma concessão ligada à cobrança.

## Pagamento não conciliado

1. Consulte `GET /v1/admin/accounts/{account_id}/billing/unmatched-payments`.
2. Compare provider, PaymentIntent, valor, moeda e horário com a solicitação
   local candidata.
3. Não edite Wallet, razão ou caso operacional. Preserve o caso `OPEN` até uma
   conciliação manual auditável ser aprovada.

## Replay e eventos atrasados

1. Corrija primeiro segredo, conectividade ou causa de dead-letter.
2. Para outbox, use o replay administrativo existente; ele preserva a sequência.
3. Para Stripe, reenvie o mesmo evento com o mesmo ID e payload. Payload alterado
   para o mesmo ID é conflito.
4. Falha ou ação adicional posterior à confirmação não regride plano ou crédito.

## Rotação de credenciais

1. Crie a nova chave ou webhook secret no Stripe.
2. Atualize o valor da variável apontada por `secret_reference` ou
   `webhook_secret_reference` e reinicie as instâncias gradualmente.
3. Envie um evento de teste e confirme os contadores operacionais.
4. Revogue a credencial antiga somente após todas as instâncias migrarem. Nunca
   grave o segredo na conexão ou em logs.
