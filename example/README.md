# TaskLab

TaskLab é uma POC para testar uma aplicação cliente da Subscription. O checkout envia o plano, o pacote, uma referência de cartão tokenizado e uma chave de idempotência. O formulário encaminha dados à Subscription para validação e salvamento opcional, permite nomear os cartões salvos e exibe o nome junto dos últimos quatro dígitos; cartões salvos também podem ser removidos pela mesma integração. A TaskLab não integra diretamente com Stripe.

Veja a [documentação completa da integração com a Subscription](../docs/integracao-saas/tasklab.md) para os contratos remotos, provisionamento do workspace, catálogo, checkout, medição de uso e limites atuais.

O frontend usa shadcn/ui com preset Base Nova e Base UI. Os controles visuais vêm dos componentes em `web/src/components/ui`; para consultar a configuração e adicionar novos componentes, use `npm run ui:info` e `npm run ui:add -- <componente>`.

O produto de demonstração é uma tarefa nomeada que custa 1 crédito. O modo pré-pago oferece pacotes de 10, 25 ou 50 créditos por R$ 10,00, R$ 25,00 ou R$ 50,00; o checkout mostra o progresso enquanto a intenção é processada. O modo de assinatura custa R$ 29,90 por mês e concede 50 créditos depois que o ciclo é confirmado. Cada conta escolhe uma modalidade no primeiro acesso; a conta `admin` / `admin` é semeada como uma conta de demonstração comum.

## Preparar

1. Configure na Subscription `ACCOUNTS_WEBHOOK_SECRET` e o segredo de criptografia das conexões de cobrança. O sandbox de checkout é opcional e permanece desligado por padrão.
2. Para exercitar pagamentos, habilite o sandbox **somente no ambiente da Subscription** com `BILLING_SANDBOX_ENABLED=true`, credenciais de teste do provedor, o segredo de assinatura de webhook e `BILLING_SANDBOX_PAYMENT_SCENARIO=APPROVED` ou `DECLINED`. Para enviar dados de cartão direto no formulário de teste, a conta Stripe também deve ter habilitado acesso à API de dados brutos de cartão. Essa opção serve apenas para sandbox; PAN/CVC passam transitoriamente pela API TaskLab e não são armazenados.
3. Copie `.env.example` para `.env` e defina `ACCOUNTS_WEBHOOK_SECRET` com o mesmo segredo configurado na Subscription. `DATABASE_URL` já aponta para um arquivo SQLite local.
4. Execute `npm install` e depois `npm run setup`. O setup cria o catálogo e as ofertas por HTTP e grava os identificadores em `catalog_settings`; repeti-lo reutiliza os identificadores já persistidos.
5. Na raiz do repositório, rode `make run`. O comando inicia os backends da Subscription e TaskLab e os frontends admin-ui e TaskLab juntos. Abra o admin em [http://localhost:5173](http://localhost:5173) e a TaskLab em [http://localhost:5174](http://localhost:5174).

Quando `BILLING_SANDBOX_ENABLED=true`, `make run` também inicia o Stripe CLI para encaminhar webhooks à Subscription. Instale-o com `brew install stripe-cli`; configure `STRIPE_SECRET_KEY` e o segredo exibido por `stripe listen --print-secret` no `.env` da raiz. O script valida se o segredo corresponde ao listener antes de iniciar.

O ingresso de webhook de pagamento deve apontar diretamente para a Subscription em `/v1/billing/webhooks/stripe`. Não configure webhook nem credencial de pagamento na TaskLab.

## Comandos

- `npm run setup`: cria ou retoma o catálogo remoto da POC.
- `npm run dev`: inicia o backend Axum e o frontend Vite.
- `npm test`: executa testes do backend Rust, Vitest e Playwright.
- `npm run build`: verifica TypeScript e gera a build web.
- `npm run ui:info`: exibe a configuração shadcn do frontend.
- `npm run ui:add -- <componente>`: adiciona um componente shadcn a `web/src/components/ui`.
- `make run` (na raiz): inicia Subscription (`3000`), admin-ui (`5173`), backend TaskLab (`3001`), frontend TaskLab (`5174`) e, com o sandbox ativo, encaminhamento Stripe CLI para webhooks.
- `make monitoring` (na raiz): acompanha memória RSS e uso de CPU dos dois backends Rust; encerre com `Ctrl+C`.

O saldo, extrato, elegibilidade, medidor e consumo são consultados na Subscription. O SQLite local guarda usuários com hash de senha, sessões, referências de checkout e histórico das execuções. Se uma resposta de consumo se perder, a repetição reutiliza a mesma transação e chave de idempotência.

Esta primeira versão não inclui renovação automática, cancelamento, troca de plano nem recarga de uma conta assinante. O fluxo de tokenização e os contratos usados estão descritos em `docs/integracao-saas/tasklab.md`.
