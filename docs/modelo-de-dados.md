# Modelo de dados da API

Este documento descreve o schema PostgreSQL criado pelas migrations em `migrations/`. Os diagramas incluem todas as tabelas e todas as relações declaradas por chave estrangeira. Para facilitar a leitura, cada entidade mostra sua chave primária (`PK`) e suas chaves estrangeiras (`FK`); os demais campos, restrições e índices estão definidos nas migrations.

As migrations `202609...` formam o modelo de Subscription API. `authors` e `posts`, criadas pela migration inicial `202601020000_init.sql`, são tabelas de exemplo do bootstrap e não participam dos fluxos da API de assinaturas.

## Visão por domínio

### Bootstrap de exemplo

```mermaid
erDiagram
    authors ||--o{ posts : escreve
    authors {
        uuid id PK
    }
    posts {
        uuid id PK
        uuid author_id FK
    }
```

### Workspace, integração e auditoria

```mermaid
erDiagram
    integration_inbox ||--o| integration_inbox_quarantine : quarentena
    workspace_projections {
        uuid workspace_id PK
    }
    integration_inbox {
        uuid event_id PK
    }
    integration_inbox_quarantine {
        uuid event_id PK
    }
    outbox_events {
        uuid event_id PK
    }
    idempotency_records {
        uuid workspace_id PK
        text idempotency_key PK
    }
    transaction_reservations {
        uuid workspace_id PK
        text transaction_id PK
    }
    audit_events {
        uuid audit_event_id PK
    }
```

`integration_inbox_quarantine.event_id` referencia o evento recebido. `outbox_events`, `idempotency_records`, `transaction_reservations` e `audit_events` não têm FK para o workspace: o escopo é identificado pelo UUID recebido de Accounts. `workspace_projections.last_event_id` também não é FK para a inbox; é a identidade do último evento aplicado.

### Catálogo e preços

```mermaid
erDiagram
    products ||--o{ items : agrupa
    items o|--o{ items : hierarquia
    items ||--o{ price_versions : precifica
    price_versions ||--o{ price_tiers : detalha
    catalog_scope_versions ||--o{ catalog_scope_items : compoe
    items ||--o{ catalog_scope_items : inclui
    price_versions ||--o{ catalog_scope_items : seleciona
    catalog_scope_versions ||--o{ catalog_scope_current : atual
    products {
        uuid product_id PK
    }
    items {
        uuid item_id PK
        uuid product_id FK
        uuid parent_item_id FK
    }
    price_versions {
        uuid price_version_id PK
        uuid item_id FK
    }
    price_tiers {
        uuid price_version_id PK
        int position PK
    }
    catalog_scope_versions {
        uuid scope_version PK
    }
    catalog_scope_items {
        uuid scope_version PK
        uuid item_id PK
        uuid price_version_id FK
    }
    catalog_scope_current {
        bool singleton PK
        uuid scope_version FK
    }
```

### Provisionamento de carteiras

```mermaid
erDiagram
    catalog_scope_versions ||--o{ wallets : provisiona_com
    items o|--o{ wallets : carteira_do_item
    wallets o|--o| wallets : carteira_pai
    price_versions o|--o{ item_wallets : preco_pendente
    usage_events o|--o| item_wallets : ultimo_uso
    wallets ||--o| customer_wallets : especializa
    wallets ||--o| item_wallets : especializa
    wallets ||--o{ wallet_lifecycle_events : historico
    wallets ||--o| wallet_effective_states : estado_atual
    catalog_scope_versions ||--o{ wallet_provisioning : escopo
    wallets {
        uuid wallet_id PK
        uuid parent_customer_wallet_id FK
        uuid item_id FK
        uuid provisioning_scope_version FK
    }
    customer_wallets {
        uuid wallet_id PK
    }
    item_wallets {
        uuid wallet_id PK
        uuid pending_price_version_id FK
        uuid last_usage_event_id FK
    }
    wallet_lifecycle_events {
        uuid wallet_lifecycle_event_id PK
        uuid wallet_id FK
    }
    wallet_effective_states {
        uuid wallet_id PK
    }
    wallet_provisioning {
        uuid customer_id PK
        uuid scope_version PK
    }
```

`wallets.parent_customer_wallet_id` é autorreferência e deve apontar para a carteira do mesmo cliente do tipo `CUSTOMER`; a regra é validada por trigger. `wallets.customer_id` e `wallet_provisioning.customer_id` são identificadores externos de cliente/workspace, sem FK local. `item_wallets.pending_price_version_id` referencia `price_versions`; `last_usage_event_id` referencia `usage_events` (mostrado também no diagrama de consumo).

