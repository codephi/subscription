# ADR 0004: eventos por HTTP assinado

## Decisão

Integrações usam inbox/outbox duráveis e entrega ao menos uma vez por HTTP. O
envelope contém identidade, tipo, versão de schema, agregado, sequência,
`occurred_at`, `workspace_id`, `correlation_id`, `causation_id` e payload.

Assinaturas usam HMAC-SHA256 sobre `<timestamp>.<raw-body>`. Entradas fora da
janela de cinco minutos ou com assinatura inválida são rejeitadas antes da
desserialização. Entregas de saída usam URL e segredo fornecidos pelo ambiente;
segredos não são persistidos.

## Consequências

- duplicatas são deduplicadas por `event_id`;
- reuso de `event_id` com envelope diferente retorna `409 workspace_event_identity_conflict`
  e preserva o evento original, inclusive quando o novo envelope aponta para outro workspace;
- ordem só é presumida dentro do workspace/agregado;
- sequência posterior aguarda a entrega das anteriores, inclusive durante lease,
  retry e dead-letter; replay preserva identidade e desbloqueia o agregado somente
  após entrega bem-sucedida (regressão RG-03 da revisão de 2026-09-08);
- lacunas ficam em quarentena;
- retry e dead-letter preservam o mesmo evento e permitem replay auditável.
