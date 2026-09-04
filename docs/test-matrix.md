# Matriz de rastreabilidade de testes

Esta matriz é o gate de implementação da V1. Um item só muda para `passing`
quando o teste indicado existe, não está ignorado e passa junto com toda a
suíte acumulada. Nenhuma fase posterior começa enquanto houver item
`not_implemented` ou `failing` na fase atual.

## Critérios de saída por fase

| ID | Origem | Cenário | Teste | Tipo | Fase | Estado |
|---|---|---|---|---|---:|---|
| PH-00 | fases §0 | Contratos identificam workspace, ator, estados bloqueadores e fronteira com Accounts | `contract_documents_cover_phase_zero_decisions` | contrato | 0 | passing |
| PH-01 | fases §1 | Assinatura inválida não altera estado; inbox/outbox convergem sob duplicação e falha | `workspace_event_foundation_is_atomic_and_idempotent`, `outbox_retries_dead_letters_and_replays` | integração | 1 | passing |
| PH-02 | fases §2 | Versão publicada determina preços, itens faturáveis e escopo de wallets de forma imutável | `published_catalog_scope_is_reproducible` | integração | 2 | passing |
| PH-03 | fases §3 | Provisionamento duplicado/concorrente converge e estados externos bloqueiam mutações | `wallet_provisioning_converges_for_workspace_lifecycle` | concorrência | 3 | passing |
| PH-04 | fases §4 | Cada alteração de saldo tem um lançamento e créditos repetidos não duplicam efeito | `customer_wallet_ledger_is_atomic_and_idempotent` | concorrência | 4 | passing |
| PH-05 | fases §5 | Franquia gratuita concede e expira uma vez por ciclo | `subscription_cycle_grant_and_expiry_are_unique`, `subscription_plans_migration_round_trips` | integração | 5 | passing |
| PH-06 | fases §6 | Consumos concorrentes não perdem unidades e rejeições são totalmente atômicas | `usage_conversion_is_concurrent_and_atomic` | concorrência | 6 | not_implemented |
| PH-07 | fases §7 | Conector falso confirma uma cobrança e concede uma vez, sem inferir timeout | `billing_connector_state_machine_is_idempotent` | integração | 7 | not_implemented |
| PH-08 | fases §8 | Stripe duplicado, atrasado ou fora de ordem não duplica efeito | `stripe_webhooks_converge_without_duplicate_effects` | contrato | 8 | not_implemented |
| PH-09 | fases §9 | Voucher, cupom e Compensation nunca duplicam crédito sob concorrência | `promotion_and_compensation_effects_are_unique` | concorrência | 9 | not_implemented |
| PH-10 | fases §10 | Restore, replay e reconciliação convergem sem editar histórico | `reconciliation_recovers_without_history_mutation` | recuperação | 10 | not_implemented |

## Concorrência, ordenação e consistência

| ID | Origem | Cenário | Teste | Tipo | Fase | Estado |
|---|---|---|---|---|---:|---|
| CC-01 | técnico §7 | Chamadas do mesmo item são serializadas pelo `ItemWallet` | `same_item_usage_is_serialized` | concorrência | 6 | not_implemented |
| CC-02 | técnico §7 | Itens diferentes acumulam em paralelo e convergem na customer wallet | `different_items_share_customer_balance_safely` | concorrência | 6 | not_implemented |
| CC-03 | técnico §7 | `transaction_id` repetido cria um efeito e retorna conflito | `duplicate_transaction_id_has_one_effect` | concorrência | 4 | passing |
| CC-04 | técnico §7 | `Idempotency-Key` repetida com payload diferente cria no máximo um efeito | `duplicate_idempotency_key_has_one_effect` | concorrência | 4 | passing |
| CC-05 | técnico §7 | Dois consumos de 600 formam um bloco de 1.000 e deixam 200 pendentes | `concurrent_partial_usage_forms_one_block` | concorrência | 6 | not_implemented |
| CC-06 | técnico §7 | Consumo apenas pendente não bloqueia nem lança na customer wallet | `pending_only_usage_skips_customer_ledger` | integração | 6 | not_implemented |
| CC-07 | técnico §7 | `accepted_at` após lock ordena fronteiras do mesmo item | `accepted_at_is_captured_after_item_lock` | concorrência | 6 | not_implemented |
| CC-08 | técnico §7 | Fronteira de ciclo usa acumuladores distintos | `cycle_boundary_uses_distinct_accumulators` | integração | 6 | not_implemented |
| CC-09 | técnico §7 | Travessia de tier atribui faixa e ordinal únicos | `tier_boundary_assigns_unique_blocks` | concorrência | 6 | not_implemented |
| CC-10 | técnico §7 | Dois resgates do último voucher geram um crédito | `last_voucher_is_redeemed_once` | concorrência | 9 | not_implemented |
| CC-11 | técnico §7 | Usuário externo repetido no cupom gera um crédito | `coupon_external_user_limit_is_atomic` | concorrência | 9 | not_implemented |
| CC-12 | técnico §7 | Duas execuções do mesmo ciclo geram uma franquia | `subscription_cycle_executes_once` | concorrência | 5 | passing |
| CC-13 | técnico §7 | Falha pré-commit não deixa efeitos parciais | `transaction_failure_rolls_back_all_effects` | integração | 4 | passing |
| CC-14 | técnico §7 | Retry pós-commit retorna conflito e permite consultar o original | `post_commit_retry_finds_original_transaction` | integração | 4 | passing |
| CC-15 | técnico §7 | Provisionamento concorrente reutiliza a mesma wallet | `concurrent_provisioning_reuses_wallets` | concorrência | 3 | passing |
| CC-16 | técnico §7 | Consumo nunca cria wallet de forma lazy | `usage_rejects_missing_wallet` | integração | 6 | not_implemented |
| CC-17 | técnico §7 | Desativação e consumo obedecem a ordem do lock | `wallet_deactivation_serializes_with_usage` | concorrência | 6 | not_implemented |
| CC-18 | técnico §7 | Customer wallet só ativa com todas as item wallets esperadas | `customer_wallet_waits_for_complete_scope` | integração | 3 | passing |

