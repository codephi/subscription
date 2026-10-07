# Integração SaaS com o Subscription

Guia de implementação para qualquer aplicação que precise vender assinaturas e consumir créditos. O sistema cliente usa `account_id`, IDs de oferta e contratos HTTP; não deve guardar chaves Stripe, criar cobranças no Stripe nem conceder créditos após o retorno do navegador.

> As rotas abaixo são as rotas implementadas pelo serviço neste repositório. Confirme o OpenAPI publicado pela instância alvo antes de implantar. O sandbox de checkout descrito em “Configuração” deve estar habilitado para criar compras de teste.

## 1. Configurar o Subscription

### 1.1 Banco e serviço

1. Configure PostgreSQL e execute as migrações no processo de inicialização do serviço. Não crie tabelas manualmente.
2. Configure `DATABASE_URL` com a URL PostgreSQL do ambiente.
3. Em cada ambiente, configure segredo de sessão, segredo de eventos de Accounts e os limites operacionais exigidos pela aplicação. Mantenha valores de produção em um gerenciador de segredos.
4. Para a integração de pagamento de teste, configure `BILLING_SANDBOX_ENABLED=true`, `STRIPE_SECRET_KEY=sk_test_…` e `STRIPE_WEBHOOK_SECRET=whsec_…`. Nunca use chaves `sk_live_` neste modo.
5. Publique o serviço e valide `GET /openapi.json` e `GET /health` no endereço configurado.

O cliente não recebe nem armazena os segredos Stripe. O Subscription possui a integração de pagamento e a referência de cliente do provedor.

### 1.2 Webhook Stripe

Para o checkout hospedado pela configuração sandbox do serviço, configure:

```text
POST {SUBSCRIPTION_BASE_URL}/v1/billing/webhooks/stripe
```

Use o segredo `whsec_…` entregue pelo Stripe (`STRIPE_WEBHOOK_SECRET` no sandbox). Para uma conexão Stripe independente, a rota é `POST /v1/billing/webhooks/{BILLING_CONNECTION_ID}`; use `webhook_url` devolvida pela configuração da conexão. O Subscription valida `Stripe-Signature` e registra os resultados de pagamento. Selecione `checkout.session.completed` para pagamentos hospedados e eventos de PaymentIntent usados por cobranças off-session, incluindo `payment_intent.succeeded`, `payment_intent.payment_failed`, `payment_intent.canceled` e `payment_intent.requires_action`. Faça primeiro a configuração no modo de teste e depois crie outro endpoint, com credenciais próprias, em produção.

A integração de account também requer eventos assinados de criação e ativação de conta para `POST /v1/internal/accounts/account-events`. O segredo compartilhado deve existir somente no Subscription e no serviço confiável que assina os eventos.

Assine os bytes exatos do JSON usando HMAC-SHA256 sobre `<unix_seconds>.<raw_body>`, codifique o digest em Base64 e envie `X-Runvibe-Timestamp` e `X-Runvibe-Signature: v1=<base64>`. O timestamp deve estar dentro da tolerância do serviço (300 segundos). Envie `account.created` com sequência `1`, depois `account.activated` com sequência `2` e `causation_id` igual ao ID do primeiro evento. Preserve os IDs e bytes da requisição ao repetir.

### 1.3 Catálogo comercial

Crie uma vez os registros comerciais pela API administrativa e guarde os IDs retornados em configuração do Subscription:

1. Um produto de uso com créditos estritos.
2. Um item de consumo para a unidade faturável (por exemplo, uma execução).
3. Uma versão de preço publicada: 1 unidade de consumo custa 1 crédito.
4. Uma assinatura comercial com planos `FREE` e `PAID`.
5. Um plano grátis com `granted_credit_units=10`, recorrência `NONE` e elegibilidade aberta.
6. Planos pagos mensais: `BRL 2000 / 100 créditos`, `BRL 4000 / 200 créditos` e `BRL 6000 / 400 créditos`, em unidades menores (centavos).
7. Uma oferta avulsa de uma unidade por `BRL 100`. A aplicação informa a quantidade inteira; o Subscription calcula preço e créditos totais. Quantidade válida: `1..=10000`.

Guarde os IDs retornados em configuração protegida do próprio Subscription, separados por ambiente. A criação do catálogo deve ser idempotente no provisionamento; não crie novos planos em cada cadastro de usuário.

