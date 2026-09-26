# Frontend administrativo: funcionalidades e operação

Este documento descreve o frontend implementado em `admin-ui/`: páginas,
consultas, ações, integração com a API, decisões técnicas, execução e limites.
O frontend é um painel operacional interno do serviço Subscription; ele não é
uma interface pública para clientes.

## Navegação e páginas

| Rota                                             | Página                    | O que oferece                                                                                |
| ------------------------------------------------ | ------------------------- | -------------------------------------------------------------------------------------------- |
| `/`                                              | Visão geral               | Indicadores de operação de Billing e atalhos para investigar cada fila.                      |
| `/workspaces`                                    | Workspaces                | Lista paginada, busca direta por UUID e acesso aos detalhes.                                 |
| `/workspaces/:workspaceId`                       | Detalhe do workspace      | Estado operacional, provisionamento, carteira, planos, extrato e consumo.                    |
| `/workspaces/:workspaceId/actions`               | Ações do workspace        | Configuração de Billing, conexão Stripe por referências, crédito direto e reconciliações.    |
| `/workspaces/:workspaceId/plans/:planId/actions` | Ações do plano do cliente | Cancelamento ao fim do período, transição e revogação.                                       |
| `/billing/:kind`                                 | Fila de Billing           | Listagem filtrável de cobranças, tentativas, pagamentos, webhooks, outbox e não conciliados. |
| `/billing/:kind/:id`                             | Registro de Billing       | Detalhe, IDs relacionados e replay de dead letter.                                           |
| `/catalog/:kind`                                 | Catálogo                  | Listagem paginada por tipo de registro e entrada para criação.                               |
| `/catalog/:kind/new`                             | Criar oferta              | Formulários para cada tipo suportado do catálogo.                                            |
| `/catalog/:kind/:id`                             | Detalhe do catálogo       | Contrato retornado pela API; publicar rascunho de preço ou revogar versão de plano.          |
| `/audit`                                         | Auditoria                 | Eventos paginados com filtros de workspace e ação.                                           |
| `/inbox`                                         | Inbox de Accounts         | Metadados de eventos recebidos, filtros e replay de evento em quarentena.                    |

As páginas são carregadas sob demanda quando a rota é aberta. Durante a carga,
a aplicação mostra um estado de espera. Rotas desconhecidas mostram uma página
de “não encontrada”.

## Funcionalidades por área

### Visão geral

A tela consulta `GET /v1/admin/billing/operations` e mostra seis contadores:

- cobranças pendentes;
- falhas de webhook;
- webhooks ainda sem processamento final;
- pagamentos não conciliados abertos;
- eventos pendentes na outbox;
- dead letters na outbox.

Cada contador abre a fila correspondente com o estado que originou o alerta.
Os números são atualizados a cada 30 segundos. A tela também oferece acesso à
consulta de workspaces.

### Workspaces, planos, carteira e consumo

A lista consulta `GET /v1/admin/workspaces` em páginas de até 20 registros e
mostra o UUID, o estado operacional, a sequência externa e a data de atualização.
O campo de busca aceita um UUID completo; após validar o formato, consulta o
detalhe e abre o workspace. Nomes e identidades de usuários não são exibidos,
pois esses dados pertencem ao Accounts e não fazem parte da projeção do
Subscription.

O detalhe reúne consultas independentes para mostrar:

- estado operacional do workspace, sequência recebida e data de atualização;
- status de provisionamento e quantidade de item wallets materializadas em
  relação à quantidade esperada;
- saldo da carteira de créditos;
- planos do cliente, estados comerciais e de renovação, versão e ciclo atual;
- extrato de créditos com navegação por cursor;
- carteiras de itens e, depois da seleção de um item, medidor e extrato de uso.

O medidor e o extrato de uso são carregados apenas para o item selecionado. A
ação “Conferir uso” solicita a reconciliação do item e exibe o resultado da API.
As chamadas de workspace, carteira e provisionamento são independentes; uma
falha em uma consulta não transforma os valores das demais em resultados
confirmados.

### Investigação de Billing

