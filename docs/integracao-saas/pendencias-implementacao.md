# Pendências para integrar o SaaS à Subscription API

Este documento descreve o trabalho necessário para oferecer no SaaS cadastro de
conta, cartão salvo, assinatura paga recorrente ou recarga de créditos e
cobrança por execução de pipeline. A Subscription API controla catálogo,
direitos de acesso, wallets, créditos e consumo. O SaaS continua responsável
por usuários, autenticação, checkout e associação entre conta e pipelines.

Leia também os fluxos operacionais de [créditos](credito.md) e
[assinaturas](assinatura.md). Este documento registra o estado e as pendências
conhecidas da integração descrita nesses guias.

## Decisões de arquitetura antes da implementação

### Identidade e limite de cobrança

- Cada conta pagante do SaaS deve ter um UUID estável usado como
  `workspace_id` e `customer_id` na Subscription API.
- Os workspaces, pipelines e execuções internos continuam identificados no
  banco do SaaS. A Subscription API não oferece saldo ou limite separado por
  workspace interno; os itens da conta compartilham a `customer_wallet`.
- O banco do SaaS deve relacionar pelo menos `account_id`, `workspace_id`,
  Stripe Customer (`cus_...`), conexão de Billing, `customer_plan_id`,
  referências de preço/oferta e IDs de operações de cobrança.
- Somente o backend autenticado do SaaS deve chamar as rotas de negócio. O
  cliente não deve poder escolher livremente `workspace_id` nem chamar a API
  diretamente.

### Quem controla a recorrência

Escolha um único sistema como autoridade para gerar as cobranças recorrentes.
Não habilite ao mesmo tempo a criação de cobranças recorrentes pela Stripe e
pelo calendário interno da Subscription API.

**Opção A — Stripe controla a assinatura de cobrança.** A Stripe gera faturas,
tenta cobrar e emite eventos. A Subscription API continua sendo a autoridade
dos direitos e créditos do produto, e deve espelhar cada ciclo confirmado. O
backend atual não cria Stripe Subscriptions nem processa eventos de ciclo de
vida de assinatura/fatura; essa integração precisa ser desenvolvida.

**Opção B — Subscription API controla os ciclos.** A API mantém
`SubscriptionPlanVersion`, `CustomerPlan` e calendário, e cria uma
`CollectionRequest` por renovação; a Stripe processa os `PaymentIntent`s. O
backend atual não gera automaticamente a cobrança paga seguinte. É necessário
implementar esse agendamento no Billing, sem delegar a geração de faturas à
Stripe.

Em ambas as opções, pagamentos confirmados devem avançar no máximo um ciclo e
conceder no máximo uma franquia por cobrança confirmada.

## Pendências comuns aos dois modelos

### 1. Completar o fluxo de cartão salvo

1. Corrigir a criação do SetupIntent: hoje o adaptador envia `return_url` na
   criação sem confirmação, combinação rejeitada pela Stripe. Definir e
   implementar onde o `return_url` entra no fluxo de confirmação usado pelo
   Stripe.js/Elements.
2. No frontend do SaaS, criar a tela de cartão com Stripe.js/Elements. O
   backend cria a sessão e entrega o `client_secret` somente ao cliente e à
   sessão que iniciaram aquela operação. Dados de cartão não passam nem ficam
   armazenados no SaaS ou na Subscription API.
3. Tratar estados do SetupIntent no cliente, incluindo autenticação adicional,
   cancelamento e falha; o redirect de retorno não é prova de sucesso.
4. Depois da confirmação, o backend consulta a Stripe e valida que o
   SetupIntent está `succeeded`, pertence ao `cus_...` esperado e identifica o
   PaymentMethod associado.
5. Só depois dessa validação registrar o PaymentMethod na
   `/payment-method-bindings`. A API hoje valida o formato da referência, mas
   não prova junto à Stripe que o método foi configurado com sucesso e pertence
   ao Customer da conexão.
6. Confirmar que o Customer da BillingConnection é propagado até toda cobrança
   PaymentIntent e que o PaymentMethod vinculado pertence a esse Customer. A
   integração Stripe em desenvolvimento já cria/associa Customer à conexão e
   o adaptador atual aceita `customer_reference`; manter essa associação como
   invariável e cobri-la no fluxo completo.