Use os contratos abaixo na ordem indicada (substitua URLs e guarde cada UUID retornado). O serviço deve ser iniciado com as migrações aplicadas. Para uma instalação reproduzível, crie um produto e item, crie/publique seu preço unitário e ative item e produto antes de associá-los aos planos:

```http
POST /v1/products
{"name":"<APP_NAME>","description":"Consumo por créditos","usage_model":"CREDIT_METERED"}

POST /v1/products/<PRODUCT_ID>/items
{"name":"<UNIT_NAME>","parent_item_id":null,"unit_name":"<UNIT_NAME>","quantity_scale":"1"}

POST /v1/items/<ITEM_ID>/price-versions
{"pricing_model":"unit","unit_block_size":"1","credit_units":"1","effective_from":"<RFC3339_UTC>","effective_until":null,"accumulation_cycle":null,"tiers":[]}
POST /v1/price-versions/<PRICE_VERSION_ID>/publish
{}
GET /v1/items/<ITEM_ID>  # PATCH /v1/items/<ITEM_ID> com status ACTIVE e expected_version se necessário
GET /v1/products/<PRODUCT_ID> # PATCH /v1/products/<PRODUCT_ID> com status ACTIVE e expected_version se necessário

POST /v1/subscriptions
{"name":"<APP_NAME> créditos","subscription_model":"CREDIT_STRICT"}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/plans
{"name":"Teste grátis","commercial_model":"FREE","price_amount_minor":null,"currency":null,"recurrence":"NONE","admission_policy":"OPEN","accepted_payment_methods":[],"granted_credit_units":"10","product_ids":["<PRODUCT_ID>"]}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/plans
{"name":"Plano 100","commercial_model":"PAID","price_amount_minor":2000,"currency":"BRL","recurrence":"MONTHLY","admission_policy":"OPEN","accepted_payment_methods":["CARD"],"granted_credit_units":"100","product_ids":["<PRODUCT_ID>"]}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/on-demand-plans
{"name":"Crédito avulso","price_amount_minor":100,"currency":"BRL","credit_units":"1"}
```

Repita o plano pago para 200 créditos/R$ 40 (`price_amount_minor: 4000`) e 400 créditos/R$ 60 (`price_amount_minor: 6000`). Catálogo comercial publicado é versionado: para alterar preço ou franquia, publique novas versões e atualize os IDs da configuração; não altere os valores no processo cliente.

## 2. Trabalho da aplicação cliente

### 2.1 Identidade e ciclo de vida

- Associe exatamente um `account_id` do Subscription à conta interna. Persista a relação antes de habilitar consumo.
- Ao criar a conta, envie `account.created` e `account.activated`, assinados conforme o formato do serviço, com IDs de evento estáveis, sequência crescente e `correlation_id` estável.
- Crie o customer plan grátis com `Idempotency-Key` estável por account e transação estável. O Subscription concede a franquia inicial; não replique essa concessão localmente.
- Guarde `customer_plan_id` e IDs de produto/ofertas retornados. Nunca use username/email como `account_id`.

### 2.2 Consultar e consumir créditos

- Leia o customer plan: `GET /v1/accounts/{account_id}/customer-plans/{customer_plan_id}`.
- Leia elegibilidade: `GET /v1/accounts/{account_id}/products/{product_id}/eligibility`.
- Leia saldo e extrato: `GET /v1/accounts/{account_id}/customer-wallet/statement`.
- Antes de iniciar uma operação, consulte a elegibilidade para UX. A resposta é uma fotografia, não uma reserva.
- Para cobrar uma unidade, envie um comando de uso com `Idempotency-Key` estável por operação e `transaction_id` imutável. O Subscription revalida entitlement e saldo dentro da transação; só considere a operação cobrada quando a resposta confirmar o débito.
- Reenvie a mesma requisição com a mesma chave após timeout. Não gere uma chave nova até consultar o resultado por transação; isso pode representar uma segunda operação legítima.

### 2.3 Cobranças e mudanças de plano