## Critérios normativos de aceitação

| ID | Origem | Grupo de cenários coberto | Testes | Fase | Estado |
|---|---|---|---|---:|---|
| AC-01 | técnico §9 Créditos/Price | Tipos inteiros, JSON decimal, ausência de moeda, preços `unit` e `tiered`, validação e overflow | `credit_units_*`, `price_version_*` | 2 | passing |
| AC-02 | técnico §9 Hierarquia | Uma customer wallet, item wallet faturável única, vínculo pai, ausência para entitlement-only | `wallet_hierarchy_*` | 3 | passing |
| AC-03 | técnico §9 Provisionamento | Escopo completo, estado efetivo append-only, reconciliação e ausência de criação lazy | `wallet_provisioning_*` | 3 | passing |
| AC-04 | técnico §9 Consumo | Conversões 1:1/bloco/tier, pendente, saldo zero, insuficiência e item inválido | `usage_*` | 6 | not_implemented |
| AC-05 | técnico §9 Elegibilidade | Plano utilizável, entitlement, renovação inativa e crédito suficiente | `eligibility_*` | 6 | not_implemented |
| AC-06 | técnico §9 Idempotência | Reuso de chave/transação, payload distinto, timeout pós-commit e deduplicação interna | `idempotency_*` | 4 | passing |
| AC-07 | técnico §9 Ciclos | Recorrências semanal/mensal/trimestral/anual e datas de fim de mês | `subscription_calendar_*` | 5 | passing |
| AC-08 | técnico §9 Planos | Exclusividade, admissão, ativação pendente, revogação, cancelamento e downgrade sem cobrança; ativação e upgrade pagos permanecem em AC-11 | `customer_plan_exclusivity_and_lifecycle_are_atomic`, `commercial_catalog_validation_and_swagger_are_enforced` | 5 | passing |
| AC-09A | técnico §9 Lotes | Concessão, expiração, reclassificação em downgrade e conciliação sem alteração do saldo | `subscription_cycle_grant_and_expiry_are_unique`, `customer_plan_exclusivity_and_lifecycle_are_atomic` | 5 | passing |
| AC-09B | técnico §9 Lotes | Prioridade estável e alocação imutável de lotes no consumo | `credit_lot_allocation_*` | 6 | not_implemented |
| AC-10 | técnico §9 OnDemand | Elegibilidade, cobrança, confirmação, expiração e isolamento entre subscriptions | `on_demand_*` | 7 | not_implemented |
| AC-11 | técnico §9 Billing | Capacidades, tentativa única, estados, expiração, regularização e idempotência | `billing_*` | 7 | not_implemented |
| AC-12 | técnico §9 Stripe | SetupIntent, PaymentIntent, assinatura, duplicação, atraso e autenticação adicional | `stripe_*` | 8 | not_implemented |
| AC-13 | técnico §9 Conciliação | Pagamento inesperado, revisão manual e estorno externo sem efeito automático | `unmatched_payment_*`, `external_refund_*` | 8 | not_implemented |
| AC-14 | técnico §9 Voucher | Estado, vínculo, crédito persistido, resgate direto e concorrência | `voucher_*` | 9 | not_implemented |
| AC-15 | técnico §9 Cupom | Código normalizado, validade, estoque, homogeneidade e limite por usuário | `coupon_*` | 9 | not_implemented |
| AC-16 | técnico §9 Compensation | Workflow, aprovação, delta, batch, execução única e auditoria | `compensation_*` | 9 | not_implemented |
| AC-17 | técnico §9 Extratos | Sequência, cursores, correlações, snapshots históricos e saldo final | `statement_*` | 6 | not_implemented |
| AC-18 | técnico §8–9 Operação | Métricas sem alta cardinalidade, reconciliações, replay e reparo não destrutivo | `operations_*`, `reconciliation_*` | 10 | not_implemented |

## Comando do gate

Cada fase deve finalizar com:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-features
cargo test --all-features
```

## Requisitos transversais adicionais

| ID | Origem | Cenário | Teste | Tipo | Fase | Estado |
|---|---|---|---|---|---:|---|
| SW-01 | solicitação do produto | Swagger UI permanece disponível e toda rota implementada consta no OpenAPI com seus schemas e erros | `assert_openapi_contains_catalog`, `assert_wallet_swagger`, `assert_credit_swagger`, `assert_plan_swagger` | contrato | transversal | passing |
