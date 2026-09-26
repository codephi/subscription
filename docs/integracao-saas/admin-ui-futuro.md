# Plano para operar a integração inteira pelo admin-ui

Este documento descreve o que falta para o operador configurar ofertas,
Stripe e contas pelo painel interno. O checkout e a confirmação de cartão
continuam sendo feitos pelo cliente no **frontend do SaaS**, com Stripe.js.
O `admin-ui` é um painel operacional, não uma página pública de pagamento.
Consulte os fluxos completos de [créditos](credito.md) e
[assinatura](assinatura.md).

## Estado atual do painel

| Etapa | Já existe no admin-ui | Falta para operar sem chamadas manuais |
| --- | --- | --- |
| Catálogo de produto, item, preço, Subscription, plano e pacote avulso | Criação, consulta, publicação de preço, ativação de produto e revogação de versão de plano | Assistente que mostre a cadeia completa e valide a oferta antes de liberar onboarding. |
| Workspace e carteira | Lista, detalhe, provisionamento, saldos, extratos, medidores e reconciliação | Visão da conta do SaaS com nome externo e workspaces de infraestrutura associados. |
| Configuração de Billing | `direct_credit_enabled`, `recurring_credit_enabled` e conexão Stripe por referências `env://` | Status de Customer Stripe, destino de webhook e verificação de conexão. |
| CustomerPlan | Leitura, cancelamento, transição e revogação | Adesão inicial, seleção da oferta, acompanhamento da primeira cobrança e regularização. |
| Crédito | Crédito direto administrativo com confirmação e idempotência | Recarga paga por `OnDemandPlan` e histórico de pacotes por conta. |
| Billing | Filas, detalhes, indicadores, auditoria e replay de outbox | Visão orientada a uma compra/assinatura, ação do cliente pendente e ligação explícita com a conta do SaaS. |
| Accounts | Inbox e replay de eventos em quarentena | Estado do provisionamento automático da conta e recuperação guiada. |

## Pré-requisitos de backend antes das telas de cobrança

1. **Autenticação e autorização.** Implementar identidade de operador,
   papéis, escopo por `workspace_id` e trilha de ator verificado. O painel
   atual não tem login e as rotas gerais/admin não têm middleware de
   autenticação; não o exponha diretamente à internet.
2. **Identidade Stripe por conta.** Automatizar a criação ou associação de
   um `cus_...` por conta pagante e persistir a relação com o UUID da conta.
   `BillingConnection.external_account_reference` hoje alimenta o parâmetro
   `customer` do SetupIntent; uma conexão para cartão salvo deve apontar ao
   Customer certo.
3. **Webhook escalável.** A resposta da conexão fornece
   `/v1/billing/webhooks/{connection_id}`. Definir e implementar entrada
   compartilhada que identifique a conexão com segurança, ou provisionar e
   manter destinos Stripe automaticamente. O painel só pode mostrar um
   estado `pronto` quando o destino e o segredo correspondentes funcionarem.
   Validar que a conexão da rota corresponde à cobrança antes de aplicar o
   evento; essa comparação não existe no caminho atual de confirmação.
4. **Contrato de cartão salvo.** Corrigir a criação do SetupIntent, que hoje
   envia `return_url` sem `confirm=true`, e enviar `customer` no PaymentIntent; validar
   que o `pm_...` anexado pertence ao Customer da conta e que o SetupIntent
   terminou com sucesso. O cadastro de binding atual valida apenas o prefixo
   `pm_`. Acrescentar tratamento seguro de autenticação adicional do mesmo
   PaymentIntent e uma ação consumível pelo frontend do SaaS.
5. **Renovação paga.** Criar a solicitação `RENEWAL` única por ciclo na data
   de vencimento. O worker atual despacha apenas solicitações existentes.
   Validar restart, concorrência, falha, expiração e regularização.
6. **Conta e consumo.** O `workspace_id` da API representa a conta pagante.
   Definir no SaaS o registro dos workspaces internos e dos `execution_id`;
   para filtros por workspace interno no painel, criar projeção/endpoint de
   relatório. O medidor atual agrega por `(conta, item)`.
7. **Política para execução falha.** Se cobrar antes de enfileirar, definir
   quando uma execução malsucedida mantém o débito e quando há ajuste. A API
   não oferece reserva/liberação nem compensação automática de uso.

Esses trabalhos pertencem ao backend/serviço e à integração do SaaS; uma
tela nova sozinha não elimina os bloqueios.

## Fluxo operacional desejado no painel

### 1. Configuração global por ambiente

1. Mostrar status de PostgreSQL, API, workers de cobrança, calendário e
   outbox. Exibir as variáveis necessárias sem revelar valores secretos.
2. Mostrar a configuração Stripe do ambiente: modo teste/produção,
   referência `env://` da chave secreta, chave publicável do checkout do SaaS
   e status do destino de webhook. Rotação deve trocar o segredo no gerenciador
   de segredos e reiniciar instâncias antes de revogar a versão antiga.
3. Oferecer uma verificação de conectividade e recebimento de evento de teste.
   Não armazenar `sk_...`, `whsec_...`, PAN ou CVV no browser ou no banco da API.

### 2. Assistente de oferta

