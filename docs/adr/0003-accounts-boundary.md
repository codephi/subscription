# ADR 0003: fronteira com Accounts

## Decisão

Accounts continua dono de usuários, accounts, membros e permissões.
Subscription persiste apenas o UUID global do account, estado operacional,
sequência externa e timestamps de processamento.

A autenticação geral foi adiada por decisão explícita. Rotas comuns e
administrativas ficam abertas nesta etapa e referências de ator não são tratadas
como identidade verificada. A entrada assíncrona de Accounts continua protegida
por assinatura HMAC do corpo bruto e timestamp.

## Consequências

- não são persistidos usuários, membros ou permissões;
- `account_id` de caminho e payload deve coincidir quando ambos existirem;
- a ausência temporária de autorização deve permanecer explícita no OpenAPI e
  no README, sem headers livres fingindo fornecer identidade confiável.