7. Definir troca e remoção de cartão, método padrão, cartões expirados e
   notificação ao usuário. O contrato atual lista vínculos, mas não oferece um
   fluxo completo de gestão de cartões pelo cliente.

### 2. Tornar webhooks escaláveis e confiáveis

1. Substituir a configuração manual de um destino Stripe por conexão por uma
   entrada compartilhada ou automação de destinos, conforme a arquitetura
   escolhida.
2. No webhook compartilhado, resolver a conexão correta a partir de dados
   confiáveis do evento e validar que ela corresponde à cobrança persistida.
   A confirmação precisa comparar a conexão do evento com a conexão salva na
   cobrança antes de aceitar qualquer efeito financeiro.
3. Validar assinatura sobre o corpo original, deduplicar por ID de evento e
   tolerar entrega repetida ou fora de ordem. Eventos de provedor são
   correlação; valor, moeda, Customer, plano e cobrança persistidos continuam
   sendo a autoridade para conceder acesso/créditos.
4. Manter processamento durável, observável e reprocessável para eventos com
   falha, sem confirmar pagamentos manualmente pela interface.

### 3. Integrar criação e ativação de conta

1. Criar usuário e conta no serviço de Accounts/SaaS e emitir um `workspace_id`
   permanente.
2. Enviar para `/v1/internal/accounts/workspace-events` os envelopes
   assinados e ordenados `workspace.created` e `workspace.activated`. Guardar
   IDs de evento e sequência para não reutilizar identidade com outro corpo.
3. Esperar a projeção operacional ativa e as wallets prontas antes de admitir
   o plano ou aceitar consumo. Reconciliar provisionamento quando a resposta
   indicar que a hierarquia não está pronta.
4. Configurar a política de admissão e `billing-config` para permitir o fluxo
   desejado. A criação de plano pode exigir `recurring_credit_enabled=true`,
   inclusive para plano FREE usado como acesso pré-pago.
5. Guardar as associações e IDs retornados em armazenamento persistente do
   SaaS; não depender de localStorage ou de IDs mantidos somente pelo navegador.

### 4. Liberar acesso a partir do estado comercial

- O backend do SaaS deve consultar/consumir a confirmação do CustomerPlan e
  liberar cada produto somente quando o plano estiver efetivamente ativo e
  conceder entitlement para esse produto.
- Uma resposta de criação, um redirect de checkout, estado `SCHEDULED` ou
  `PENDING` não comprova pagamento. Para compra e assinatura paga, aguardar
  confirmação processada e consultar o recurso interno correspondente.
- Definir a reação a `PAST_DUE`, `RENEWAL_INACTIVE`, cancelamento ao fim do
  período, revogação e fim de acesso. A API não substitui o enforcement de
  autorização nas rotas do próprio SaaS.

## Recarga e cobrança por execução

### 5. Habilitar o modelo pré-pago

1. Cadastrar na admin-ui o produto `CREDIT_METERED`, item faturável, preço
   versionado e publicado; cadastrar uma assinatura `CREDIT_STRICT`, plano
   FREE que concede acesso sem saldo e ofertas `OnDemandPlan` para cada pacote.
2. Configurar a integração Stripe da conta pela operação disponível e guardar
   o vínculo retornado. A funcionalidade de integrações valida a chave Stripe,
   pode criar Customer gerenciado e armazena segredos no cofre; se ainda estiver
   usando uma BillingConnection legada, conferir como o Customer é criado e
   associado antes de ativar cobranças.
3. Depois de corrigir o cartão e webhooks, criar o CustomerPlan FREE pelo
   backend e confirmar que está ativo. A admin-ui atualmente não cria essa
   adesão.
4. Criar no SaaS a tela de pacotes e checkout. O backend inicia uma compra
   avulsa com chave de idempotência estável, guarda o `collection_request_id`
   e mostra saldo somente depois de pagamento confirmado e crédito lançado.
   A admin-ui consulta/investiga a cobrança, mas não realiza a compra pelo
   cliente.

