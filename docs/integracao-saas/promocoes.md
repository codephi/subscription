# Vouchers e cupons

Vouchers concedem créditos persistentes à wallet do workspace. Sua validade
define somente quando o código pode ser resgatado. Cupons reduzem o preço de
uma compra inicial de assinatura, uma compra avulsa de créditos ou ambas; eles
nunca alteram o benefício contratado e não se aplicam a renovação,
regularização ou troca de plano.

Os códigos são normalizados para ASCII maiúsculo e únicos em cada tipo.
Vouchers e cupons têm validade opcional, limite total opcional e limite por
workspace padrão de um uso. Limites nulos são ilimitados. Alterações de validade,
limites e estado geram histórico versionado. Código e benefício são imutáveis;
arquivamento é terminal.

## Gestão administrativa

- `POST|GET /v1/admin/vouchers` e `POST|GET /v1/admin/coupons` cadastram e
  paginam promoções. A lista aceita `cursor`, `limit`, `status` e `search`.
- `GET|PATCH /v1/admin/{vouchers|coupons}/{id}` lê ou atualiza estado,
  validade e limites com `expected_version`.
- `GET /v1/admin/{vouchers|coupons}/{id}/history` retorna snapshots anteriores
  e posteriores.
- As respostas mostram usos concluídos, reservas em andamento e disponibilidade
  derivada (`AVAILABLE`, `NOT_STARTED`, `EXPIRED`, `EXHAUSTED`, `DISABLED` ou
  `ARCHIVED`). A validade vazia significa sem expiração.

Percentuais são pontos-base inteiros: `1000` significa 10%, `10000` significa
100%. Valores fixos são inteiros em unidades monetárias menores e exigem moeda
compatível com o plano/oferta. O desconto percentual arredonda para baixo; o
desconto final é limitado ao preço original. Descontos sem efeito após
arredondamento são rejeitados.

## Resgatar voucher

```http
POST /v1/workspaces/{workspace_id}/voucher-redemptions
Idempotency-Key: voucher-redemption-2026-001
Content-Type: application/json

{"voucher_id":"<UUID>","code":null,"transaction_id":"voucher-redemption-2026-001","description":"Campanha de boas-vindas"}
```

Envie `voucher_id` ou `code`, nunca ambos. A API obtém os créditos do cadastro,
valida workspace, validade e limites e, em uma transação, registra o resgate,
cria lançamento `VOUCHER_CREDIT`, lote persistente, referências, auditoria e
outbox. Retentativas com a mesma chave e operação não repetem o crédito.

## Cotar e confirmar uma compra

`POST /v1/workspaces/{workspace_id}/checkout-quotes` calcula o preço, desconto,
total, moeda e créditos contratados sem reservar uso. Informe o `customer_plan_id`,
`checkout_kind` (`INITIAL` ou `ON_DEMAND`), oferta de créditos quando aplicável
e `coupon_code`.

Após revisar a cotação, `POST /v1/workspaces/{workspace_id}/checkouts` cria a
operação com `Idempotency-Key`, os mesmos dados comerciais, `transaction_id` e,
se o total for positivo, o `payment_method_binding_id` do workspace. Para
contratação inicial, crie e guarde o `CustomerPlan` antes do checkout. O admin-ui
persiste a referência da adesão e reutiliza os mesmos identificadores ao
recuperar a operação.

A reserva de uso global e por workspace é confirmada junto da criação da
cobrança e fica vinculada aos snapshots de preço, desconto, moeda e versão do
cupom. Webhook validado consome a reserva uma única vez; falha terminal libera
o uso; resultado incerto preserva a reserva. Estornos externos não devolvem
usos concluídos automaticamente.

Quando o total chega a zero, checkout conclui atomicamente com estado
`COMPLETED`, `payment_required=false` e sem `collection_request_id`, provedor ou
pagamento fictício. Retentativas retornam a conclusão existente. Para total
positivo, Billing persiste o valor líquido e o webhook precisa coincidir com
esse snapshot.

## Admin-ui e limites

Em **Promoções**, o operador pode cadastrar e editar os dois recursos, consultar
histórico e contadores, resgatar voucher, escolher workspace/oferta/cartão,
revisar cotação e acompanhar o checkout. O admin-ui é interno e não tem login;
mantenha-o e as rotas administrativas em rede confiável. A implementação de
Compensation permanece pendente e mantém a Fase 9 aberta.
