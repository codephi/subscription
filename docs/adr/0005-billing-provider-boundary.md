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
- dados brutos de cartão não são persistidos. Segredos de conexões antigas ficam
  fora do banco; credenciais administradas pela API são persistidas cifradas.

## Credenciais administradas pela API

A configuração Stripe por account cifra a chave API e o segredo de assinatura
em `billing_connections` com AES-256-GCM. O nonce é único por escrita; account,
conexão e finalidade são autenticados como dados associados. A chave mestra vem
de `BILLING_CREDENTIAL_ENCRYPTION_KEY`, em base64, somente no servidor.
Conexões antigas que guardam `env://` continuam resolvendo segredos do ambiente.

Uma integração nova permanece `PENDING_SETUP` até receber o segredo do webhook.
Clientes Stripe gerados automaticamente usam chave de idempotência estável e
uma operação persistida por conexão. Se o resultado remoto ficar incerto, a
operação permanece pendente para investigação, sem repetir a criação às cegas.

O admin-ui recebe apenas indicadores de configuração e os metadados públicos da
conta. A URL do webhook combina seu caminho com `PUBLIC_API_BASE_URL`; registrar
o segredo não comprova que o Stripe entregou eventos.
