# TaskLab

TaskLab é uma POC para testar uma aplicação cliente da Subscription. A TaskLab chama apenas contratos de negócio da Subscription para catálogo, checkout, carteira e assinaturas. O Subscription escolhe e integra o provedor, devolve uma URL hospedada opaca, e a TaskLab redireciona o navegador para concluir o pagamento. PAN/CVC não passam pela TaskLab.

Veja a [documentação completa da integração com a Subscription](../docs/integracao-saas/tasklab.md) para os contratos remotos, provisionamento do account, catálogo, checkout, medição de uso e limites atuais.

O frontend usa shadcn/ui com preset Base Nova e Base UI. Os controles visuais vêm dos componentes em `web/src/components/ui`; para consultar a configuração e adicionar novos componentes, use `npm run ui:info` e `npm run ui:add -- <componente>`.

O produto de demonstração é uma tarefa nomeada que custa 1 crédito. O modo pré-pago oferece créditos avulsos; o checkout pode solicitar consentimento para guardar o cartão e aceitar um apelido opcional. A carteira permite cadastrar, renomear e remover cartões. O catálogo também inclui “Tasklab — teste de recorrência”, mensal por R$ 1,00 e 10 créditos por ciclo. A recorrência real é mensal; ciclos futuros podem ser simulados com Stripe Test Clocks no ambiente de teste. Cada conta escolhe uma modalidade no primeiro acesso.

## Preparar

