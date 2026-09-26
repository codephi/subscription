# Modelo de assinatura: mensalidade e créditos por ciclo

Este procedimento vende um plano `PAID` recorrente. Cada pagamento confirmado
ativa ou renova o direito aos produtos e pode conceder uma franquia de créditos
para execuções. O preço em dinheiro do plano e o preço em créditos de cada
execução são regras diferentes. Uma conta do SaaS corresponde a um
`workspace_id` da Subscription API; os workspaces de infraestrutura internos
compartilham a carteira dessa conta. Veja o [mapeamento comum](README.md).

Os exemplos usam `<API>`, `<WORKSPACE_ID>`, `<PRODUCT_ID>`, `<ITEM_ID>`,
`<SUBSCRIPTION_ID>`, `<PLAN_VERSION_ID>`, `<CUSTOMER_PLAN_ID>`,
`<CONNECTION_ID>` e `<BINDING_ID>` como valores substituíveis.

## 1. Definir a oferta antes de receber clientes

1. Escolha a política: preço mensal em moeda, franquia de créditos por ciclo,
   produtos liberados e custo em créditos de cada execução. Exemplo: R$ 99,00
   mensais, 10.000 créditos por ciclo, execução de agente a 20 créditos.
2. Crie e ative os produtos `CREDIT_METERED`, seus itens e preços de uso como
   no [passo 1 do modelo de créditos](credito.md#1-configurar-o-catálogo-uma-vez-por-ambiente).
   O preço de uso é uma `PriceVersion` de um `Item`; a mensalidade é um campo
   da versão do plano comercial. Publique o preço e ative item e produto antes
   de criar o plano.
3. Crie uma `Subscription` `CREDIT_STRICT` e uma versão de plano `PAID`. Use
   `price_amount_minor` positivo, moeda ISO em maiúsculas, `recurrence` como
   `MONTHLY`, `accepted_payment_methods=["CARD"]` e os IDs dos produtos
   ativos. O exemplo concede 10.000 créditos por ciclo. Use `"0"` se a
   assinatura apenas der acesso e o cliente comprar todos os créditos à parte.

```http
POST /v1/subscriptions
{"name":"Infra Pro","subscription_model":"CREDIT_STRICT"}

POST /v1/subscriptions/<SUBSCRIPTION_ID>/plans
{"name":"Infra Pro mensal","commercial_model":"PAID","price_amount_minor":9900,"currency":"BRL","recurrence":"MONTHLY","admission_policy":"OPEN","accepted_payment_methods":["CARD"],"granted_credit_units":"10000","product_ids":["<PRODUCT_ID>"]}
```

4. Se houver recargas extras, crie `OnDemandPlan` na mesma `Subscription`; a
   conta poderá comprar créditos extras após sua primeira cobrança confirmar.
   Registre o ID da versão do plano e dos produtos na configuração do SaaS.

**Admin-ui atual:** `Catálogo` cria produto, item, preço, Subscription, plano e
pacotes avulsos, publica preços e ativa produtos. O painel permite ver e revogar
versões do plano. A adesão de um cliente específico ainda é feita pela API.

## 2. Preparar Stripe para cobrança recorrente

1. No [Dashboard Stripe](https://dashboard.stripe.com/), obtenha a chave
   secreta para a Subscription API e a chave publicável para Stripe.js no SaaS.
   Configure `STRIPE_SECRET_KEY` no ambiente do serviço e mantenha ambientes
   de teste e produção independentes. Não crie Stripe Product/Price/Subscription
   para representar esta oferta: o domínio comercial e o calendário ficam na
   Subscription API; o Stripe recebe `SetupIntent` e `PaymentIntent`.
2. Para cada conta pagante, crie um [Stripe Customer](https://docs.stripe.com/api/customers/create)
   e persista o `cus_...` no SaaS. A Subscription API ainda não cria Customer.
3. Depois de ativar a conta no serviço, crie sua `BillingConnection` com esse
   `cus_...` como `external_account_reference`. Guarde o
   `billing_connection_id` e `webhook_path` retornados.

```http
POST /v1/workspaces/<WORKSPACE_ID>/billing-connections
{"provider":"STRIPE","external_account_reference":"cus_...","secret_reference":"env://STRIPE_SECRET_KEY","webhook_secret_reference":"env://STRIPE_WEBHOOK_SECRET_001"}
```

4. No Stripe, crie um destino HTTPS que aponte para
   `https://<HOST_PUBLICO_DA_API><webhook_path>` e selecione
   `payment_intent.succeeded`, `payment_intent.payment_failed`,
   `payment_intent.canceled` e, para observação de estornos externos,
   `charge.refunded`. Copie o `whsec_...` específico do destino para a
   variável referenciada pela conexão. Teste a assinatura e o recebimento.

O caminho de webhook por conta não escala manualmente; há também bloqueios no
adaptador do cartão salvo e na renovação paga. Leia os
[bloqueios para produção](README.md#limites-que-bloqueiam-uma-ativação-comercial-sem-intervenção)
antes de anunciar cobrança automática. O `admin-ui` cadastra a conexão por
referências, mas a configuração de Customer, chaves e webhook ocorre no Stripe
ou por automação externa.

## 3. Ativar a conta e preparar a carteira

1. O SaaS cria uma conta com UUID estável. Envie para
   `POST /v1/internal/accounts/workspace-events` os eventos assinados
   `workspace.created` (`sequence=1`) e `workspace.activated` (`sequence=2`).
   `aggregate_id`, `workspace_id` e `payload.workspace_id` devem coincidir.
   O [índice](README.md#preparação-comum-do-serviço) descreve a assinatura; o
   [schema](../contracts/accounts-workspace-event-v1.schema.json) lista todos
   os campos obrigatórios.
2. Confirme `GET /v1/workspaces/<WORKSPACE_ID>/wallet-provisioning` com status
   `ACTIVE` e `GET /v1/workspaces/<WORKSPACE_ID>/wallets` com `ready=true`.
   Se o catálogo mudou depois do provisionamento, reconcilie a hierarquia.
3. Consulte `GET /v1/workspaces/<WORKSPACE_ID>/billing-config`. A opção
   `recurring_credit_enabled` precisa ser `true` para criar o CustomerPlan;
   se estiver `false`, use `PUT /v1/workspaces/<WORKSPACE_ID>/billing-config`
   com `expected_version` da leitura, mantendo o valor desejado de
   `direct_credit_enabled`.

**Admin-ui atual:** mostra estado do workspace, carteira e provisionamento;
`Workspaces > Ações` ajusta `billing-config` e reconcilia a wallet. O evento
de Accounts deve partir do backend do SaaS/Accounts.

## 4. Criar a assinatura do cliente e salvar o cartão

1. Após o usuário selecionar a oferta, crie o CustomerPlan com uma chave
   estável. Guarde `customer_plan_id`. A resposta paga deve indicar
   `activation_status=PENDING_INITIAL_PAYMENT`: ainda não há direito nem
   franquia liberados.

```http
POST /v1/workspaces/<WORKSPACE_ID>/customer-plans
Idempotency-Key: account:<WORKSPACE_ID>:pro-plan
{"plan_version_id":"<PLAN_VERSION_ID>","transaction_id":"account:<WORKSPACE_ID>:pro-plan"}
```

2. Seu backend chama a sessão de setup com `return_url` HTTPS. Entregue o
   `client_secret` ao cliente certo; o frontend usa
   [Stripe.js/Elements para confirmar o SetupIntent](https://docs.stripe.com/payments/save-and-reuse?platform=web&ui=embedded-form).
   O cartão é coletado pelo Stripe. Depois do resultado `succeeded`, o backend
   verifica no Stripe o Customer e o PaymentMethod `pm_...` e registra o vínculo
   com o CustomerPlan.
   A criação de SetupIntent da API precisa de correção antes deste passo: ela
   envia `return_url` sem `confirm=true`, que o Stripe não aceita na criação.

```http
POST /v1/workspaces/<WORKSPACE_ID>/billing-connections/<CONNECTION_ID>/payment-method-setup-sessions
{"return_url":"https://seu-saas.example/billing/return"}

POST /v1/workspaces/<WORKSPACE_ID>/payment-method-bindings
{"billing_connection_id":"<CONNECTION_ID>","customer_plan_id":"<CUSTOMER_PLAN_ID>","provider_payment_method_reference":"pm_..."}
```

3. A API atual não valida o SetupIntent ao criar o binding. Não trate apenas
   o `pm_...` ou o redirect como confirmação. O fluxo de webhook atual também
   não ativa plano FREE que exija cartão; este guia usa plano PAID.

**Admin-ui atual:** visualiza o CustomerPlan, mas não cria adesão, sessão de
setup ou vínculo de método de pagamento.

## 5. Solicitar e confirmar a primeira mensalidade

1. Seu backend cria a solicitação inicial uma única vez. A resposta `202`
   contém `collection_request_id`, valor/moeda, estado e prazo de pagamento;
   `202` significa cobrança agendada, não pagamento confirmado.

```http
POST /v1/workspaces/<WORKSPACE_ID>/customer-plans/<CUSTOMER_PLAN_ID>/collection-requests
Idempotency-Key: initial:<CUSTOMER_PLAN_ID>
{"payment_method_binding_id":"<BINDING_ID>","transaction_id":"initial:<CUSTOMER_PLAN_ID>"}
```

2. O worker de Billing pega a solicitação quando `scheduled_at` chegar e
   cria um PaymentIntent idempotente no Stripe. A confirmação definitiva vem
   do webhook assinado `payment_intent.succeeded`. A API valida valor, moeda,
   método, plano e janela comercial antes de ativar o contrato e conceder a
   franquia do primeiro ciclo uma única vez.
3. Acompanhe
   `GET /v1/workspaces/<WORKSPACE_ID>/collection-requests/<COLLECTION_ID>` e
   `GET /v1/workspaces/<WORKSPACE_ID>/customer-plans/<CUSTOMER_PLAN_ID>`.
   Libere a infraestrutura quando `activation_status=ACTIVATED` e o produto
   aparecer entre `entitled_product_ids`. Confira a concessão em
   `GET /v1/workspaces/<WORKSPACE_ID>/wallets` e no extrato da carteira.
4. Se o Stripe exigir ação do cliente, a cobrança permanece pendente e a
   interface do SaaS precisa levá-lo ao fluxo de autenticação do mesmo
   PaymentIntent. A API atual não fornece um fluxo completo de retomada no
   navegador; esse é outro requisito antes de produção. Se houver timeout ou
   resposta incerta, investigue a tentativa existente e aguarde/reenvie o
   webhook original; não crie uma segunda cobrança por conta própria.
   Falha definitiva ou expiração da cobrança inicial encerra o CustomerPlan
   pendente sem liberar produto ou franquia; uma nova contratação exige nova
   intenção comercial. Um estorno externo observado não desfaz
   automaticamente os créditos nem o plano.

**Bloqueio atual:** o adaptador cria o PaymentIntent sem enviar o Customer
`cus_...` que possui o cartão salvo. Corrija e valide esse contrato com Stripe
antes da primeira cobrança real. Veja a
[referência Stripe](https://docs.stripe.com/api/payment_intents/create).

## 6. Cobrar execuções e acompanhar o ciclo

1. Cada execução usa `POST /v1/workspaces/<WORKSPACE_ID>/usage-events` com
   `product_id`, `item_id`, `item_units="1"`, `transaction_id` e
   `Idempotency-Key` estáveis por execução. Inclua o ID do workspace interno
   do SaaS em `metadata`. Só enfileire a execução após `201`.
2. O débito sai da carteira principal. `403` significa produto sem direito;
   `409 insufficient_credit` significa saldo insuficiente. A leitura de
   elegibilidade não reserva créditos. O
   [modelo de créditos](credito.md#5-cobrar-uma-execução-de-infraestrutura)
   traz um exemplo completo do payload.
3. Para comprar créditos extras sem mudar a assinatura, publique um
   `OnDemandPlan` na mesma `Subscription` e use o fluxo de compra do
   [guia de créditos](credito.md#4-salvar-cartão-e-comprar-créditos).

## 7. Renovação, falha e cancelamento

O comportamento pretendido é: na próxima data de ciclo, criar uma única
solicitação `RENEWAL`, cobrar o cartão salvo e, após webhook confirmado,
iniciar o próximo ciclo e conceder sua franquia uma vez. **O processo atual
despacha cobranças existentes, mas não gera automaticamente a solicitação de
renovação paga.** Implemente essa geração e valide concorrência/restart antes
de vender recorrência automática.

Uma falha definitiva de renovação leva o plano a `PAST_DUE` /
`RENEWAL_INACTIVE`. A regularização manual usa
`POST /v1/workspaces/<WORKSPACE_ID>/customer-plans/<CUSTOMER_PLAN_ID>/renewal-regularizations`
com `Idempotency-Key`, `payment_method_binding_id` e `transaction_id` novos;
só a confirmação reativa o ciclo. `POST .../cancel` agenda o cancelamento ao
fim do período confirmado. Não cria estorno automático. O `admin-ui` já permite
cancelamento, transição e revogação do plano, além da investigação de Billing;
ele não inicia regularização nem acompanha a ação de cartão do cliente.

## 8. Verificação operacional

Em Stripe teste, verifique: conta ativa, wallet pronta, plano pendente antes
do pagamento, uma cobrança inicial, webhook aplicado, uma ativação, uma
franquia e um débito por execução. Repita o webhook e a chave de execução;
confira ausência de efeito duplicado. Antes de liberar produção, verifique
também renovação, falha, autenticação adicional, expiração e
[runbook de investigação](../runbooks/billing-mvp.md).