As filas disponíveis são `collections`, `attempts`, `payments`, `webhooks`,
`outbox` e `unmatched`. Cada lista é paginada e mostra os identificadores e
estados disponíveis para o tipo de registro. Os filtros incluem workspace e
estado; também é possível filtrar por cobrança (`collection_request_id`) e
correlação (`correlation_id`). O estado usado pelos atalhos da visão geral e os
IDs de cobrança/correlação são representados na URL.

O detalhe apresenta, quando disponíveis, workspace, cobrança, tentativa,
pagamento, correlação, evento e pagamento do provedor, tipo, horário, valor em
unidades menores, moeda e dados de falha. Links levam a registros relacionados
e ao workspace correspondente. Eventos da outbox em `DEAD_LETTER` podem ser
reenfileirados após confirmação explícita.

### Catálogo e ofertas

O catálogo oferece consulta por cursor e detalhe para produtos, itens, preços,
assinaturas, planos, ofertas avulsas e políticas de admissão. As listas mostram
nome, ID, vínculo, estado e data de criação. O detalhe apresenta os campos
retornados pela API e identifica versões imutáveis.

Os formulários de criação cobrem:

- **Produto:** nome, descrição e uma escolha em linguagem simples entre
  “Consumo cobrado em créditos” e “Acesso por assinatura, sem cobrança por
  consumo”. A tela informa que, na versão atual, somente produtos com consumo
  medido podem ser publicados. Os valores do contrato (`CREDIT_METERED` e
  `ENTITLEMENT_ONLY`) continuam iguais no envio à API.
- **Item:** produto pai, nome, item pai opcional, unidade e escala de quantidade.
- **Preço:** item, período de vigência e conversão; suporta preço unitário e
  faixas de preço com blocos e créditos.
- **Assinatura:** nome e modelo de assinatura.
- **Plano:** assinatura, modelo comercial, valor em unidade menor, moeda,
  recorrência, política de admissão, créditos concedidos e produtos associados.
- **Oferta avulsa:** assinatura, nome, preço, moeda e unidades de crédito.
- **Política de admissão:** identificador, versão e um fato obrigatório
  (`EMAIL_VERIFIED` ou `IDENTITY_VERIFIED`).

Preço é criado como rascunho. A publicação só é oferecida para um rascunho e
exige confirmação; versões publicadas são apresentadas como imutáveis. A página
de detalhe de um plano de catálogo permite revogar a versão com motivo e
referência operacional, depois de confirmar.

### Ações de workspace

Na configuração de Billing, o operador pode alterar as permissões de crédito
direto e recorrente. O envio usa a versão lida da configuração, permitindo que a
API detecte alterações concorrentes.

A conexão Stripe recebe o identificador da conta externa e referências a
variáveis de ambiente, por exemplo `env://STRIPE_SECRET_KEY` e
`env://STRIPE_WEBHOOK_SECRET`. O formulário não recebe nem envia o conteúdo dos
segredos.

O crédito direto pede ID de transação, unidades de crédito e descrição opcional.
Antes de enviar, mostra uma confirmação com workspace, unidades e transação. O
botão fica indisponível quando crédito direto está desabilitado para o workspace.
Em caso de sucesso, a tela mostra as referências do lote e do lançamento e
atualiza as consultas de carteira e extrato.

A área de reconciliação permite conferir o saldo da carteira contra o extrato e
reconciliar o provisionamento. O resultado mostra consistência, saldos e
quantidade de item wallets materializadas/esperadas.

### Ações de plano do cliente

A página consulta o estado atual do plano antes de disponibilizar suas ações.
Permite:

- agendar cancelamento ao fim do período, com confirmação;
- solicitar transição `UPGRADE` ou `DOWNGRADE`, informar a nova versão, vínculo
  de pagamento opcional, transação e referência operacional, revisar e confirmar;
- revogar imediatamente com motivo e referência operacional.

Após ação bem-sucedida, os dados do plano e a lista de planos do workspace são
consultados novamente. O resultado retornado pela API é exibido na página.

### Auditoria e inbox

Auditoria lista horário, ação, workspace, recurso, correlação e referência
operacional. Pode ser filtrada por UUID de workspace e texto da ação.