1. Cadastrar produto `CREDIT_METERED`, itens, unidades e preços em créditos;
   publicar preço e ativar item/produto. Mostrar qual versão de preço vigora
   agora e se cada item terá wallet provisionada.
2. Criar `Subscription` `CREDIT_STRICT` e escolher:
   - **Créditos:** plano FREE sem franquia + pacotes `OnDemandPlan` pagos;
   - **Assinatura:** plano PAID recorrente com preço em moeda, franquia e
     produtos; pacotes extras opcionais.
3. Mostrar revisão final com IDs de catálogo, preço por execução, preço do
   pacote/mensalidade, moeda, recorrência, produtos incluídos e momento em
   que os créditos serão concedidos. Bloquear publicação de oferta incompleta.

As operações de catálogo já têm endpoints em `/v1/products`, `/v1/items`,
`/v1/price-versions`, `/v1/subscriptions` e seus filhos. A melhoria aqui é a
orquestração e a revisão da oferta, não a criação de um segundo catálogo.

### 3. Onboarding de uma conta do SaaS

1. Receber do SaaS o UUID da conta. Mostrar no painel a sequência de eventos
   `workspace.created` e `workspace.activated`; o envio assinado continua
   responsabilidade de Accounts. Não oferecer botão que crie uma identidade
   local sem o evento do sistema proprietário.
2. Mostrar estado `ACTIVE`, provisionamento `ACTIVE` e `wallets.ready=true`.
   Em erro de materialização, oferecer
   `POST /v1/admin/workspaces/{id}/wallet-provisioning/reconcile` com o
   resultado e histórico da ação.
3. Exibir/ajustar `billing-config` com `expected_version`. Para ambos os
   modelos, `recurring_credit_enabled` deve estar ativo antes da adesão.
4. Criar o CustomerPlan pelo
   `POST /v1/workspaces/{id}/customer-plans`, com `Idempotency-Key` e
   `transaction_id` persistidos no servidor. Mostrar estados `ACTIVATED`,
   `PENDING_INITIAL_PAYMENT`, `PAST_DUE` e os produtos efetivamente concedidos.
5. Exibir o `cus_...` Stripe associado e a conexão. Após automação do
   provisionamento, a tela mostra destino de webhook, estado da assinatura e
   horário do último evento válido. Segredos permanecem no servidor.

### 4. Checkout e pagamento do cliente

1. O operador configura o pacote/plano no admin-ui; o cliente escolhe a oferta
   no SaaS. O backend do SaaS chama a API para criar sessão de setup. O
   frontend do SaaS usa Stripe.js/Elements e envia apenas a referência
   `pm_...` validada ao backend.
2. O backend registra o binding e cria a intenção `INITIAL` ou `ON_DEMAND`
   com idempotência. O painel mostra `collection_request_id`, valor, moeda,
   tipo, estado, prazo e PaymentIntent correlato.
3. Após webhook `payment_intent.succeeded`, o painel mostra `PAID`, plano
   `ACTIVATED` quando aplicável, lote de crédito e novo saldo. Em
   `REQUIRES_ACTION`, mostrar que o cliente precisa voltar ao SaaS para
   autenticar **a mesma** tentativa. Em falha/expiração, mostrar motivo e
   ação manual permitida. Não oferecer “cobrar de novo” sem investigar o ID
   original.

### 5. Operação contínua

1. Por conta, mostrar saldo principal, extrato, compras, CustomerPlan,
   coleções e consumo por item. Para quebrar por workspace interno, combinar
   com os registros de execução do SaaS ou desenvolver um relatório dedicado.
2. Mostrar execuções recusadas por falta de saldo e permitir ao operador
   localizar `transaction_id`/`execution_id`. A chamada de consumo permanece
   no backend do SaaS, nunca no painel.
3. Para assinatura, mostrar data do próximo ciclo, cobrança de renovação,
   `PAST_DUE` e regularização. Para créditos, mostrar pacotes e recargas.
4. Preservar as telas existentes de investigação de webhook, pagamentos não
   conciliados, outbox, auditoria e reconciliação. Links entre esses registros
   devem manter `workspace_id`, `customer_plan_id` e
   `collection_request_id` visíveis ao operador.

## Sequência de implementação recomendada

1. Resolver os sete pré-requisitos de backend e validar pagamento Stripe de
   teste de ponta a ponta. Priorize a criação do SetupIntent, `customer` no
   PaymentIntent, Customer por conta, verificação de setup e roteamento de webhook.
2. Adicionar ao admin-ui a revisão de oferta e a página de onboarding da
   conta, usando as rotas já existentes. Persistir operações idempotentes no
   backend, não apenas no `localStorage` do navegador.
3. Adicionar as visões de compra inicial, recarga e regularização; conectar
   estados de Billing ao registro da conta.
4. Adicionar visões de renovação e consumo por workspace interno após os
   respectivos contratos de backend estarem disponíveis.
5. Atualizar `/openapi.json` e os tipos do painel com `npm run api:types` ao
   mudar endpoints; então verificar os fluxos em Stripe teste antes de
   produção.

O [inventário do painel atual](../frontend-administrativo.md) contém rotas,
formulários e endpoints implementados. O [runbook](../runbooks/billing-mvp.md)
cobre cobranças incertas, replay, pagamentos não conciliados e rotação de
credenciais.
