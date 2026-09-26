# Integração do SaaS de infraestrutura com Subscription

Este diretório reúne os procedimentos para cobrar uma conta do SaaS por
execuções de infraestrutura. Leia o [modelo de créditos](credito.md) para
recargas avulsas, o [modelo de assinatura](assinatura.md) para cobrança
recorrente e o [plano do admin-ui](admin-ui-futuro.md) para a operação futura
pelo painel.

## Identidades e recursos

| No SaaS | Na Subscription API | Regra |
| --- | --- | --- |
| Conta pagante | `workspace_id` e `customer_id` iguais ao UUID dessa conta | Uma conta tem uma `customer_wallet` com saldo compartilhado. |
| Workspace de infraestrutura, como agentes ou APIs | ID mantido pelo SaaS | Identifica onde a execução ocorreu; pode ir em `metadata` do evento de uso. |
| Tipo de infraestrutura | `Product` publicado | O plano concede acesso ao produto. |
| Unidade cobrada, como uma execução | `Item` de um produto `CREDIT_METERED` | A `item_wallet` mede esse item para a conta; não guarda créditos próprios. |
| Regra de conversão | `PriceVersion` publicada | Exemplo: bloco de `1` execução custa `20` créditos. |
| Compra de créditos | `OnDemandPlan` da mesma `Subscription` comercial | Confirmação do pagamento credita a carteira principal. |
| Plano recorrente | `SubscriptionPlanVersion` `PAID` | Confirmação da cobrança ativa o plano e concede a franquia do ciclo. |

Um `workspace_id` da Subscription API representa **a conta de cobrança**, não
cada workspace de infraestrutura criado pelo cliente. Se todos usarem o mesmo
`product_id`/`item_id`, o medidor desse item agrega o uso da conta. O SaaS deve
guardar `infra_workspace_id`, `execution_id` e resultado da execução no próprio
banco; a Subscription API recebe essas referências no `transaction_id` e em
`metadata` para auditoria. Não há consulta por workspace interno ou limite de
créditos por workspace interno no contrato atual.

## Regras comuns de integração

1. O SaaS autentica o usuário e verifica a posse da conta. Somente o backend do
   SaaS chama as rotas de negócio da Subscription API.
2. O SaaS persiste a relação `account_id -> workspace_id da Subscription ->
   Stripe Customer cus_... -> customer_plan_id -> billing_connection_id`.
3. `credit_units` e `item_units` são strings decimais no JSON; valor monetário é
   inteiro em unidades menores da moeda, por exemplo `5000` para R$ 50,00.
4. Operações com `Idempotency-Key` recebem uma chave estável por intenção de
   negócio. Guarde também o `transaction_id`; em resposta perdida, consulte o
   recurso ou a transação antes de criar uma nova intenção.
5. A leitura de elegibilidade é apenas informativa. A aceitação de
   `POST /v1/workspaces/{workspace_id}/usage-events` é a decisão transacional
   que autoriza cobrar e iniciar uma execução.
6. Um retorno do navegador ou uma `CollectionRequest` `SCHEDULED`/`PENDING`
   não comprova pagamento. Confirme `PAID`/ativação e o lançamento na carteira.
7. O webhook de saída, quando configurado, entrega eventos ao menos uma vez.
   Consumidores do SaaS deduplicam por `event_id` e consultam o recurso da API
   para decidir o acesso. Configure `OUTBOUND_EVENT_WEBHOOK_URL` e
   `OUTBOUND_EVENT_WEBHOOK_SECRET` se for usar essa entrega.

## Preparação comum do serviço

1. Provisione PostgreSQL, aplique as migrações iniciando o serviço e confira
   `GET /health`, `/docs` e `/openapi.json`. O processo inicia os workers de
   calendário FREE, despacho de cobranças já agendadas e, se configurado,
   entrega da outbox.
2. Configure `ACCOUNTS_WEBHOOK_SECRET` para os eventos de criação/ativação da
   conta. O emissor assina **os bytes exatos do JSON** com HMAC-SHA256 sobre
   `<timestamp Unix em segundos>.<corpo>`. Envie
   `x-runvibe-timestamp: <timestamp>` e
   `x-runvibe-signature: v1=<HMAC em base64>`; a tolerância é de cinco minutos.
   Exemplo de envelope para a primeira mensagem (o evento `activated` recebe
   outro `event_id`, `sequence=2` e `event_type=workspace.activated`):