1. Configure na Subscription `ACCOUNTS_WEBHOOK_SECRET` e o segredo de criptografia das conexões de cobrança. O sandbox de checkout é opcional e permanece desligado por padrão.
2. Para exercitar pagamentos, habilite o sandbox **somente no ambiente da Subscription** com `BILLING_SANDBOX_ENABLED=true`, credenciais de teste do provedor, o segredo de assinatura de webhook e `BILLING_SANDBOX_PAYMENT_SCENARIO=APPROVED` ou `DECLINED`. O setup de cartão abre a página hospedada da Stripe e aceita os cartões de teste padrão.
3. Copie `.env.example` para `.env` e defina `ACCOUNTS_WEBHOOK_SECRET` com o mesmo segredo configurado na Subscription. Configure `APP_PUBLIC_URL` com o origin público da TaskLab (HTTPS em produção); `DATABASE_URL` já aponta para um arquivo SQLite local.
4. Execute `npm install` e depois `npm run setup`. O setup cria o catálogo e as ofertas por HTTP e grava os identificadores em `catalog_settings`; repeti-lo reutiliza os identificadores já persistidos.
5. Na raiz do repositório, rode `make run`. O comando inicia os backends da Subscription e TaskLab e os frontends admin-ui e TaskLab juntos. Abra o admin em [http://localhost:5173](http://localhost:5173) e a TaskLab em [http://localhost:5174](http://localhost:5174).

Quando `BILLING_SANDBOX_ENABLED=true`, `make run` também inicia o encaminhamento de webhooks para a Subscription. Configure as credenciais de teste e o segredo de webhook somente no ambiente da Subscription.

O webhook de pagamento deve chegar diretamente à Subscription. Não configure webhook nem credencial de pagamento na TaskLab.

Esta versão usa os contratos de Account (`account_id`, eventos `account.*` e
rotas `/v1/accounts/{account_id}`). O banco SQLite local também usa uma
migration inicial atualizada: configure `DATABASE_URL` para um arquivo novo ou
faça backup e recrie o arquivo local antes de iniciar o exemplo. O setup não
apaga bancos automaticamente.

## Comandos

- `npm run setup`: cria ou retoma o catálogo remoto da POC.
- `npm run dev`: inicia o backend Axum e o frontend Vite.
- `npm test`: executa testes do backend Rust, Vitest e Playwright.
- `npm run build`: verifica TypeScript e gera a build web.
- `npm run ui:info`: exibe a configuração shadcn do frontend.
- `npm run ui:add -- <componente>`: adiciona um componente shadcn a `web/src/components/ui`.
- `make run` (na raiz): inicia Subscription (`3000`), admin-ui (`5173`), backend TaskLab (`3001`), frontend TaskLab (`5174`) e, com o sandbox ativo, encaminhamento Stripe CLI para webhooks.
- `make monitoring` (na raiz): acompanha memória RSS e uso de CPU dos dois backends Rust; encerre com `Ctrl+C`.

### Chamadas feitas por `npm run setup`

O script executa `cargo run --manifest-path backend/Cargo.toml -- setup`. Ele conecta ao SQLite local e à API configurada em `SUBSCRIPTION_API_URL` e provisiona o catálogo da TaskLab. Cada identificador remoto é gravado em `catalog_settings`; quando já existe, a criação daquele recurso é ignorada.

| Método e endpoint | Finalidade | Quando é chamado |
| --- | --- | --- |
| `POST /v1/products` | Cria o produto TaskLab, com medição por créditos. | Se `product_id` não estiver salvo. |
| `POST /v1/products/{product_id}/items` | Cria o item “Tarefa”, medido em unidades. | Se `item_id` não estiver salvo. |
| `POST /v1/items/{item_id}/price-versions` | Cria o preço unitário que consome 1 crédito por tarefa. | Se `price_version_id` não estiver salvo. |
| `GET /v1/price-versions/{price_version_id}` | Consulta o estado do preço. | Sempre que o setup chega à etapa do preço; publica-o se não estiver `ACTIVE`. |
| `POST /v1/price-versions/{price_version_id}/publish` | Publica o preço. | Apenas se a consulta anterior indicar estado diferente de `ACTIVE`. |
| `GET /v1/items/{item_id}` | Consulta o estado e a versão do item. | Sempre que o setup chega à etapa de ativação; ativa-o se necessário. |
| `PATCH /v1/items/{item_id}` | Define o item como `ACTIVE`, enviando `expected_version`. | Apenas se o item não estiver `ACTIVE`. |
| `GET /v1/products/{product_id}` | Consulta o estado e a versão do produto. | Sempre que o setup chega à etapa de ativação; ativa-o se necessário. |
| `PATCH /v1/products/{product_id}` | Define o produto como `ACTIVE`, enviando `expected_version`. | Apenas se o produto não estiver `ACTIVE`. |
| `POST /v1/subscriptions` | Cria as assinaturas “TaskLab pré-pago” e “TaskLab mensal”, ambas com modelo `CREDIT_STRICT`. | Uma chamada por assinatura cujo ID ainda não esteja salvo. |
| `POST /v1/subscriptions/{subscription_id}/plans` | Cria o plano grátis/pré-pago e o plano mensal de R$ 29,90, que concede 50 créditos. | Uma chamada por plano cujo ID ainda não esteja salvo. |
| `POST /v1/subscriptions/{prepaid_subscription_id}/on-demand-plans` | Cria opções de recarga de 10, 25 e 50 créditos. | Uma chamada por recarga cujo ID ainda não esteja salvo. |

Os `POST` de criação e `PATCH` de ativação não enviam chave de idempotência. A retomada do setup é feita pelos IDs locais persistidos. O setup também grava esses IDs no SQLite com `INSERT ... ON CONFLICT`, sem fazer chamadas HTTP para isso. Para recarga de 10 créditos, um `topup_plan_id` legado, se existente, é reaproveitado antes de criar ou recuperar `topup_10_plan_id`.

O saldo, extrato, elegibilidade, medidor e consumo são consultados na Subscription. O SQLite local guarda usuários com hash de senha, sessões, referências de checkout e histórico das execuções. Se uma resposta de consumo se perder, a repetição reutiliza a mesma transação e chave de idempotência.

Esta primeira versão não inclui renovação automática, cancelamento, troca de plano nem recarga de uma conta assinante. O fluxo de tokenização e os contratos usados estão descritos em `docs/integracao-saas/tasklab.md`.
