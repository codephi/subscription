# Modelo de créditos: recarga avulsa e débito por execução

Este procedimento usa um plano `FREE` de acesso, sem franquia, e pacotes
`OnDemandPlan` pagos. A conta só executa infraestrutura após comprar créditos.
Cada conta do SaaS corresponde a um `account_id` da Subscription API. Leia
também o [mapeamento e os bloqueios atuais](README.md).

Os exemplos usam `<API>`, `<ACCOUNT_ID>`, `<PRODUCT_ID>`, `<ITEM_ID>`,
`<SUBSCRIPTION_ID>`, `<PLAN_VERSION_ID>`, `<CUSTOMER_PLAN_ID>`,
`<ON_DEMAND_PLAN_ID>`, `<CONNECTION_ID>` e `<BINDING_ID>` como valores
substituíveis. Faça as chamadas de negócio a partir do backend do SaaS.

## 1. Configurar o catálogo uma vez por ambiente

1. Crie um produto para cada família de infraestrutura cobrada e um item para
   cada unidade faturável. Exemplo: produto `Infra de agentes`, item
   `Execução de agente`, `unit_name=execução`, `quantity_scale="1"`.
2. Crie uma versão de preço `unit`: bloco de `"1"` item unit e, por exemplo,
   `"20"` credit units. Use `effective_from` UTC já vigente quando as contas
   forem provisionadas. Publique a versão. Ative
   primeiro o item e depois o produto usando `expected_version` obtido pelo
   `GET` do recurso. Repita para `Infra de API` se sua cobrança for distinta.

```http
POST /v1/products
{"name":"Infra de agentes","description":"Execuções","usage_model":"CREDIT_METERED"}

POST /v1/products/<PRODUCT_ID>/items
{"name":"Execução de agente","parent_item_id":null,"unit_name":"execução","quantity_scale":"1"}

POST /v1/items/<ITEM_ID>/price-versions
{"pricing_model":"unit","unit_block_size":"1","credit_units":"20","effective_from":"<UTC>","effective_until":null,"accumulation_cycle":null,"tiers":[]}

POST /v1/price-versions/<PRICE_VERSION_ID>/publish
PATCH /v1/items/<ITEM_ID>
{"status":"ACTIVE","expected_version":<VERSAO_ATUAL>}
PATCH /v1/products/<PRODUCT_ID>
{"status":"ACTIVE","expected_version":<VERSAO_ATUAL>}
```

3. Crie uma `Subscription` comercial `CREDIT_STRICT`. Publique um plano
   `FREE`, `OPEN`, `NONE`, sem método de pagamento e com zero crédito concedido.
   Associe todos os produtos que a conta poderá consumir; o plano precisa de
   pelo menos um produto ativo `CREDIT_METERED`.
4. Na **mesma** `Subscription`, crie um `OnDemandPlan` para cada pacote de
   recarga. `price_amount_minor` é o preço em centavos para `BRL`, e
   `credit_units` é a quantidade que entra na carteira após confirmação.

```http
POST /v1/subscriptions
{"name":"Infra pré-paga","subscription_model":"CREDIT_STRICT"}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/plans
{"name":"Acesso pré-pago","commercial_model":"FREE","price_amount_minor":null,"currency":null,"recurrence":"NONE","admission_policy":"OPEN","accepted_payment_methods":[],"granted_credit_units":"0","product_ids":["<PRODUCT_ID>"]}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/on-demand-plans
{"name":"Pacote 1000 créditos","price_amount_minor":5000,"currency":"BRL","credit_units":"1000"}
```

**Admin-ui atual:** `Catálogo` permite criar produto, itens, preços, assinatura,
plano e pacote avulso, publicar preço e ativar o produto. O cadastro conectado
de produto evita copiar IDs entre seus itens e preços. Salve os IDs retornados
em configuração do SaaS. O painel ainda não cadastra um `CustomerPlan` para a
conta nem compra o pacote pelo cliente.

## 2. Preparar Stripe em teste e em produção