### Créditos e razão da carteira

```mermaid
erDiagram
    workspace_projections ||--o| workspace_billing_configs : configura
    customer_wallets ||--o{ customer_wallet_entries : lancamentos
    customer_wallet_entries ||--o| credit_lots : origem
    customer_wallet_entries ||--o{ wallet_transaction_references : referencias
    direct_credits o|--o{ wallet_transaction_references : referencia_direta
    credit_lots o|--o{ wallet_transaction_references : referencia_lote
    customer_plans o|--o{ wallet_transaction_references : plano
    customer_plan_cycles o|--o{ wallet_transaction_references : ciclo
    subscription_plan_versions o|--o{ wallet_transaction_references : versao_plano
    usage_events o|--o{ wallet_transaction_references : uso
    debits o|--o{ wallet_transaction_references : debito
    products o|--o{ wallet_transaction_references : produto
    items o|--o{ wallet_transaction_references : item
    item_wallets o|--o{ wallet_transaction_references : carteira_item
    customer_wallet_entries {
        uuid customer_wallet_entry_id PK
        uuid customer_wallet_id FK
    }
    credit_lots {
        uuid credit_lot_id PK
        uuid granting_entry_id FK
    }
    wallet_transaction_references {
        uuid wallet_transaction_reference_id PK
        uuid customer_wallet_entry_id FK
        uuid direct_credit_id FK
        uuid credit_lot_id FK
        uuid customer_plan_id FK
        uuid customer_plan_cycle_id FK
        uuid plan_version_id FK
        uuid usage_event_id FK
        uuid debit_id FK
        uuid product_id FK
        uuid item_id FK
        uuid item_wallet_id FK
    }
    direct_credits {
        uuid direct_credit_id PK
    }
    workspace_billing_configs {
        uuid workspace_id PK
    }
```

`wallet_transaction_references` é uma referência tipada/polimórfica: `reference_kind` exige exatamente o campo correspondente (por exemplo, `DIRECT_CREDIT`, `CUSTOMER_PLAN_CYCLE`, `USAGE_EVENT` ou `ITEM_WALLET`). A tabela tem FKs para os alvos listados, mas cada linha preenche apenas o alvo selecionado. `direct_credits.customer_id` e `credit_lots.customer_id` são IDs externos; a origem do lote é vinculada ao lançamento em `granting_entry_id`.

### Assinaturas, ciclos e admissão

```mermaid
erDiagram
    subscriptions ||--o{ subscription_plan_versions : versiona
    subscriptions ||--o{ on_demand_plans : oferece
    subscription_plan_versions ||--o{ subscription_plan_products : inclui
    products ||--o{ subscription_plan_products : elegivel
    subscription_plan_versions ||--o{ customer_plans : contratado
    subscriptions ||--o{ active_customer_plan_slots : limita
    customer_plans ||--o| active_customer_plan_slots : ocupa
    customer_plans ||--o{ customer_plan_cycles : ciclos
    customer_plans ||--o{ customer_plan_entitlements : direitos
    products ||--o{ customer_plan_entitlements : concede
    customer_plans ||--o{ customer_plan_transitions : transicoes
    subscription_plan_versions ||--o{ customer_plan_transitions : versao_anterior
    subscription_plan_versions ||--o{ customer_plan_transitions : versao_nova
    credit_lots ||--o{ credit_lot_reclassifications : reclassifica
    customer_plan_cycles ||--o{ credit_lot_reclassifications : ciclo
    customer_plan_transitions ||--o{ credit_lot_reclassifications : transicao
    customer_plan_cycles ||--o| subscription_calendar_jobs : agenda
    subscription_admission_policies ||--o{ subscription_plan_versions : politica_publicada
    subscription_admission_policies ||--o{ subscription_admission_evidence : valida
    workspace_projections ||--o{ subscription_admission_evidence : evidencia
    customer_plans ||--o{ subscription_admission_decisions : decisao
    customer_plan_transitions o|--o{ subscription_admission_decisions : transicao_avaliada
    subscription_plan_versions ||--o{ subscription_admission_decisions : plano_avaliado
    subscription_admission_evidence ||--o{ subscription_admission_decisions : evidência_usada
    subscriptions {
        uuid subscription_id PK
    }
    subscription_plan_versions {
        uuid plan_version_id PK
        uuid subscription_id FK
        uuid admission_policy_version_id FK
    }
    subscription_plan_products {
        uuid plan_version_id PK
        uuid product_id PK
    }
    on_demand_plans {
        uuid on_demand_plan_id PK
        uuid subscription_id FK
    }
    customer_plans {
        uuid customer_plan_id PK
        uuid plan_version_id FK
    }
    active_customer_plan_slots {
        uuid customer_id PK
        uuid subscription_id PK
        uuid customer_plan_id FK
    }
    customer_plan_cycles {
        uuid customer_plan_cycle_id PK
        uuid customer_plan_id FK
    }
    customer_plan_entitlements {
        uuid customer_plan_entitlement_id PK
        uuid customer_plan_id FK
        uuid product_id FK
    }
    customer_plan_transitions {
        uuid plan_transition_id PK
        uuid customer_plan_id FK
        uuid previous_plan_version_id FK
        uuid new_plan_version_id FK
    }
    credit_lot_reclassifications {
        uuid credit_lot_reclassification_id PK
        uuid credit_lot_id FK
        uuid customer_plan_cycle_id FK
        uuid plan_transition_id FK
    }
    subscription_calendar_jobs {
        uuid customer_plan_cycle_id PK
    }
    subscription_admission_policies {
        uuid policy_version_id PK
    }
    subscription_admission_evidence {
        uuid event_id PK
        uuid workspace_id FK
        uuid policy_version_id FK
    }
    subscription_admission_decisions {
        uuid decision_id PK
        uuid customer_plan_id FK
        uuid plan_transition_id FK
        uuid plan_version_id FK
        uuid evidence_event_id FK
    }
```

