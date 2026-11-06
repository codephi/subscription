# ADR 0006: checkout hospedado e recorrência gerenciada pelo provedor

## Contexto

Tasklab precisa oferecer compras avulsas, assinatura, cadastro independente de
cartão e gestão de carteira sem receber dados brutos de cartão nem conhecer
detalhes do gateway. A Subscription já é dona do Billing e dos créditos.

## Decisão

- Tasklab usa somente os contratos da Subscription e trata `redirect_url` como
  valor opaco. Checkout e configuração de cartão acontecem em páginas hospedadas
  pelo provedor.
- A Subscription controla consentimento para reutilização de cartão e guarda
  referências tokenizadas e metadados mínimos. Um apelido é opcional.
- Stripe usa recorrência gerenciada pelo provedor. O dispatcher local não agenda
  cobranças para planos ligados a uma assinatura gerenciada externamente.
- Webhooks assinados e idempotentes são a autoridade para pagamentos e ciclos;
  o retorno do navegador não concede créditos.
- O contrato interno permanece orientado a capacidades. Um gateway futuro deve
  declarar suporte às operações necessárias; nenhum adaptador fictício é criado.
- O Tasklab inclui um plano mensal de teste de R$ 1,00 com 10 créditos por ciclo.
  A periodicidade de produção segue mensal; testes de ciclos usam Stripe Test
  Clocks quando o cenário permitir.

## Consequências

O catálogo, o checkout, as referências da carteira e a confirmação de pagamento
permanecem na Subscription. Stripe controla cobranças recorrentes e faturas;
Subscription projeta o estado da assinatura e concede cada ciclo uma única vez.
Operações de cartão continuam dependentes das capacidades do provedor e podem
devolver URLs hospedadas de gerenciamento.