### 6. Cobrar pipeline de forma consistente

1. Gerar um ID estável de execução no SaaS antes do enqueue e mapear a conta
   para `workspace_id`, `product_id` e `item_id` configurados.
2. Antes de iniciar o pipeline, chamar `/usage-events` com `item_units`,
   `transaction_id` e `Idempotency-Key` determinísticos para aquela execução.
   Repetições devem representar retry da mesma execução, nunca uma nova
   cobrança.
3. Enfileirar somente após a API aceitar o consumo. Tratar falta de entitlement,
   saldo insuficiente e wallet indisponível sem iniciar a execução.
4. Persistir a resposta e correlações no SaaS para suporte, auditoria e retries.
   Consultar extratos/medidores para reconciliação, sem usar a elegibilidade
   como reserva de saldo.
5. Definir política para falha do pipeline depois do débito. A API não possui
   reserva/liberação nem compensação automática; eventual reembolso precisa de
   um fluxo de produto e financeiro definido separadamente.

## Trabalho adicional para Stripe controlar a recorrência (Opção A)

1. **Estender o adaptador/conector:** implementar criação, consulta, alteração
   e cancelamento de Stripe Subscription e a associação a preço recorrente da
   Stripe. Hoje `Subscription` e `SubscriptionPlanVersion` são catálogo interno;
   não criam Stripe Product/Price/Subscription.
2. **Vincular objetos:** persistir `stripe_subscription_id`, Customer,
   referência de preço e, conforme necessário, IDs de invoice/payment intent,
   todos vinculados ao workspace e ao CustomerPlan internos. Definir restrições
   de unicidade e integridade para evitar associação cruzada entre contas.
3. **Definir a autoridade de estado:** mapear os estados da Stripe para estados
   comerciais e de renovação internos; decidir se mudanças são recebidas por
   webhook, reconciliadas por consulta ou ambas. Não manter dois agendadores
   gerando a mesma renovação.
4. **Processar eventos de assinatura e invoice:** ao receber eventos, localizar
   o CustomerPlan pela associação persistida; verificar conexão/Customer,
   invoice, valor, moeda, preço e período; deduplicar eventos; e aplicar cada
   efeito comercial uma única vez.
5. **Conceder franquia pelo ciclo pago:** a confirmação da invoice deve avançar
   o ciclo interno e criar a concessão de créditos idempotente vinculada à
   invoice/período. Uma invoice paga repetida não pode renovar duas vezes.
6. **Processar falhas e ação do cliente:** refletir fatura vencida ou pagamento
   que exige ação sem conceder novo ciclo. Criar no SaaS uma experiência para o
   cliente concluir autenticação/atualizar cartão e retomar a cobrança conforme
   o estado da invoice da Stripe.
7. **Sincronizar operações:** ao cancelar, trocar de plano ou atualizar método
   padrão, aplicar a mudança tanto na Stripe quanto no CustomerPlan interno com
   idempotência e comportamento definido para falha parcial.
8. **Migrar/evitar duplicidade:** se já houver CustomerPlan ou CollectionRequest
   no fluxo interno, definir a migração e impedir cobranças concorrentes do
   calendário local e da Stripe.
9. **Atualizar OpenAPI e admin-ui:** expor/mostrar IDs e estados da assinatura,
   invoices, período vigente, próxima data, falha e correlação. A admin-ui hoje
   investiga Billing existente, mas não opera Stripe Subscriptions.

## Trabalho adicional para Subscription API controlar a recorrência (Opção B)

1. Implementar a geração da CollectionRequest `RENEWAL` na data correta do
   ciclo pago, usando `subscription_calendar_jobs`/mecanismo de calendário
   adequado para cobrar uma única vez mesmo após restart ou claims concorrentes.
2. Garantir criação idempotente e transacional: uma renovação por CustomerPlan
   e ciclo; respeitar cancelamento, transição, expiração e estado comercial;
   não gerar cobrança para plano encerrado ou workspace bloqueado.
3. Despachar a cobrança pelo cartão salvo e tratar resposta incerta sem criar
   uma segunda cobrança. Preservar a chave idempotente de provedor e permitir
   reconciliação da tentativa original.
