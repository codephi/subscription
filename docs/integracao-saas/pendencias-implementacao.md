# Pendências para integrar o SaaS à Subscription API

Este documento descreve o trabalho necessário para oferecer no SaaS cadastro de
conta, cartão salvo, assinatura paga recorrente ou recarga de créditos e
cobrança por execução de pipeline. A Subscription API controla catálogo,
direitos de acesso, wallets, créditos e consumo. O SaaS continua responsável
por usuários, autenticação, checkout e associação entre conta e pipelines.

Leia também os fluxos operacionais de [créditos](credito.md) e
[assinaturas](assinatura.md). Este documento registra o estado e as pendências
conhecidas da integração descrita nesses guias.

O exemplo TaskLab em `example/` já integra criação de conta, checkout de
assinatura e recarga, configuração/listagem/remoção de cartão e cobrança de
execução. Para esse exemplo, esses itens não são pendências de implementação;
consulte o [guia TaskLab](tasklab.md). As pendências abaixo tratam do que um
SaaS consumidor ainda precisa construir e das lacunas atuais da API/admin-ui.

## Decisões de arquitetura antes da implementação

### Identidade e limite de cobrança

- Cada conta pagante do SaaS deve ter um UUID estável usado como
  `account_id` e `customer_id` na Subscription API.
- Os accounts, pipelines e execuções internos continuam identificados no
  banco do SaaS. A Subscription API não oferece saldo ou limite separado por
  account interno; os itens da conta compartilham a `customer_wallet`.
- O banco do SaaS deve relacionar pelo menos o `account_id`, o usuário interno,
  Stripe Customer (`cus_...`), conexão de Billing, `customer_plan_id`,
  referências de preço/oferta e IDs de operações de cobrança.
- Somente o backend autenticado do SaaS deve chamar as rotas de negócio. O
  cliente não deve poder escolher livremente `account_id` nem chamar a API
  diretamente.

### Quem controla a recorrência

Escolha um único sistema como autoridade para gerar as cobranças recorrentes.
O fluxo implementado neste repositório usa a Subscription API para agendar
renovações pagas mensais e a Stripe para processar cada cobrança. Não crie
Stripe Subscriptions para os mesmos CustomerPlans.

**Opção A — Stripe controla a assinatura de cobrança (alternativa ainda não implementada).**
A Stripe gera faturas, tenta cobrar e emite eventos. A Subscription API continua sendo a autoridade
dos direitos e créditos do produto, e deve espelhar cada ciclo confirmado. O
backend atual não cria Stripe Subscriptions nem processa eventos de ciclo de
vida de assinatura/fatura; essa integração precisa ser desenvolvida.

**Opção B — Subscription API controla os ciclos (fluxo atual, mensal).** A API
mantém `SubscriptionPlanVersion`, `CustomerPlan` e calendário, e cria uma
`CollectionRequest` por renovação; a Stripe processa os `PaymentIntent`s. O
dispatcher do Billing agenda a cobrança quando vence o ciclo, usa o cartão
salvo e confirma o novo ciclo e a franquia pelo fluxo de confirmação persistido.
O agendamento de renovação paga está limitado a planos `MONTHLY`; outras
recorrências comerciais ainda precisam ser incluídas nesse fluxo.

Em ambas as opções, pagamentos confirmados devem avançar no máximo um ciclo e
conceder no máximo uma franquia por cobrança confirmada.

## Pendências comuns aos dois modelos

### 1. Fluxo de cartão salvo

**Implementado na API:** Subscription cria uma Checkout Session hospedada em
modo `setup` ou confirma a sessão de pagamento hospedada, recupera os objetos
na Stripe e valida Customer e PaymentMethod antes de persistir um vínculo
tokenizado. A API permite listar e remover cartões salvos; uma repetição da
confirmação não duplica o vínculo. O checkout hospedado de compra também pode
salvar o cartão para uso futuro. Dados brutos do cartão não são persistidos pela
Subscription.

**Pendente em uma integração SaaS própria:** redirecionar o navegador para a
URL de checkout, confirmar o setup usando o ID da sessão retornado e guardar o
ID do vínculo no backend. O exemplo TaskLab já implementa esse fluxo. Ainda é
necessário validar em Stripe test mode o ciclo completo, incluindo autenticação
adicional. Setup aprovado autoriza uso futuro, mas não garante aprovação das
próximas cobranças.

As cobranças off-session usam o Customer e o vínculo de cartão associados à
CollectionRequest. A API oferece remoção de cartão, mas ainda não oferece
seleção de método padrão, tratamento de cartões expirados ou notificações ao
cliente. A experiência para atualizar o cartão deve ser integrada no SaaS.

Cobranças com `requires_action` são registradas e expostas como estado da
operação; ainda falta uma experiência segura no SaaS para o cliente concluir a
autenticação adicional do PaymentIntent e retomar a operação.

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

1. Criar usuário e conta no serviço de Accounts/SaaS e emitir um `account_id`
   permanente.
