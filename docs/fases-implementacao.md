# Fases de implementação

## Objetivo

Implementar a API de assinaturas, créditos e consumo em incrementos seguros.
Cada fase deve deixar o sistema utilizável, testado e observável antes que a
próxima introduza regras comerciais ou integração externa mais complexa.

## Princípios de execução

- O núcleo de assinaturas, créditos e consumo não depende de um provedor de
  pagamentos. Stripe é o primeiro adaptador previsto.
- Valores de crédito e quantidades usam inteiros de 64 bits representados como
  strings decimais nos contratos HTTP.
- Movimentos financeiros e de consumo são append-only: correções criam novos
  registros, nunca alteram ou apagam o histórico.
- Qualquer operação externa que possa alterar crédito ou consumo exige
  `transaction_id` e `Idempotency-Key`.
- Cada fase inclui migrações, contrato OpenAPI, testes unitários, testes de
  integração com PostgreSQL e cenários de concorrência pertinentes.

## Fase 0 — Preparação e decisões executáveis

**Objetivo:** transformar a especificação em uma base de desenvolvimento
verificável.

Entregas:

- ADRs para unidades de crédito, razão imutável, idempotência, concorrência e
  limites de responsabilidade entre o domínio e o provedor de pagamentos;
- modelo de autenticação, autorização e isolamento por workspace;
- convenções para IDs, relógio UTC, erros de domínio, paginação por cursor e
  auditoria;
- ambiente local PostgreSQL, migrações e estratégia de dados de teste;
- primeiro recorte do OpenAPI e matriz de invariantes a serem testadas.

**Saída da fase:** equipe consegue criar migrações e fluxos de domínio com
contratos e critérios de aceite compartilhados.

## Fase 1 — Primitivas do domínio e fundação transacional

**Objetivo:** estabelecer tipos e mecanismos reutilizáveis antes de regras de
negócio maiores.

Entregas:

- tipos para IDs de domínio, `credit_units`, `item_units`, períodos e erros;
- validação de inteiros, overflow, metadados e referências externas;
- registro de idempotência e unicidade de `transaction_id` por workspace;
- infraestrutura transacional SQLx, locks explícitos e mapeamento de erros de
  restrição para respostas de domínio;
- trilhas mínimas de auditoria e telemetria.

**Critério de saída:** requisições duplicadas nunca criam dois efeitos, mesmo
após falha de rede ou concorrência.

## Fase 2 — Carteira principal e extrato de créditos

**Objetivo:** entregar o razão confiável que sustenta todos os créditos e
débitos futuros.

Entregas:

- `Wallet`, `CustomerWallet`, eventos de ciclo de vida e provisionamento;
- `CustomerWalletEntry`, referências transacionais e projeção de saldo;
- concessão direta de créditos e consulta de saldo e extrato;
- regras de imutabilidade, versão otimista e reconciliação entre saldo e razão;
- testes de crédito concorrente, conflito de chave e falha antes do commit.

**Critério de saída:** toda mudança de saldo possui exatamente um lançamento
auditável e o saldo pode ser reconciliado com o extrato.

## Fase 3 — Catálogo, preços e medição de consumo

**Objetivo:** converter consumo de itens em débitos de crédito de forma
determinística.

Entregas:

- `Product`, `Item` e `PriceVersion` com publicação imutável;
- provisionamento de `ItemWallet` para itens faturáveis;
- `UsageEvent`, `ItemWalletEntry`, `PricingAccumulator`, `BillingBlock` e
  `Debit`;
- preços por bloco e faixas progressivas, com ciclos declarativos quando
  necessário;
- endpoints de consumo, elegibilidade, medidor e extrato por item.

**Critério de saída:** consumos concorrentes no mesmo item formam blocos sem
perda ou duplicação; um débito sem saldo suficiente é rejeitado integralmente.

## Fase 4 — Assinaturas, planos e franquia por ciclo

**Objetivo:** modelar o contrato comercial sem ainda depender de uma cobrança
real.

Entregas:

- `Subscription`, `SubscriptionPlanVersion`, `OnDemandPlan`, `CustomerPlan` e
  ciclos materializados;
- adesão a planos gratuitos, entitlement e regras de elegibilidade;
- concessão de franquia por ciclo, `CreditLot`, alocação e expiração;
- cancelamento no fim do período, transição de plano e regularização de estado;
- scheduler durável apenas para eventos internos de calendário.

**Critério de saída:** planos gratuitos concedem e expiram franquias uma única
vez por ciclo, sem afetar créditos avulsos persistentes.

## Fase 5 — Fronteira de billing agnóstica ao provedor

**Objetivo:** definir a integração de pagamento sem acoplar o domínio ao
Stripe.

Entregas:

- interface `BillingConnector` e capacidades de meio de pagamento;
- `BillingConnection`, `PaymentMethodBinding`, `CollectionRequest`,
  `CollectionAttempt` e `BillingPayment`;
- máquina de estados normalizada para confirmação, falha, autenticação
  adicional e estado incerto;
- `WebhookInbox`, validação de assinatura e deduplicação de eventos;
- outbox transacional e eventos correlacionados de cobrança.

**Critério de saída:** um conector falso permite provar que a confirmação
idempotente ativa um ciclo e concede créditos uma única vez.

## Fase 6 — Primeiro conector: Stripe e cartão tokenizado

**Objetivo:** habilitar planos pagos e recargas usando cartão sem armazenar
dados sensíveis.

Entregas:

- setup seguro de cartão via superfície hospedada ou componente do Stripe;
- criação idempotente de cobrança e correlação com o domínio local;
- processamento de webhooks assinados para pagamento confirmado, falha e
  autenticação adicional;
- renovação com tentativa única no ciclo e regularização manual;
- procedimentos operacionais para cobrança não conciliada e estorno externo.

**Critério de saída:** eventos duplicados, atrasados ou fora de ordem não
duplicam cobrança, assinatura, ciclo ou concessão de créditos.

## Fase 7 — Promoções e ajustes administrativos

**Objetivo:** completar as fontes não recorrentes de crédito com controles de
auditoria.

Entregas:

- `Voucher`, `Coupon`, limite de resgate por usuário e estoque concorrente;
- `Compensation` com criação, aprovação quando necessária e execução;
- referências oficiais entre promoções, créditos, pagamentos e extratos;
- permissões administrativas e histórico de ações.

**Critério de saída:** um vale, cupom ou compensação nunca gera crédito em
duplicidade, inclusive sob requisições concorrentes.

## Fase 8 — Operação, reconciliação e prontidão de produção

**Objetivo:** tornar o serviço sustentável após o lançamento.

Entregas:

- jobs de reconciliação para razão, saldo, blocos, lotes e cobranças;
- métricas, alertas e painéis para conflitos, pendências e falhas de webhook;
- retenção, exportação e consultas administrativas de extratos;
- testes de carga, deadlock, recuperação pós-falha e disaster recovery;
- runbooks para conciliação manual, incidente de pagamento e reparo auditado.

**Critério de saída:** divergências são detectadas, investigadas e corrigidas
sem editar histórico financeiro.

## Ordem recomendada de liberação

1. Fases 0 a 3: créditos e consumo estrito, sem assinatura paga.
2. Fase 4: planos gratuitos e franquias por ciclo.
3. Fases 5 e 6: billing agnóstico e Stripe como primeiro conector.
4. Fase 7: promoções e ajustes administrativos.
5. Fase 8: escala operacional e preparação para produção.

Essa sequência permite validar o maior risco do produto — a consistência entre
uso, saldo e extrato — antes de introduzir dinheiro, fornecedores externos e
fluxos comerciais adicionais.