4. Confirmar ciclo e créditos apenas pelo webhook válido da cobrança persistida;
   validar conexão, Customer, valor, moeda, plano e janela comercial. A mesma
   cobrança só pode conceder um ciclo.
5. Definir recuperação de falha: estado de atraso, prazo de tolerância,
   suspensão de entitlement, retentativa ou regularização manual, incluindo a
   experiência para autenticação adicional do cliente.
6. Usar a rota de regularização existente para retomada manual e ampliar a
   admin-ui para iniciar/acompanhar essa operação se operadores precisarem
   executá-la pelo painel. Hoje o painel mostra a investigação, mas não inicia
   regularização nem conduz ação de cartão.

## Escopo atual da admin-ui

| Já disponível | Continua sendo necessário implementar |
| --- | --- |
| Criar produtos, itens, preços, planos comerciais, ofertas avulsas e políticas; publicar preço e ativar produto. | Checkout e seleção de ofertas para clientes. |
| Consultar workspaces, plano, saldo, extrato, medidor e prontidão das wallets. | Cadastro/login e associação de usuário à conta no SaaS. |
| Configurar e testar uma integração Stripe gerenciada, validar chave e criar Customer associado à conexão. | Automação escalável de destinos Stripe/webhooks e checkout para o cliente. |
| Conceder crédito manualmente e ajustar configuração de Billing. | Compra self-service de créditos e configuração self-service de cartão. |
| Cancelar, transicionar ou revogar CustomerPlan; reconciliar wallets/uso. | Adesão de cliente, assinatura Stripe e geração de renovação automática. |
| Investigar coleções, tentativas, pagamentos, webhooks e filas; replay operacional disponível para os casos expostos. | Experiência do cliente para autenticação adicional, falhas, troca de cartão e atualização de acesso. |

O painel é operacional e não tem autenticação própria. Deve permanecer em rede
interna/VPN até que autenticação e autorização sejam implementadas.

## Sequência recomendada de entrega

1. Escolher Opção A ou Opção B e registrar qual sistema é autoridade para
   cobrança, estado da assinatura e calendário.
2. Implementar e validar em Stripe test mode o fluxo de Customer, SetupIntent,
   PaymentMethod e PaymentIntent; corrigir a criação do SetupIntent, validar
   posse/estado do cartão e verificar que o Customer associado chega à cobrança.
3. Implementar webhooks escaláveis, deduplicados e vinculados à cobrança e à
   conta persistidas.
4. Integrar cadastro de conta, eventos assinados, provisionamento de wallet e
   adesão ao plano pelo backend do SaaS.
5. Entregar primeiro o caminho pré-pago: compra confirmada, crédito lançado,
   registro de consumo antes de cada pipeline e reconciliação de execução.
6. Implementar o modelo de recorrência escolhido, incluindo estados de falha,
   cancelamento, mudança de plano e concessão idempotente de franquia.
7. Verificar fluxos ponta a ponta em Stripe test mode, incluindo webhook
   repetido, resposta perdida, falha de pagamento, autenticação adicional,
   restart/concorrência do worker e nenhuma liberação antes da confirmação.
8. Antes de produção, proteger rotas de negócio e administrativas com
   autenticação de serviço e autorização por workspace. A configuração atual
   depende de rede confiável e não oferece login/autorização.

## Critérios de conclusão

- Uma conta autenticada do SaaS só opera sobre o seu workspace associado.
- Um cartão só é vinculado depois de SetupIntent confirmado e validado contra
  o Customer correto; dados brutos do cartão nunca passam pelo backend.
- Compra avulsa só credita após confirmação válida e não duplica com webhook
  repetido.
- Cada execução é debitada uma única vez antes de entrar na fila, e saldo
  insuficiente impede o enqueue.
- Uma assinatura só libera acesso após pagamento confirmado; renovações pagas
  avançam exatamente um ciclo e concedem a franquia no máximo uma vez.
- Falhas, estados incertos, cancelamentos e autenticação adicional resultam em
  estado visível para o cliente e operação investigável no painel.
