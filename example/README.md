# TaskLab

TaskLab é uma POC para testar uma aplicação cliente da Subscription. A aplicação local conhece somente a API da Subscription: o checkout envia o plano, o tipo da compra, o pacote quando aplicável, uma transação e uma chave de idempotência. Credenciais, cartão de pagamento, conexão do provedor, decisão de aprovação ou recusa e webhook ficam na Subscription.

O produto de demonstração é uma tarefa nomeada que custa 1 crédito. O modo pré-pago oferece uma recarga de R$ 10,00 por 10 créditos. O modo de assinatura custa R$ 29,90 por mês e concede 50 créditos depois que o ciclo é confirmado. Cada conta escolhe uma modalidade no primeiro acesso; a conta `admin` / `admin` é semeada como uma conta de demonstração comum.

## Preparar

1. Configure na Subscription `ACCOUNTS_WEBHOOK_SECRET` e o segredo de criptografia das conexões de cobrança. O sandbox de checkout é opcional e permanece desligado por padrão.
2. Para exercitar pagamentos, habilite o sandbox **somente no ambiente da Subscription** com `BILLING_SANDBOX_ENABLED=true`, credenciais de teste do provedor, o segredo de assinatura de webhook e `BILLING_SANDBOX_PAYMENT_SCENARIO=APPROVED` ou `DECLINED`. A Subscription escolhe o meio de pagamento de teste; a TaskLab nunca recebe esses segredos nem envia dados de cartão.
3. Copie `.env.example` para `.env` e defina `ACCOUNTS_WEBHOOK_SECRET` com o mesmo segredo configurado na Subscription. `DATABASE_URL` já aponta para um arquivo SQLite local.
4. Execute `npm install` e depois `npm run setup`. O setup cria o catálogo e as ofertas por HTTP e grava os identificadores em `catalog_settings`; repeti-lo reutiliza os identificadores já persistidos.
5. Na raiz do repositório, rode `make run`. O comando inicia os backends da Subscription e TaskLab e os frontends admin-ui e TaskLab juntos. Abra o admin em [http://localhost:5173](http://localhost:5173) e a TaskLab em [http://localhost:5174](http://localhost:5174).

O ingresso de webhook de pagamento deve apontar diretamente para a Subscription em `/v1/billing/webhooks/stripe`. Não configure webhook nem credencial de pagamento na TaskLab.

## Comandos

- `npm run setup`: cria ou retoma o catálogo remoto da POC.
- `npm run dev`: inicia o backend Axum e o frontend Vite.
- `npm test`: executa testes do backend Rust, Vitest e Playwright.
- `npm run build`: verifica TypeScript e gera a build web.
- `make run` (na raiz): inicia Subscription (`3000`), admin-ui (`5173`), backend TaskLab (`3001`) e frontend TaskLab (`5174`).
- `make monitoring` (na raiz): acompanha memória RSS e uso de CPU dos dois backends Rust; encerre com `Ctrl+C`.

O saldo, extrato, elegibilidade, medidor e consumo são consultados na Subscription. O SQLite local guarda usuários com hash de senha, sessões, referências de checkout e histórico das execuções. Se uma resposta de consumo se perder, a repetição reutiliza a mesma transação e chave de idempotência.

Esta primeira versão não inclui renovação automática, cancelamento, troca de plano nem recarga de uma conta assinante. Os campos fictícios de cartão existem apenas na interface e não são enviados nem armazenados.
