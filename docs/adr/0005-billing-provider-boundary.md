# ADR 0005: fronteira de Billing e Stripe

## Decisão

O domínio depende de uma interface `BillingConnector`. A V1 terá uma
implementação falsa para testes e um adaptador Stripe por HTTP, isolado em
`repositories/`. O adaptador fixa a versão `2026-08-26.dahlia`, usa
SetupIntents para preparar cartões e PaymentIntents para cobranças.

Confirmação definitiva vem de webhook assinado e deduplicado. Timeout ou
resposta incerta preserva a tentativa; não dispara polling, nova cobrança ou
cancelamento.

## Consequências

- `reqwest` será introduzido somente junto do adaptador HTTP;
- chaves idempotentes do provedor derivam da tentativa persistida;
- dados brutos de cartão e segredos não são persistidos.