1. No [Dashboard Stripe](https://dashboard.stripe.com/), escolha o ambiente de
   teste. Obtenha uma chave secreta para o backend da Subscription API e uma
   chave publicável para o checkout do SaaS. Guarde `sk_...` no gerenciador de
   segredos e injete-a como `STRIPE_SECRET_KEY`; configure uma variável de
   segredo de webhook por destino, por exemplo `STRIPE_WEBHOOK_SECRET_001`,
   depois de criá-lo. Repita a
   configuração com chaves de produção no ambiente de produção.
2. Para **cada conta pagante**, crie um [Customer do Stripe](https://docs.stripe.com/api/customers/create)
   e persista seu `cus_...` junto ao `account_id` no SaaS. A API atual não cria
   esse Customer. `external_account_reference` da conexão deve receber esse
   `cus_...`; o `SetupIntent` usa o valor como parâmetro `customer`.

```http
POST https://api.stripe.com/v1/customers
Authorization: Bearer <STRIPE_SECRET_KEY>
Content-Type: application/x-www-form-urlencoded

description=Conta%20<ACCOUNT_ID>
```

3. Após a conta estar ativa na Subscription API, crie a conexão. Guarde
   `billing_connection_id` e o `webhook_path` da resposta.

```http
POST /v1/accounts/<ACCOUNT_ID>/billing-connections
{"provider":"STRIPE","external_account_reference":"cus_...","secret_reference":"env://STRIPE_SECRET_KEY","webhook_secret_reference":"env://STRIPE_WEBHOOK_SECRET_001"}
```

4. Configure no Stripe um destino HTTPS para
   `https://<HOST_PUBLICO_DA_API><webhook_path>`, com eventos
   `payment_intent.succeeded`, `payment_intent.payment_failed`,
   `payment_intent.canceled` e, se usado na operação, `charge.refunded`.
   Copie o segredo de assinatura **desse destino** (`whsec_...`) para a
   variável indicada por `webhook_secret_reference`. Um `whsec_` diferente
   exige uma variável de ambiente diferente na conexão correspondente.
5. Envie um evento de teste e confirme o recebimento no Stripe e na fila de
   webhooks da API. Use o payload original: a API valida `Stripe-Signature`
   sobre o corpo exato. Não emita confirmação de pagamento manualmente.

**Limite de escala:** a API expõe um `webhook_path` por conexão. O operador não
deve cadastrar manualmente um destino por cliente em um SaaS em produção. O
provisionamento automático ou uma entrada compartilhada de webhooks é trabalho
de backend indicado no [índice](README.md).

**Admin-ui atual:** `Accounts > Ações > Conexão Stripe` cadastra a conexão
com `cus_...` e referências `env://`. O painel não cria Customer, chaves ou
destinos no Stripe.

## 3. Ativar uma conta recém-criada no SaaS

1. Crie a conta no SaaS, gere um UUID estável e use-o como
   `<ACCOUNT_ID>` de cobrança. Os accounts internos de agentes e APIs
   continuam no banco do SaaS.
2. O serviço de contas envie dois eventos assinados e sequenciais para
   `POST /v1/internal/accounts/account-events`: `account.created` com
   `sequence=1` e depois `account.activated` com `sequence=2`. Cada evento
   usa um `event_id` novo e imutável; `aggregate_id`, `account_id` e
   `payload.account_id` têm o mesmo UUID. A assinatura HMAC e o schema estão
   no [índice](README.md) e no [contrato](../contracts/accounts-account-event-v1.schema.json).
3. Confira `GET /v1/accounts/<ACCOUNT_ID>/wallet-provisioning` e
   `GET /v1/accounts/<ACCOUNT_ID>/wallets`. Espere `ACTIVE`/`ready=true`;
   o segundo retorna `customer_wallet.wallet_id`, saldo e as `item_wallets`
   ligadas aos itens publicados. Se o escopo do catálogo mudou, use a ação de
   reconciliação antes de admitir o cliente.
4. Crie o `CustomerPlan` gratuito. Guarde `customer_plan_id`; o retorno deve
   estar `activation_status=ACTIVATED`. Esse plano concede o direito de usar os
   produtos, mas, com saldo zero, as execuções serão recusadas até uma recarga.
   Antes dessa chamada, confira `GET /v1/accounts/<ACCOUNT_ID>/billing-config`:
   `recurring_credit_enabled` deve ser `true`, inclusive nesse plano sem franquia.
   Se necessário, ajuste via `PUT` usando `expected_version` da leitura.

```http
GET /v1/accounts/<ACCOUNT_ID>/billing-config
PUT /v1/accounts/<ACCOUNT_ID>/billing-config
{"direct_credit_enabled":false,"recurring_credit_enabled":true,"expected_version":<VERSAO_LIDA>}
```

```http
POST /v1/accounts/<ACCOUNT_ID>/customer-plans
Idempotency-Key: account:<ACCOUNT_ID>:prepaid-plan
{"plan_version_id":"<PLAN_VERSION_ID>","transaction_id":"account:<ACCOUNT_ID>:prepaid-plan"}
```

**Admin-ui atual:** exibe account, provisionamento, carteiras e planos;
permite reconciliar o provisionamento. A criação de conta vem do SaaS/Accounts
e a adesão do cliente ao plano ainda exige API.

## 4. Salvar cartão e comprar créditos

1. Seu backend chama a sessão de setup associada ao CustomerPlan. A Subscription
   resolve a integração, cria uma sessão hospedada e devolve uma URL junto com
   um identificador opaco. O frontend redireciona o cliente à URL.
2. Após o retorno, envie `payment_method_setup_id` à API. A Subscription localiza
   a sessão e valida sucesso, Customer esperado e PaymentMethod associado antes
   de persistir o vínculo. O cliente não fornece referências do provedor.

```http
POST /v1/accounts/<ACCOUNT_ID>/payment-method-setup-sessions
{"customer_plan_id":"<CUSTOMER_PLAN_ID>","success_url":"https://app.example/return?payment_setup=complete","cancel_url":"https://app.example/return?payment_setup=cancelled"}

POST /v1/accounts/<ACCOUNT_ID>/payment-method-bindings
{"customer_plan_id":"<CUSTOMER_PLAN_ID>","payment_method_setup_id":"<PAYMENT_METHOD_SETUP_ID>"}
```

3. Quando o cliente escolher um pacote, gere uma intenção de compra única no
   SaaS. Chame o endpoint abaixo e grave `collection_request_id`. A resposta
   `201` cria uma cobrança `SCHEDULED`; o worker inicia o PaymentIntent depois.
   Repetições da mesma intenção reutilizam a chave e a transação.

```http
POST /v1/accounts/<ACCOUNT_ID>/customer-plans/<CUSTOMER_PLAN_ID>/on-demand-purchases
Idempotency-Key: topup:<ID_UNICO_DA_COMPRA>
{"on_demand_plan_id":"<ON_DEMAND_PLAN_ID>","payment_method_binding_id":"<BINDING_ID>","transaction_id":"topup:<ID_UNICO_DA_COMPRA>"}
```

4. O Stripe envia `payment_intent.succeeded` ao webhook. A API valida
   assinatura, conexão, valor, moeda e solicitação; só então confirma a compra
   e lança o lote de créditos na carteira. Acompanhe
   `GET /v1/accounts/<ACCOUNT_ID>/collection-requests/<COLLECTION_ID>` e
   `GET /v1/accounts/<ACCOUNT_ID>/wallets`. Somente mostre os créditos como
   disponíveis após a confirmação. Falha, expiração ou estado incerto não
   concede créditos; investigue a mesma cobrança, sem criar outra por timeout.
   Um `charge.refunded` externo é observado para investigação, mas não retira
   automaticamente créditos já concedidos.

**Bloqueio atual para cartão salvo:** o PaymentIntent ainda não recebe o
`customer` do Stripe. Corrija isso antes de executar a primeira recarga real;
veja [índice](README.md). O `admin-ui` atual mostra filas e registros de
Billing, mas não inicia setup nem a compra avulsa.

## 5. Cobrar uma execução de infraestrutura

1. O SaaS autentica a conta, identifica o account interno e cria um
   `execution_id` estável. Pode consultar
   `GET /v1/accounts/<ACCOUNT_ID>/products/<PRODUCT_ID>/eligibility`
   para exibir saldo e elegibilidade, sabendo que essa leitura não reserva saldo.
2. Antes de enfileirar a execução, registre uma unidade do item. Use a mesma
   chave e `transaction_id` para identificar um retry da **mesma** execução.
   Uma repetição já confirmada pode retornar `409` com referência à operação
   existente; consulte a transação antes de enfileirar de novo. Se o preço for
   `1 execução = 20 créditos`, o retorno terá `debited_credit_units="20"` e
   `balance_after_credit_units` atualizado.

```http
POST /v1/accounts/<ACCOUNT_ID>/usage-events
Idempotency-Key: execution:<EXECUTION_ID>
{"transaction_id":"execution:<EXECUTION_ID>","product_id":"<PRODUCT_ID>","item_id":"<ITEM_ID>","item_units":"1","expected_price_version_id":null,"occurred_at":null,"metadata":{"infra_account_id":"<ID_INTERNO>","execution_id":"<EXECUTION_ID>"}}
```

3. Só enfileire após `201`. `403` indica falta de direito ao produto; `409
   insufficient_credit` indica saldo insuficiente; `503` pode indicar carteira
   ainda não provisionada. A API revalida preço, plano e saldo dentro da
   transação, inclusive com execuções simultâneas.
4. Mostre o saldo a partir de `GET /v1/accounts/<ACCOUNT_ID>/wallets` e o
   histórico em `/customer-wallet/statement`. O SaaS mantém seu próprio índice
   por account interno. Defina a política para falha da infraestrutura depois
   do débito: a V1 ainda não tem reserva/liberação de créditos nem compensação
   automática dessa execução.

**Admin-ui atual:** permite consultar carteira, extrato, medidores e conciliar
uso. O registro de execução é uma chamada do backend do SaaS, não uma ação
manual do operador.

## 6. Verificação operacional

Confira um caso de teste completo: conta ativa, wallet pronta, plano FREE
`ACTIVATED`, cartão salvo, compra `PAID` após webhook, saldo aumentado uma vez,
execução aceita e saldo debitado uma vez. Repita o mesmo `event_id` do Stripe e
a mesma chave de execução para verificar deduplicação. Investigue divergências
em `/v1/admin/billing/operations`, no painel de Billing e no
[runbook](../runbooks/billing-mvp.md).