Inbox lista metadados de eventos do Accounts: ID do evento, workspace, tipo,
sequência externa, estado e horário de recebimento. O filtro aceita workspace e
estado. Eventos `QUARANTINED` oferecem replay com confirmação. A lista não
carrega nem exibe o payload recebido.

## Dados, estado e integração com a API

O frontend usa Vite, React, TypeScript, React Router, Tailwind CSS v4,
shadcn/ui no estilo `base-nova` com primitivas Base UI, ícones Lucide e Zustand.
Componentes shadcn são mantidos como código do projeto em
`admin-ui/src/components/ui/`.

A folha global importa Tailwind, animações shadcn e a fonte Geist Variable. Os
componentes de interface locais incluem Alert, AlertDialog, Badge, Button, Card,
Checkbox, Empty, Field, Input, Label, Select, Separator, Skeleton e Table. A
navegação e os formulários usam cores semânticas do tema shadcn.

O botão compacto de tema no canto superior direito da área principal alterna
entre claro e escuro, aplica a classe `dark` na raiz do documento e guarda a
escolha em `localStorage`. A preferência é aplicada antes de montar o React para
evitar que a página pisque no tema errado ao abrir ou recarregar.

O Zustand guarda preferências de interface em memória: texto de busca de
workspace, item selecionado e filtros compartilhados de workspace/estado de
Billing. Não guarda respostas remotas. Consultas e mutações usam TanStack Query,
com cache, invalidação após gravações, uma tentativa automática de retry e
validade padrão de 15 segundos. A visão geral também atualiza seus indicadores a
cada 30 segundos.

O cliente HTTP tipado está dividido por responsabilidade em
`admin-ui/src/api/`: workspace/carteiras, Billing, catálogo, planos e operações.
Os tipos em `generated.ts` vêm do snapshot `openapi.json`. Para atualizar o
contrato e os tipos após mudança de endpoint ou DTO, dentro de `admin-ui/`, rode:

```sh
npm run api:types
```

### Endpoints usados pela interface

- **Workspaces e leitura:** `GET /v1/admin/workspaces`,
  `GET /v1/admin/workspaces/{workspace_id}`,
  `GET /v1/admin/workspaces/{workspace_id}/customer-plans`,
  `GET /v1/workspaces/{workspace_id}/wallets`,
  `GET /v1/workspaces/{workspace_id}/wallet-provisioning`,
  `GET /v1/workspaces/{workspace_id}/customer-wallet/statement`,
  `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet` e
  `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement`.
- **Uso e provisionamento:**
  `POST /v1/admin/workspaces/{workspace_id}/items/{item_id}/usage/reconcile` e
  `POST /v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile`.
- **Billing e carteira:** `GET /v1/admin/billing/operations`,
  `GET /v1/admin/billing/records/{kind}`,
  `GET /v1/admin/billing/records/{kind}/{id}`,
  `GET`/`PUT /v1/workspaces/{workspace_id}/billing-config`,
  `POST /v1/workspaces/{workspace_id}/credits/direct`,
  `POST /v1/workspaces/{workspace_id}/billing-connections`,
  `POST /v1/admin/workspaces/{workspace_id}/customer-wallet/reconcile` e
  `POST /v1/admin/outbox-events/{event_id}/replay`.
- **Planos de cliente:**
  `GET /v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}`;
  `POST /v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/cancel`;
  `POST /v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/plan-transitions`;
  `POST /v1/admin/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/revoke`.
  O endpoint de transição recebe `Idempotency-Key`.
- **Catálogo:** `GET /v1/admin/catalog/{kind}`; detalhes em
  `/v1/products/{product_id}`, `/v1/items/{item_id}`,
  `/v1/price-versions/{price_id}`, `/v1/subscriptions/{subscription_id}`,
  `/v1/subscription-plans/{plan_id}`,
  `/v1/on-demand-plans/{on_demand_plan_id}` e
  `/v1/admission-policies/{policy_version_id}`. Criações usam `POST
/v1/products`, `/v1/products/{product_id}/items`,
  `/v1/items/{item_id}/price-versions`, `/v1/subscriptions`,
  `/v1/subscriptions/{subscription_id}/plans`, `/v1/admission-policies` e
  `/v1/subscriptions/{subscription_id}/on-demand-plans`. Publicação e revogação
  usam `POST /v1/price-versions/{price_id}/publish` e
  `POST /v1/subscription-plans/{plan_id}/revoke`.