2. Enviar para `/v1/internal/accounts/account-events` os envelopes
   assinados e ordenados `account.created` e `account.activated`. Guardar
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
3. Criar o CustomerPlan FREE pelo backend e confirmar que está ativo. A
   admin-ui atualmente não cria essa adesão.
4. Em um SaaS próprio, criar a tela de pacotes e checkout. A API já aceita
   compras avulsas (`ON_DEMAND`) pela rota de compra ou pelo checkout genérico,
   com cartão salvo
   ou checkout hospedado Stripe. O backend do SaaS deve iniciar a compra com
   chave de idempotência estável, guardar `checkout_id` e
   `collection_request_id`, acompanhar o estado e mostrar o saldo somente após
   confirmação e lançamento do crédito. A admin-ui investiga cobranças, mas não
   realiza a compra pelo cliente.

### 6. Cobrar pipeline de forma consistente

1. Gerar um ID estável de execução no SaaS antes do enqueue e mapear a conta
   para `account_id`, `product_id` e `item_id` configurados.
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
   todos vinculados ao account e ao CustomerPlan internos. Definir restrições
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

## Pendências para ampliar a recorrência pela Subscription API (Opção B)

1. Ampliar o agendamento e a confirmação de renovações pagas para as
   recorrências comerciais além de `MONTHLY`, mantendo uma única cobrança por
   CustomerPlan e ciclo e respeitando cancelamento e estado comercial.
2. Validar em Stripe test mode o fluxo completo de renovação, incluindo
   reinício/concorrência do dispatcher, resposta incerta e webhook repetido.
   A implementação já persiste tentativas com chave idempotente e confirma o
   ciclo e os créditos pela cobrança registrada.
3. Definir recuperação de falha: estado de atraso, prazo de tolerância,
   suspensão de entitlement, retentativa ou regularização manual, incluindo a
   experiência para autenticação adicional do cliente.
4. Integrar no SaaS a rota de regularização existente para retomada manual e
   ampliar a admin-ui para iniciar/acompanhar essa operação se operadores precisarem
   executá-la pelo painel. Hoje o painel mostra a investigação, mas não inicia
   regularização nem conduz ação de cartão.

## Escopo atual da admin-ui

| Já disponível | Continua sendo necessário implementar |
| --- | --- |
| Criar produtos, itens, preços, planos comerciais, ofertas avulsas e políticas; publicar preço e ativar produto. | Checkout e seleção de ofertas para clientes. |
| Consultar accounts, plano, saldo, extrato, medidor e prontidão das wallets. | Cadastro/login e associação de usuário à conta no SaaS. |
| Configurar e testar uma integração Stripe gerenciada, validar chave e criar Customer associado à conexão. | Automação escalável de destinos Stripe/webhooks. |
| Conceder crédito manualmente e ajustar configuração de Billing. | Compra self-service de créditos e configuração self-service de cartão. |
| Cancelar, transicionar ou revogar CustomerPlan; reconciliar wallets/uso; agendar e cobrar renovações pagas mensais pela Subscription API. | Adesão self-service, recorrências pagas diferentes de mensal e gestão self-service de falhas/cartões. |
| Investigar coleções, tentativas, pagamentos, webhooks e filas; replay operacional disponível para os casos expostos. | Experiência do cliente para autenticação adicional, falhas, troca de cartão e atualização de acesso. |

O painel é operacional e não tem autenticação própria. Deve permanecer em rede
interna/VPN até que autenticação e autorização sejam implementadas.

## Sequência recomendada de entrega

1. Manter a Subscription API como autoridade do ciclo e estado comercial, ou
   planejar explicitamente a migração para Stripe Subscriptions sem executar
   dois agendadores para o mesmo plano.
2. Integrar o fluxo hospedado existente no SaaS e validar em Stripe test mode
   Customer, SetupIntent, PaymentMethod e PaymentIntent; conferir a conclusão
   de autenticação adicional e a cobrança futura.
3. Implementar webhooks escaláveis, deduplicados e vinculados à cobrança e à
   conta persistidas.
4. Integrar cadastro de conta, eventos assinados, provisionamento de wallet e
   adesão ao plano pelo backend do SaaS.
5. Entregar primeiro o caminho pré-pago: compra confirmada, crédito lançado,
   registro de consumo antes de cada pipeline e reconciliação de execução.
6. Validar renovações mensais e decidir a política de falhas; depois ampliar a
   geração e confirmação de renovações para outras recorrências, se necessário.
7. Verificar fluxos ponta a ponta em Stripe test mode, incluindo webhook
   repetido, resposta perdida, falha de pagamento, autenticação adicional,
   restart/concorrência do worker e nenhuma liberação antes da confirmação.
8. Antes de produção, proteger rotas de negócio e administrativas com
   autenticação de serviço e autorização por account. A configuração atual
   depende de rede confiável e não oferece login/autorização.

## Critérios de conclusão

- Uma conta autenticada do SaaS só opera sobre o seu account associado.
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