`customer_plans.customer_id` e `active_customer_plan_slots.customer_id` identificam o workspace/cliente fornecido por Accounts; não há FK para `workspace_projections`. O slot ativo materializa no máximo um plano vigente por cliente e assinatura e é mantido por trigger para planos `ACTIVE`/`ACTIVE_PAID` com renovação `CURRENT`. A agenda também é materializada por trigger a partir de ciclos ativos de planos gratuitos. `subscription_plan_versions` é uma versão publicada e imutável; o vínculo com política de admissão é opcional e permitido quando a admissão exige aprovação.

### Medição de uso

```mermaid
erDiagram
    item_wallets ||--o{ usage_events : recebe
    products ||--o{ usage_events : identifica
    items ||--o{ usage_events : mede
    price_versions o|--o{ usage_events : preco_esperado
    usage_events ||--o| debits : debita
    customer_wallet_entries ||--o| debits : lancamento_debito
    item_wallets ||--o{ item_wallet_entries : historico
    usage_events ||--o| item_wallet_entries : registra
    debits o|--o{ item_wallet_entries : associa
    customer_wallet_entries o|--o{ item_wallet_entries : associa
    item_wallets ||--o{ billing_blocks : bloqueios
    items ||--o{ billing_blocks : item
    price_versions ||--o{ billing_blocks : preco_aplicado
    usage_events ||--o{ billing_blocks : origem
    item_wallet_entries ||--o{ billing_blocks : lancamento_item
    debits ||--o{ billing_blocks : debito
    customer_wallet_entries ||--o{ billing_blocks : lancamento_cliente
    items ||--o{ pricing_accumulators : acumulador
    price_versions ||--o{ pricing_accumulators : preco
    debits ||--o{ credit_lot_allocations : aloca
    credit_lots ||--o{ credit_lot_allocations : consumido
    usage_events {
        uuid usage_event_id PK
        uuid item_wallet_id FK
        uuid product_id FK
        uuid item_id FK
        uuid expected_price_version_id FK
    }
    debits {
        uuid debit_id PK
        uuid usage_event_id FK
        uuid customer_wallet_entry_id FK
    }
    item_wallet_entries {
        uuid item_wallet_entry_id PK
        uuid item_wallet_id FK
        uuid usage_event_id FK
        uuid debit_id FK
        uuid customer_wallet_entry_id FK
    }
    pricing_accumulators {
        uuid pricing_accumulator_id PK
        uuid item_id FK
        uuid price_version_id FK
    }
    billing_blocks {
        uuid billing_block_id PK
        uuid item_wallet_id FK
        uuid item_id FK
        uuid price_version_id FK
        uuid usage_event_id FK
        uuid item_wallet_entry_id FK
        uuid debit_id FK
        uuid customer_wallet_entry_id FK
    }
    credit_lot_allocations {
        uuid credit_lot_allocation_id PK
        uuid debit_id FK
        uuid credit_lot_id FK
    }
```

`item_wallets.last_usage_event_id` também referencia `usage_events`. `usage_events.customer_id`, `pricing_accumulators.customer_id` e os campos de cliente em outras tabelas são escopos externos sem FK local. A aceitação do evento, a atualização dos medidores, o débito na carteira do cliente e as alocações de lotes são gravados transacionalmente.

### Billing, pagamentos e conciliação