- **Auditoria e inbox:** `GET /v1/admin/audit-events`,
  `GET /v1/admin/integration-inbox` e
  `POST /v1/admin/integration-inbox/{event_id}/replay`.

Os segmentos `{kind}` e os campos enviados são definidos pelo cliente OpenAPI
gerado; não são usados endpoints montados a partir de nomes livres do usuário.

IDs de workspace, oferta, evento e operação são mostrados como identificadores,
sem resolução para nomes externos. Valores de crédito, quantidade e unidades
permanecem strings decimais no cliente para respeitar o contrato da API; valores
monetários em unidades menores são apresentados como retornados. Datas UTC são
formatadas no fuso local do navegador.

### Erros e ações repetidas

O cliente transforma respostas de erro em `ApiRequestError` com HTTP, código e
mensagem estruturada. O feedback distingue indisponibilidade `503` e exibe a
referência `existing_operation` quando ela vem em uma resposta de conflito.
Consultas têm estados de carregamento, erro e resultado vazio.

Crédito direto e transição de plano geram uma chave de idempotência por escopo e
transação. A chave fica no `localStorage` até a API confirmar a ação; novas
tentativas com a mesma transação reutilizam a chave, e uma transação diferente
recebe outra. Há uma única chave pendente por escopo; iniciar outra transação no
mesmo escopo substitui a chave anterior. Após sucesso, a chave é removida. O
armazenamento é local ao navegador: trocar de navegador ou limpar seus dados
remove a chave, então o operador deve investigar a transação antes de repetir
uma ação de resultado incerto.

Erros de renderização e de consulta são escritos como JSON no console do
navegador, com nível, componente, etapa, tipo, mensagem e horário. Essa
instrumentação não envia dados para um serviço remoto.

## Execução e validação

Com a API disponível em `http://127.0.0.1:3000`, a partir de `admin-ui/`:

```sh
npm ci
npm run dev
```

O servidor de desenvolvimento encaminha `/v1` e `/health` para a API local. Para
usar outra origem, defina `VITE_API_BASE_URL` no ambiente de build/execução do
frontend. Variáveis com prefixo `VITE_` ficam visíveis no navegador; não coloque
segredos nelas.

Comandos de validação:

```sh
npm test
npm run test:e2e
npm run lint
npm run build
npm run preview
```

`npm run build` gera os arquivos estáticos em `admin-ui/dist/`; `npm run preview`
serve essa saída para conferência local. Uma hospedagem estática precisa
redirecionar rotas SPA para `index.html` e manter o backend inacessível fora da
rede autorizada.

Os testes unitários cobrem formatação, chave de idempotência e páginas de visão
geral/workspaces. Os testes Playwright simulam respostas HTTP e cobrem navegação
de investigação de Billing, criação de produto, replay da inbox, crédito direto
com chave de idempotência e verificações axe na visão geral, auditoria e inbox.
Eles validam o comportamento da interface sem depender de dados reais. Os testes
Rust de integração cobrem os contratos e a persistência da API contra PostgreSQL.

## Limites e próximos pré-requisitos

- Não há login, sessão ou autorização por perfil. A API e o frontend precisam
  permanecer restritos à rede local ou VPN; não publique esta interface na
  internet sem implementar autenticação e autorização.
- Referências operacionais/`actor_reference` são textos digitados pelo operador;
  não provam identidade autenticada.
- Cupons, vouchers, compensações, estornos iniciados pelo produto e pausa de
  planos não aparecem como operações do painel, pois seus fluxos de backend não
  estão implementados.
- Telemetria é local ao console; não existe agregador remoto ou painel de erros.
- Os testes e2e usam API simulada. Ainda não há uma suíte de navegador de ponta a
  ponta contra uma instância real do serviço.
- A análise axe automatizada cobre as páginas citadas nos testes; isso não
  substitui uma auditoria completa de acessibilidade para todas as páginas e
  fluxos.
