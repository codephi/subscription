# Prontidão para o MVP

Status revisado em 12 de setembro de 2026.

## Decisão de escopo

Existem dois recortes possíveis de MVP:

- **MVP gratuito:** oferece catálogo, planos gratuitos, wallets, franquias e
  consumo medido. O núcleo funcional desse recorte está implementado nas fases
  0 a 6; ainda exige a preparação operacional mínima descrita abaixo antes de
  uma liberação real.
- **MVP pago:** acrescenta contratação, renovação e recuperação de pagamento
  com Stripe. Este é o recorte recomendado para validar o produto comercial e
  depende da conclusão das fases 7 e 8, além da preparação operacional mínima.

Promoções não fazem parte do primeiro MVP. Voucher, Cupom e Compensation ficam
para a fase 9, salvo se passarem a ser requisito comercial explícito.

## O que está pronto

- fases 0 a 6 com seus gates automatizados aprovados;
- contratos, projeção assinada de workspaces e entrega por inbox/outbox;
- catálogo e preços publicados imutáveis;
- provisionamento e reconciliação de wallets;
- razão append-only, lotes de crédito, saldo e extratos;
- planos gratuitos, recorrência, franquia por ciclo e consumo `CREDIT_STRICT`;
- fronteira de Billing com conector falso, tentativa comercial única e estados
  normalizados;
- confirmação inicial paga, renovação e regularização manual idempotentes;
- expiração comercial, falha definitiva e preservação de resultados incertos;
- caso operacional idempotente para pagamento confirmado sem solicitação local.

## Bloqueadores do MVP pago

### 1. Concluir a fase 7

- receber e validar webhook assinado, inclusive duplicado, atrasado e fora de
  ordem;
- reprocessar eventos válidos pela `WebhookInbox` sem regredir estado terminal;
- concluir cancelamento normal e revogação administrativa, incluindo o
  encerramento atômico de cobranças pendentes;
- provar o comportamento de planos com recorrência `NONE`;
- implementar compra `OnDemand` com crédito persistente e sem criar ciclo;
- concluir upgrade pago sem prorrata e com troca atômica após confirmação;
- aplicar `payment_completion_window` e idempotência em todos os tipos de
  `CollectionRequest`;
- cobrir recuperação antes e depois de falha de commit;
- aprovar `PH-07`, `AC-10`, `AC-11` e `EX-07` na matriz de testes.

### 2. Implementar a fase 8

- criar e consultar `BillingConnection`;
- criar sessão segura de setup e persistir somente o vínculo tokenizado do
  cartão;
- implementar o `BillingConnector` do Stripe;
- criar `PaymentIntent` com chave idempotente e metadata de correlação;
- normalizar confirmação, falha e `requires_action`;
- expor o endpoint real de webhook e validar a assinatura do Stripe antes de
  qualquer escrita;
- provar convergência para eventos duplicados, atrasados e fora de ordem;
- disponibilizar consulta operacional de pagamentos não correspondidos;
- observar estorno executado externamente sem iniciar refund ou efeito de
  crédito automático;
- aprovar `PH-08`, `AC-12`, `AC-13` e `EX-08`.

### 3. Preparação operacional mínima

O MVP não precisa concluir toda a fase 10, mas não deve ser liberado sem:

- reconciliação de cobranças, wallets e saldos;
- métricas e alertas para falha de webhook, backlog de inbox/outbox,
  dead-letter e cobrança não correspondida;
- replay operacional auditável;
- rotação das credenciais de webhook e do provedor;
- testes essenciais de carga, concorrência e recuperação;
- runbooks para cobrança incerta, pagamento não conciliado e replay.

## Fora do primeiro MVP

- Voucher e Cupom;
- Compensation administrativa;
- refund iniciado pelo produto;
- meios de pagamento diferentes de cartão tokenizado;
- retentativa automática, polling ou watchdog do provedor;
- pausa e retomada de plano;
- recursos avançados da fase 10, como disaster recovery completo e testes de
  escala máxima.

## Ordem de implementação

1. Fechar o contrato de webhook assinado e os cenários de atraso/replay.
2. Completar cancelamento, revogação e recorrência `NONE`.
3. Completar `OnDemand` e upgrade pago.
4. Aprovar o gate integral da fase 7.
5. Implementar o conector Stripe e seu fluxo de cartão/webhook.
6. Aprovar o gate integral da fase 8.
7. Entregar a preparação operacional mínima e executar a validação ponta a
   ponta em ambiente semelhante ao de produção.

O estado detalhado e executável permanece na
[matriz de testes](test-matrix.md). O desenho normativo está no
[plano técnico](plano-tecnico-api-assinaturas-rust.md), e a sequência completa
está nas [fases de implementação](fases-implementacao.md).