```mermaid
erDiagram
    workspace_projections ||--o{ billing_connections : conecta
    billing_connections ||--o{ payment_method_bindings : tokeniza
    customer_plans o|--o{ payment_method_bindings : plano_associado
    payment_method_bindings ||--o{ collection_requests : metodo
    customer_plans o|--o{ collection_requests : cobra_plano
    subscription_plan_versions o|--o{ collection_requests : snapshot_plano
    on_demand_plans o|--o{ collection_requests : snapshot_on_demand
    collection_requests ||--o{ collection_attempts : tentativas
    collection_requests ||--o{ billing_payments : pagamentos
    collection_attempts ||--o| billing_payments : resultado
    customer_wallet_entries ||--o| billing_credit_grant_references : concessao
    collection_requests ||--o| billing_credit_grant_references : pedido_concedido
    billing_payments ||--o| billing_credit_grant_references : pagamento_confirmado
    billing_connections ||--o{ unmatched_payment_cases : origem
    workspace_projections ||--o{ unmatched_payment_cases : workspace
    customer_plans o|--o{ unmatched_payment_cases : candidato
    unmatched_payment_cases ||--o{ unmatched_payment_case_events : historico
    collection_requests ||--o| billing_plan_upgrade_contexts : upgrade
    subscription_plan_versions ||--o{ billing_plan_upgrade_contexts : plano_anterior
    billing_connections ||--o{ external_refund_observations : observa
    workspace_projections ||--o{ external_refund_observations : workspace
    workspace_projections {
        uuid workspace_id PK
    }
    billing_connections {
        uuid billing_connection_id PK
        uuid workspace_id FK
    }
    payment_method_bindings {
        uuid payment_method_binding_id PK
        uuid billing_connection_id FK
        uuid workspace_id FK
        uuid customer_plan_id FK
    }
    collection_requests {
        uuid collection_request_id PK
        uuid workspace_id FK
        uuid customer_plan_id FK
        uuid plan_version_id FK
        uuid on_demand_plan_id FK
        uuid payment_method_binding_id FK
    }
    collection_attempts {
        uuid collection_attempt_id PK
        uuid collection_request_id FK
    }
    billing_payments {
        uuid billing_payment_id PK
        uuid collection_request_id FK
        uuid collection_attempt_id FK
    }
    billing_webhook_inbox {
        uuid billing_webhook_inbox_id PK
    }
    billing_credit_grant_references {
        uuid billing_credit_grant_reference_id PK
        uuid customer_wallet_entry_id FK
        uuid collection_request_id FK
        uuid billing_payment_id FK
    }
    unmatched_payment_cases {
        uuid unmatched_payment_case_id PK
        uuid workspace_id FK
        uuid billing_connection_id FK
        uuid candidate_customer_plan_id FK
    }
    unmatched_payment_case_events {
        uuid unmatched_payment_case_event_id PK
        uuid unmatched_payment_case_id FK
    }
    billing_plan_upgrade_contexts {
        uuid collection_request_id PK
        uuid previous_plan_version_id FK
    }
    external_refund_observations {
        uuid external_refund_observation_id PK
        uuid billing_connection_id FK
        uuid workspace_id FK
    }
```

As FKs compostas de Billing preservam o escopo: uma conexão pertence ao mesmo workspace da forma de pagamento; plano, binding, cobrança e caso não conciliado precisam pertencer ao mesmo cliente/workspace. `billing_webhook_inbox` é uma inbox idempotente do provedor, sem FK para pagamento; a associação é resolvida durante o processamento por IDs e metadados do evento. `billing_credit_grant_references` registra, no máximo uma vez, a relação entre lançamento, pedido de cobrança e pagamento que concedeu créditos.

## Convenções e limites do modelo

- `workspace_id` e `customer_id` representam a identidade do workspace/cliente mantida por Accounts. Quando não há FK para `workspace_projections`, essa ausência é intencional: a API consome a identidade e os eventos de Accounts sem replicar a tabela proprietária de clientes.
- Relações desenhadas como linhas nos diagramas correspondem a FKs, salvo relações descritas explicitamente como conceituais. Campos como `aggregate_id`, `resource_id`, `actor_reference`, IDs dentro de `payload` e IDs de cliente sem FK são referências polimórficas ou externas, não relações garantidas pelo banco.
- Tabelas de razão, decisões de admissão, eventos de ciclo de vida e observações de provedor preservam histórico append-only por triggers. Catálogos publicados também têm proteção de imutabilidade.
- Para conferir a definição física exata e a evolução de cada campo, consulte os arquivos `migrations/*.up.sql` em ordem cronológica.
