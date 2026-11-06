# Prontidão para o MVP

Status revisado em 14 de setembro de 2026.

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
- contratos, projeção assinada de accounts e entrega por inbox/outbox;
- catálogo e preços publicados imutáveis;
- provisionamento e reconciliação de wallets;
- razão append-only, lotes de crédito, saldo e extratos;
- planos gratuitos, recorrência, franquia por ciclo e consumo `CREDIT_STRICT`;
- fronteira de Billing com conector falso, tentativa comercial única e estados
  normalizados;
- confirmação inicial paga, renovação e regularização manual idempotentes;
- expiração comercial, falha definitiva e preservação de resultados incertos;
- caso operacional idempotente para pagamento confirmado sem solicitação local.

## MVP pago implementado

As fases 7 e 8 foram concluídas no recorte do primeiro MVP:

- conexões Stripe e vínculos de cartão armazenam apenas referências tokenizadas;
- SetupIntent e PaymentIntent usam o adaptador Stripe e idempotência estável;
- o webhook valida a assinatura antes de escrever, deduplica e não regride
  estados finais;
- confirmação, falha definitiva e `requires_action` são normalizados;
- adesão, renovação, regularização, OnDemand e upgrade usam a janela comercial;
- OnDemand não cria ciclo e upgrade só efetiva após confirmação integral;
- cancelamento e revogação terminalizam solicitações pendentes atomicamente;
- pagamentos sem solicitação e estornos externos não geram efeitos automáticos;
- os gates acumulados das fases 7 e 8 estão aprovados.

## Preparação operacional mínima

O recorte mínimo foi entregue sem declarar a fase 10 completa:

- reconciliações existentes de wallets, saldo, lotes e uso;
- consulta de cobranças, webhook, outbox, dead-letter e não conciliados;
- replay auditável e deduplicação/reprocessamento seguro;
- rotação de segredos por referências `env://`, sem persisti-los;
- testes de concorrência e recuperação nos fluxos críticos;
- [runbook do MVP pago](runbooks/billing-mvp.md).

## Fora do primeiro MVP

- Voucher e Cupom;
- Compensation administrativa;
- refund iniciado pelo produto;
- meios de pagamento diferentes de cartão tokenizado;
- retentativa automática, polling ou watchdog do provedor;
- pausa e retomada de plano;
- recursos avançados da fase 10, como disaster recovery completo e testes de
  escala máxima.

Antes da liberação ainda é necessária uma validação ponta a ponta com chaves de
teste Stripe e a configuração dos alertas no ambiente de observabilidade.

O estado detalhado e executável permanece na
[matriz de testes](test-matrix.md). O desenho normativo está no
[plano técnico](plano-tecnico-api-assinaturas-rust.md), e a sequência completa
está nas [fases de implementação](fases-implementacao.md).