- Leia as ofertas e valores do catálogo do Subscription, não mantenha preços comerciais duplicados na aplicação cliente.
- Recargas são compras de quantidade inteira. Passe `quantity` e a oferta unitária; não calcule saldo no cliente.
- O checkout inicial requer plano do cliente, tipo da compra e transação. Guarde o `checkout_id`; acompanhe com `GET /v1/accounts/{account_id}/checkouts/{checkout_id}`.
- Conceda créditos somente quando o estado do Subscription confirmar a cobrança. Uma URL de retorno do navegador não confirma pagamento.
- Um upgrade pago hospedado usa `POST /v1/accounts/{account_id}/checkouts` com `checkout_kind=PLAN_UPGRADE`, `target_plan_version_id` e `Idempotency-Key`. O checkout devolve uma URL hospedada; o plano e a franquia só mudam após confirmação. A rota de transição direta continua disponível para integrações que já possuem uma forma de pagamento verificada.
- Recusa/expiração não concede franquia. Após falha de renovação, mostre a situação pendente e permita regularização pelo endpoint de Billing. Não solicite dados de cartão diretamente na aplicação cliente.
- Cancelamento usa `POST /v1/accounts/{account_id}/customer-plans/{customer_plan_id}/cancel`. Mostre a data efetiva devolvida pelo Subscription; não apague nem invalide créditos avulsos já comprados.

### 2.4 Estados e repetição

Estados comerciais relevantes incluem `PENDING_INITIAL_PAYMENT`, `ACTIVE`, `ACTIVE_PAID`, `PAST_DUE` e `CANCELED`; pedidos de cobrança podem estar `SCHEDULED`, `PENDING`, `REQUIRES_ACTION`, `PAID`, `FAILED`, `EXPIRED` ou `CANCELED`. Trate estados desconhecidos como não confirmados e consulte novamente.

Use `Idempotency-Key` ASCII estável para cada ação lógica. Em timeout, repita exatamente o mesmo corpo com a mesma chave. Se o cliente perdeu a resposta, consulte o recurso persistido antes de iniciar uma operação com nova chave. Erros de validação e conflitos não devem ser repetidos sem corrigir o estado ou a entrada.

## 3. Contratos HTTP copiáveis

### 3.1 Configuração do cliente

```dotenv
SUBSCRIPTION_BASE_URL=https://subscription.example
SUBSCRIPTION_ACCOUNTS_WEBHOOK_SECRET=<segredo-compartilhado-apenas-entre-servicos>
SUBSCRIPTION_PRODUCT_ID=<uuid-do-produto>
SUBSCRIPTION_ITEM_ID=<uuid-do-item>
SUBSCRIPTION_FREE_PLAN_VERSION_ID=<uuid-plano-gratis>
SUBSCRIPTION_PLAN_100_VERSION_ID=<uuid-plano-100>
SUBSCRIPTION_PLAN_200_VERSION_ID=<uuid-plano-200>
SUBSCRIPTION_PLAN_400_VERSION_ID=<uuid-plano-400>
SUBSCRIPTION_TOPUP_PLAN_ID=<uuid-oferta-unitaria>
```

Não defina `STRIPE_SECRET_KEY` no processo cliente.

### 3.2 Inscrição gratuita

```http
POST /v1/accounts/{account_id}/customer-plans
Idempotency-Key: account:{account_id}:trial:v1
Content-Type: application/json

{"plan_version_id":"<SUBSCRIPTION_FREE_PLAN_VERSION_ID>","transaction_id":"account:<account_id>:trial:v1"}
```

Antes da inscrição, cada evento de account usa o mesmo envelope e a assinatura exigida pelo endpoint:

```json
{"event_id":"<uuid-created>","event_type":"account.created","schema_version":1,"aggregate_id":"<account_id>","sequence":1,"occurred_at":"<RFC3339_UTC>","account_id":"<account_id>","correlation_id":"<stable-uuid>","causation_id":null,"payload":{"account_id":"<account_id>"}}
```

Para a ativação use outro `event_id`, `event_type:"account.activated"`, `sequence:2` e `causation_id:"<uuid-created>"`. O HMAC cobre os bytes exatos do JSON enviado; não reserialize entre assinar e transmitir.

Resposta `201 Created` (campos omitidos aqui são específicos do estado do ciclo):

```json
{
  "customer_plan_id": "<uuid>",
  "account_id": "<account_id>",
  "plan_version_id": "<uuid-do-plano-gratis>",
  "commercial_status": "ACTIVE",
  "activation_status": "ACTIVATED"
}
```