```json
{"event_id":"<UUID_DO_EVENTO>","event_type":"workspace.created","schema_version":1,"aggregate_id":"<WORKSPACE_ID>","sequence":1,"occurred_at":"<UTC>","workspace_id":"<WORKSPACE_ID>","correlation_id":"<UUID_DA_OPERACAO>","causation_id":null,"payload":{"workspace_id":"<WORKSPACE_ID>"}}
```

3. Configure as credenciais Stripe no gerenciador de segredos e injete-as no
   processo, por exemplo `STRIPE_SECRET_KEY` e `STRIPE_WEBHOOK_SECRET`. A API
   armazena somente `env://STRIPE_SECRET_KEY` e
   `env://STRIPE_WEBHOOK_SECRET` na conexão. Com um destino por conexão, cada
   segredo de assinatura usa uma variável própria. A chave publicável Stripe fica no
   frontend do SaaS; a chave secreta nunca vai para o navegador.
4. Separe ambientes de teste e produção: chaves `sk_test_`/`pk_test_`, Customer,
   destinos de webhook e `whsec_` de teste não substituem os de produção.
5. Mantenha as rotas comuns e administrativas em rede confiável até adicionar
   autenticação de serviço e autorização por `workspace_id`. O `admin-ui`
   atual também não tem login.

Referências Stripe: [chaves de API](https://docs.stripe.com/keys),
[criar Customer](https://docs.stripe.com/api/customers/create),
[SetupIntent](https://docs.stripe.com/api/setup_intents),
[PaymentIntent](https://docs.stripe.com/api/payment_intents/create) e
[destinos de webhook](https://docs.stripe.com/api/webhook_endpoints).

## Limites que bloqueiam uma ativação comercial sem intervenção

Os guias abaixo descrevem as chamadas disponíveis e o fluxo pretendido. Antes
de cobrar clientes reais automaticamente, resolva e valide estes pontos:

1. O adaptador envia `return_url` ao **criar** o SetupIntent sem `confirm=true`.
   A [API Stripe](https://docs.stripe.com/api/setup_intents/create) aceita esse
   parâmetro na criação somente com `confirm=true`. Ajuste o adaptador para
   deixar o `return_url` na confirmação feita por Stripe.js, ou use um fluxo
   de confirmação compatível, antes de contar com a sessão de setup.
2. O adaptador cria `SetupIntent` com `customer=cus_...`, mas cria
   `PaymentIntent` com `payment_method` **sem `customer`**. O Stripe exige o
   Customer correspondente quando o método já está anexado a ele. Acrescentar
   o Customer ao comando e ao adaptador é necessário para o cartão salvo.
3. `POST /payment-method-bindings` aceita uma referência `pm_...` e não valida
   no Stripe se o SetupIntent terminou com sucesso ou se o método pertence ao
   Customer. O SaaS deve verificar o resultado no Stripe; a API deve ganhar a
   validação/ingestão confiável antes do uso desassistido. O webhook atual
   processa eventos de `payment_intent.*` e `charge.refunded`, não efetiva
   `setup_intent.succeeded` para planos FREE que exigem cartão.
4. Cada `BillingConnection` devolve um caminho de webhook próprio. O Stripe
   precisa de um destino que entregue nesse caminho; criar um destino manual
   por conta não escala. Destinos registrados na mesma conta Stripe para os
   mesmos tipos de evento recebem eventos de outros Customers também. É
   necessária automação de provisionamento de destinos
   ou uma entrada compartilhada que encaminhe o evento à conexão certa.
   A confirmação atual recebe `connection_id` na rota, mas não compara esse ID
   com a conexão da cobrança persistida; esse vínculo deve ser validado antes
   de usar um ingresso compartilhado ou receber eventos de várias conexões.
5. O worker de Billing despacha e expira `CollectionRequest` existentes. O
   código atual não mostra um gerador automático de `CollectionRequest` para
   a próxima renovação PAID. Não prometa renovação automática antes de
   implementar e validar esse agendamento.
6. Não há reserva/captura/liberação de créditos por execução. Cobrar antes de
   enfileirar protege o saldo; a política para falha posterior da infraestrutura
   precisa ser definida. A compensação automática de consumo não está pronta.

Esses itens também aparecem como dependências no [plano do admin-ui](admin-ui-futuro.md).