Guarde `customer_plan_id`. Para uma requisição repetida após timeout, use o mesmo corpo, a mesma transação e a mesma chave. O Subscription trata o evento de account e a inscrição como operações idempotentes independentes.

### 3.3 Elegibilidade, carteira e uso

```http
GET /v1/accounts/{account_id}/products/{product_id}/eligibility
GET /v1/accounts/{account_id}/customer-wallet/statement?limit=50
```

Registre consumo em `POST /v1/accounts/{account_id}/usage-events`:

```http
POST /v1/accounts/{account_id}/usage-events
Idempotency-Key: task:<operation_id>:v1
Content-Type: application/json

{
  "transaction_id": "task:<operation_id>:v1",
  "product_id": "<SUBSCRIPTION_PRODUCT_ID>",
  "item_id": "<SUBSCRIPTION_ITEM_ID>",
  "item_units": "1",
  "expected_price_version_id": "<SUBSCRIPTION_PRICE_VERSION_ID>",
  "occurred_at": null,
  "metadata": {"operation_id": "<operation_id>"}
}
```

`item_units` é decimal serializado como string; para esta integração, uma tarefa envia `"1"`.

### 3.4 Checkout e consulta

Quando a conta começa diretamente num plano pago (sem customer plan grátis anterior), crie primeiro o customer plan pago pendente e então inicie `INITIAL`. Para a experiência que começa pelo plano grátis, mantenha esse customer plan e use `PLAN_UPGRADE` após escolha do usuário.

```http
POST /v1/accounts/{account_id}/customer-plans
Idempotency-Key: account:<account_id>:paid-plan:v1
Content-Type: application/json

{"plan_version_id":"<SUBSCRIPTION_PLAN_100_VERSION_ID>","transaction_id":"account:<account_id>:paid-plan:v1"}

POST /v1/accounts/{account_id}/checkouts
Idempotency-Key: checkout:<operation_id>:initial:v1
Content-Type: application/json

{"customer_plan_id":"<customer_plan_id>","checkout_kind":"INITIAL","transaction_id":"initial:<operation_id>:v1","success_url":"https://client.example/billing/return","cancel_url":"https://client.example/billing/cancelled"}
```

```http
POST /v1/accounts/{account_id}/checkouts
Idempotency-Key: checkout:<operation_id>:v1
Content-Type: application/json

{
  "customer_plan_id": "<customer_plan_id>",
  "checkout_kind": "ON_DEMAND",
  "on_demand_plan_id": "<SUBSCRIPTION_TOPUP_PLAN_ID>",
  "quantity": 25,
  "transaction_id": "topup:<operation_id>:v1",
  "coupon_code": null,
  "success_url": "https://client.example/billing/return",
  "cancel_url": "https://client.example/billing/cancelled"
}
```

Valores válidos de `checkout_kind`: `INITIAL`, `ON_DEMAND`, `PLAN_UPGRADE`. `quantity` é opcional e assume `1`; para recarga, use inteiro de `1` a `10000`. Para checkout hospedado, omita `payment_method_binding_id` e informe URLs absolutas HTTPS (HTTP somente em loopback). Dinheiro usa `amount_minor` (`BRL 2000` significa `R$ 20,00`); créditos são inteiros. A resposta contém `checkout_id`, `collection_request_id`, `status`, `amount_minor`, `currency`, `granted_credit_units` e `redirect_url`. Redirecione o usuário para `redirect_url` e consulte o Subscription depois do retorno; o `session_id` fornecido pelo navegador não confirma pagamento.

```http
GET /v1/accounts/{account_id}/checkouts/<checkout_id>
```

O checkout hospedado devolve `redirect_url`; a aplicação cliente redireciona o usuário e depois consulta o checkout. A sessão é modo pagamento e salva o método para renovações futuras na mesma compra. Se a resposta inicial se perder, repita a mesma requisição com o mesmo `Idempotency-Key`; não crie outra compra para o mesmo intento. O webhook confirmado, e não o retorno do navegador, autoriza créditos.

### 3.5 Upgrade, regularização e cancelamento

```http
POST /v1/accounts/{account_id}/checkouts
Idempotency-Key: upgrade:<operation_id>:v1
Content-Type: application/json

{
  "customer_plan_id": "<customer_plan_id>",
  "checkout_kind": "PLAN_UPGRADE",
  "target_plan_version_id": "<SUBSCRIPTION_PLAN_200_VERSION_ID>",
  "transaction_id": "upgrade:<operation_id>:v1",
  "success_url": "https://client.example/billing/return",
  "cancel_url": "https://client.example/billing/cancelled"
}
```

O Subscription calcula e cobra somente a diferença entre o preço atual e o destino. A concessão da diferença de créditos e a mudança de ciclo ocorrem após confirmação verificada. Para regularizar um plano em `PAST_DUE`, envie:

```http
POST /v1/accounts/{account_id}/customer-plans/{customer_plan_id}/renewal-regularizations
Idempotency-Key: regularize:<operation_id>:v1
Content-Type: application/json

{"transaction_id":"regularize:<operation_id>:v1"}
```

`payment_method_binding_id` é opcional; omitindo-o, o Subscription escolhe o método ativo do plano. A resposta inicial é `CollectionRequestResponse` e pode estar `SCHEDULED`. Consulte o estado final com:

```http
GET /v1/accounts/{account_id}/collection-requests/{collection_request_id}
```

A resposta contém os snapshots da cobrança (`amount_minor`, `currency`, `granted_credit_units`, `quantity`), `status`, `scheduled_at` e `payment_expires_at`. Para cancelar, chame `POST /v1/accounts/{account_id}/customer-plans/{customer_plan_id}/cancel` e mantenha o estado e a data efetiva devolvidos pelo Subscription.

## 4. Critérios de aceite ponta a ponta

Use account e contas novos em ambiente de teste. Inspecione respostas API, carteira e extrato após cada etapa.

1. **Cadastro:** ativar account, inscrever plano gratuito uma vez; confirmar 10 créditos de origem trial no extrato. Repetir o evento e a inscrição com a mesma chave; saldo não pode duplicar.
2. **Uso e bloqueio:** executar 10 operações idempotentes de 1 crédito. Confirmar saldo zero; a operação 11 deve ser recusada sem criar débito. Repetir uma operação já concluída com a mesma chave; não deve haver novo débito.
3. **Compra:** criar ou usar um customer plan pago pendente e iniciar checkout `INITIAL`; confirmar que antes do webhook não há franquia paga. Após confirmação verificada, conferir a concessão conforme a versão do plano e o snapshot de valor/currency.
4. **Recarga:** comprar 25 unidades da oferta de R$ 1. Conferir cobrança de `2500 BRL` e acréscimo de 25 créditos depois da confirmação; uma quantidade inválida deve falhar sem ledger.
5. **Upgrade:** de 100 para 200 créditos, confirmar cobrança somente da diferença definida pelos snapshots e concessão da diferença de franquia; saldo disponível antes do upgrade deve ser preservado. Próxima renovação tem preço integral do destino.
6. **Renovação:** no limite mensal, confirmar somente uma cobrança/ciclo apesar de múltiplas execuções do dispatcher/reinício. Créditos mensais antigos expiram no fim do ciclo; saldo avulso persiste.
7. **Pagamento recusado:** use no checkout de teste o cartão de recusa genérica `4000 0000 0000 0002`, data futura e CVC de três dígitos. O Subscription não concede créditos antes da confirmação e mostra o checkout recusado. Para simular recusa de renovação, use as ferramentas de teste do Stripe numa cobrança off-session; não altere a carteira manualmente. Consulte a [lista de cartões de teste do Stripe](https://docs.stripe.com/testing?numbers-or-method-or-token=tokens).
8. **Regularização:** quitar a pendência e conferir um único ciclo/franquia, sem duplicar a confirmação se o webhook for reenviado.
9. **Cancelamento:** solicitar cancelamento e verificar o fim do ciclo efetivo. No limite, confirmar a transição de estado, sem uma nova renovação; conferir que saldo avulso continua registrado.
10. **Isolamento e falhas:** repetir chaves em dois accounts, concorrer com duas operações sobre o último crédito, reiniciar o serviço entre eventos e simular falha antes do commit. Nenhuma conta pode afetar a outra; débito/crédito deve ser aplicado uma vez ou não aplicado.

Guarde IDs de account, plano, checkout e transação (sem segredos) para que o roteiro seja reproduzível. Execute primeiro contra Stripe test mode; produção exige webhook ativo, credenciais isoladas e confirmação do fluxo hospedado na versão implantada.
