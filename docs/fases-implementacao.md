# Fases de implementação

## Objetivo

Implementar a API de assinaturas, créditos e consumo em incrementos seguros.
Cada fase deve deixar o sistema utilizável, testado e observável antes que a
próxima introduza regras comerciais ou integrações externas mais complexas.
Este roteiro operacionaliza o
[plano técnico](plano-tecnico-api-assinaturas-rust.md); suas decisões normativas
de domínio prevalecem e qualquer mudança nelas exige atualizar os dois arquivos.

## Escopo de aceite vigente

Conforme o [ADR 0003](adr/0003-accounts-boundary.md), a autenticação geral e a
autorização por escopo foram adiadas por decisão do produto. Nas fases 0–3,
os critérios de credencial/contexto são verificados na entrada assinada de
Accounts. Rotas comuns e administrativas, incluindo reconciliação e replay,
permanecem abertas nesta entrega; a restrição administrativa descrita no roteiro
é um requisito futuro, não uma proteção já implementada. Referências de ator
nessas rotas não representam identidade verificada. O aceite desta etapa não
autoriza exposição fora de uma rede confiável.

## Fronteiras de responsabilidade

- O sistema de Accounts é a fonte de verdade para usuários, autenticação,
  workspaces, membros e permissões. O Subscription não replica nem administra
  esses conceitos.
- O Subscription é dono de catálogo, planos, `CustomerPlan`, wallets, créditos,
  consumo e Billing. Na V1, `workspace_id` é também o `customer_id` de cobrança;
  não existe uma segunda hierarquia local de clientes.
- A integração usa um identificador de workspace global, estável e imutável. O
  contrato deve definir formato, emissor e política para ambientes, mas não
  permite reciclar o ID de um workspace encerrado.
- Chamadas entre serviços usam autenticação serviço-a-serviço e autorização por
  escopo. O contexto confiável do workspace e do ator vem de credencial ou
  envelope assinado e deve coincidir com o `workspace_id` do recurso; headers
  livres informados pelo cliente não são fonte de autorização.
- O núcleo de assinaturas, créditos e consumo não depende de um provedor de
  pagamentos. Stripe é o primeiro adaptador previsto.

## Princípios de execução

- O Subscription mantém apenas uma projeção local mínima do workspace:
  `workspace_id`, estado operacional, versão/sequência externa e timestamps de
  processamento. Dados de usuário, membros e permissões permanecem em Accounts.
- Integrações assíncronas têm inbox/outbox duráveis, envelope versionado,
  `event_id`, agregado, sequência, `occurred_at`, `correlation_id` e
  `causation_id`. A entrega é ao menos uma vez: duplicatas são ignoradas e a
  ordem só é presumida dentro do mesmo workspace/agregado.
- Os eventos de saída cobrem, no mínimo, provisionamento, mudanças efetivas de
  `CustomerPlan`/entitlement, créditos, consumo e os eventos normativos de
  Billing. Publicar um fato não transfere ao consumidor a regra de negócio.
- Evento atrasado não regride a projeção. Lacuna de sequência deixa o workspace
  sem avançar para operações dependentes e dispara retry/replay ou reconciliação;
  não se inventa estado local.
- Provisionamento é idempotente e materializado. Consumo, crédito ou adesão só
  operam após workspace ativo, `customer_wallet` ativa e todas as `item_wallets`
  esperadas para a versão de escopo vigente.
- Valores de crédito e quantidades usam inteiros de 64 bits representados como
  strings decimais nos contratos HTTP.
- Movimentos financeiros e de consumo são append-only: correções criam novos
  registros, nunca alteram ou apagam o histórico.
- Qualquer operação externa que possa alterar crédito ou consumo exige
  `transaction_id` e `Idempotency-Key`.
- Cada fase inclui migrações, contrato OpenAPI ou de eventos, testes unitários,
  testes de integração com PostgreSQL e cenários de concorrência pertinentes.

## Fase 0 — Contratos e decisões executáveis

**Objetivo:** transformar a especificação e a fronteira com Accounts em uma
base de desenvolvimento verificável.

Entregas:

- ADRs para unidades de crédito, razão imutável, idempotência, concorrência e
  limites entre Accounts, Subscription e provedores de pagamento;
- contrato do identificador estável e modelo de autenticação serviço-a-serviço,
  autorização por escopo e propagação do contexto de workspace/ator;
- catálogo versionado de entrada para `workspace.created`,
  `workspace.activated`, `workspace.blocked` e `workspace.terminated`, incluindo
  significado, transições permitidas, sequência e política de replay;
- contrato inicial dos eventos de saída do Subscription, seus consumidores e
  dados permitidos, sem fixar broker ou provedor: famílias de provisionamento,
  `customer_plan`, entitlement, crédito e uso, além de `collection.*`,
  `payment.*` e `external_refund.*` já exigidas pelo plano técnico;
- convenções para IDs, UTC, erros de domínio, paginação por cursor, auditoria e
  observabilidade, além do primeiro recorte do OpenAPI;
- ambiente local PostgreSQL, estratégia de migrações/dados de teste e matriz de
  invariantes do plano técnico.

**Critério de saída:** contratos de HTTP e eventos permitem provar quem autentica,
quem autoriza, como um workspace é correlacionado e quais estados bloqueiam cada
operação, sem criar cadastro local de usuários ou clientes.

## Fase 1 — Fundação transacional e fronteira com Accounts

**Objetivo:** receber o contexto externo com segurança e estabelecer mecanismos
reutilizáveis antes das regras de negócio.

Entregas:

- tipos para IDs de domínio, `credit_units`, `item_units`, períodos e erros;
- middleware de autenticação serviço-a-serviço e validação da igualdade entre o
  workspace autorizado, o caminho e o payload;
- `WorkspaceProjection` mínima e inbox deduplicada por `event_id`, com
  compare-and-set por versão/sequência e quarentena de lacunas ou conflitos;
- outbox transacional, dispatcher com retry e contratos de replay/reconciliação;
- registro de idempotência, unicidade de `transaction_id` por workspace,
  transações SQLx, locks explícitos e auditoria estruturada.

**Critério de saída:** credencial inválida ou contexto divergente não altera
estado; duplicatas, reentregas e eventos fora de ordem convergem para uma única
projeção; falhas antes/depois do commit não perdem nem duplicam inbox/outbox.

## Fase 2 — Catálogo e versões de preço

**Objetivo:** definir o escopo faturável necessário ao provisionamento sem
depender de wallets ou cobrança real.

Entregas:

- `Product`, `Item` e `PriceVersion`, com publicação imutável;
- Products `CREDIT_METERED` publicáveis na V1 e `ENTITLEMENT_ONLY` apenas
  modelado como capacidade futura não ativável;
- preços por unidade ou faixas progressivas, blocos e ciclos declarativos;
- versão do escopo aplicável usada para calcular o conjunto esperado de
  `item_wallets`;
- validações de overflow, faixas, compatibilidade e mudança de versão.

O cálculo e a seleção do escopo são serializados antes da leitura dos itens
aplicáveis; mudanças concorrentes em itens distintos devem preservar todos os
commits no escopo final.

**Critério de saída:** uma versão publicada determina de modo imutável e
reprodutível preços, itens faturáveis e wallets esperadas para provisionamento.

## Fase 3 — Ciclo do workspace e provisionamento de wallets

**Objetivo:** converter o ciclo de vida vindo de Accounts em recursos locais
prontos para operar, sem criação preguiçosa.

Entregas:

- processamento idempotente de criação e ativação para materializar uma
  `customer_wallet` e as `item_wallets` aplicáveis, com saldo inicial zero;
- `Wallet`, `CustomerWallet`, `ItemWallet`, `WalletLifecycleEvent` append-only e
  `WalletProvisioning` por workspace e versão de escopo;
- bloqueio que impede novas mutações de negócio sem apagar saldo, plano ou
  histórico, e encerramento que desabilita recursos preservando auditoria;
- retomada/reprocessamento seguro de provisionamento parcial e endpoint
  administrativo restrito de reconciliação;
- eventos de saída `workspace_provisioning.started`, `.completed` e `.failed`,
  com nomes/schema definitivos estabelecidos no contrato da Fase 0.

Ao retornar a um escopo anterior, contadores históricos não comprovam
prontidão. Consultas e novas operações verificam a customer wallet e todas as
especializações e estados ativos das item wallets esperadas. Uma hierarquia
incompleta exige reconciliação explícita, inclusive quando a operação usa outro
item que permaneceu ativo.

**Critério de saída:** criação/ativação duplicada ou concorrente produz uma única
hierarquia; o workspace só fica pronto com todas as wallets esperadas ativas;
bloqueio/encerramento atrasado é aplicado pela sequência externa e nenhuma
operação nova atravessa o estado efetivo.

## Fase 4 — Razão e créditos

**Objetivo:** entregar o razão confiável que sustenta concessões e débitos.

Entregas:

- `CustomerWalletEntry`, projeção de saldo, concessão direta e extrato;
- `CreditLot`, referências transacionais e reconciliação entre saldo, razão e
  lotes disponíveis;
- regras de imutabilidade, versão otimista e falha atômica antes do commit;
- eventos versionados de crédito após commit, sem incluir membros ou dados
  sensíveis do sistema de Accounts.

O aceite inclui falha injetada no commit, encerramento da conexão PostgreSQL
antes da confirmação e perda da resposta após commit. Reuso de chave ou
transação devolve referência consultável à operação original, quando concluída,
sem expor seu payload. Créditos distintos concorrentes preservam sequência,
versão e saldo; a reconciliação compara a soma integral do razão, seu encadeamento
e os lotes não expirados, sem alterar histórico. A origem do lote é imutável;
saldo residual, classificação efetiva e validade continuam como projeções dos
fluxos de consumo, reclassificação e expiração das fases seguintes.

**Critério de saída:** toda mudança de saldo tem exatamente um lançamento;
créditos concorrentes ou repetidos não duplicam efeitos; workspace não
operacional rejeita integralmente a chamada.

## Fase 5 — Assinaturas, planos e franquia por ciclo

**Objetivo:** modelar o contrato comercial sem depender de cobrança real.

Entregas:

- `Subscription`, `SubscriptionPlanVersion`, `OnDemandPlan`, `CustomerPlan` e
  ciclos materializados;
- adesão a planos gratuitos, política de admissão, entitlement e elegibilidade;
- concessão por ciclo, expiração de lote e transição de plano;
- cancelamento no fim do período e scheduler durável apenas para eventos
  internos de calendário;
- eventos de saída para mudanças efetivas de `CustomerPlan`, entitlement,
  concessão e expiração, correlacionados ao workspace e à transação de origem.

**Critério de saída:** plano gratuito concede e expira franquia uma vez por
ciclo; bloqueio externo não destrói créditos e impede novas mutações conforme o
contrato; consumidores deduplicam eventos pela identidade estável.

Progresso em 2026-09-11: corrigida a contagem do calendário após downgrade,
com migração reversível que preserva ordinais históricos. Regressões verificam
três renovações sem saltos, créditos reclassificados preservados e revogação
concorrente impedindo novo ciclo.

Continuação: elegibilidade considera o entitlement do produto mesmo com outra
Subscription pendente/revogada; downgrade reaplica admissão e não ignora exigência
de cartão. Cancelamento pendente libera o slot imediatamente, cancelamento
recorrente preserva o ciclo em curso e chamadas repetidas não duplicam eventos
nem sobrescrevem estados terminais.

Scheduler de ciclos gratuitos: fila persistida por ciclo, criada e encerrada
na transação do calendário, com backfill de ciclos existentes. Workers concorrentes
usam lease de 60 segundos e retomam reservas expiradas; falhas reagendam o trabalho
em 30 segundos sem bloquear outros workspaces. Testes cobrem reinício, concorrência,
rollback no commit, bloqueio operacional e execução automática. Não executa
cobranças nem consultas a provedores.

Política de admissão: versões imutáveis declaram fatos de e-mail e identidade;
atestados assinados de Accounts são sequenciados, possuem validade e referência
opaca. Adesão e downgrade serializam a avaliação com atualizações da evidência e
registram a evidência exata da decisão. Retirada, expiração, outro workspace,
duplicata alterada e falha de commit não autorizam nem deixam efeitos parciais.
Validação antifraude de cartão continua no fluxo de Billing.

Fase concluída em 2026-09-11: o slot exclusivo acompanha os estados comercial e
de renovação, e a composição de Products do plano publicado rejeita inclusão,
alteração ou remoção tardia. Todos os critérios atribuídos à fase 5 estão cobertos
por testes executáveis na matriz e pelo gate acumulado `check-phase-gate.sh 5`.

## Fase 6 — Medição de consumo

**Objetivo:** converter consumo de itens em débitos de modo determinístico,
depois que o entitlement já pode ser verificado.

Entregas:

- `UsageEvent`, `ItemWalletEntry`, `PricingAccumulator`, `BillingBlock`, `Debit`
  e `CreditLotAllocation`;
- endpoints de consumo, elegibilidade, medidor e extrato por item;
- validação conjunta de workspace, `CustomerPlan`, entitlement, wallet
  materializada e saldo dentro da decisão transacional;
- reconciliação entre item units recebidas, blocos, pendências, alocações e o
  débito correlato na `customer_wallet`;
- eventos versionados de consumo e débito após commit.

**Critério de saída:** consumos concorrentes formam blocos sem perda ou
duplicação; saldo insuficiente, falta de entitlement ou workspace não
operacional rejeita integralmente a chamada.

Fase concluída em 2026-09-11: conversões unitárias e por faixa preservam cada
intervalo recebido em bloco ou pendência, consolidam o débito por chamada e
serializam ItemWallet, acumulador e CustomerWallet. A elegibilidade expõe seus
fatos separadamente sem autorizar o comando posterior; Product fora do plano,
saldo insuficiente, wallet desabilitada e workspace bloqueado não deixam efeitos.
Todos os critérios da fase 6 estão cobertos pelo gate `check-phase-gate.sh 6`.

## Fase 7 — Fronteira de Billing agnóstica ao provedor

**Objetivo:** definir pagamentos sem acoplar Subscription ao Stripe.

Entregas:

- interface `BillingConnector` e capacidades de meio de pagamento;
- `BillingConnection`, `PaymentMethodBinding`, `CollectionRequest`,
  `CollectionAttempt` e `BillingPayment`;
- máquina de estados normalizada para confirmação, falha, autenticação
  adicional, expiração comercial e estado incerto;
- `WebhookInbox`, validação de assinatura, deduplicação e correlação;
- eventos correlacionados de intenção/resultado na outbox, preservando os
  envelopes e garantias definidos na Fase 1.

**Critério de saída:** um conector falso prova que confirmação idempotente ativa
um ciclo e concede créditos uma única vez; atraso ou ausência de webhook não
autoriza nova cobrança, polling, cancelamento ou crédito.

Progresso em 2026-09-12: a confirmação inicial de plano pago usa o contrato
agnóstico `BillingConnector`, converge quando webhook e resposta síncrona chegam
em ordens diferentes e efetiva uma única vez plano, ciclo, entitlement e crédito.
Snapshot divergente, valor/moeda incompatível ou evento duplicado não repetem
efeitos; o lançamento fica ligado de modo imutável à solicitação e ao pagamento.
Solicitações vencidas são reclamadas concorrentemente por prazo comercial e
terminalizadas uma única vez. A adesão inicial é cancelada, a renovação passa a
`PAST_DUE`/`RENEWAL_INACTIVE`, OnDemand não altera o plano principal e uma
confirmação posterior fica rejeitada sem efeitos financeiros ou de acesso.
Falha definitiva de renovação produz o mesmo bloqueio de nova recorrência, sem
retentativa, novo ciclo, nova solicitação ou alteração do saldo já concedido.
Chaves estrangeiras compostas impedem uso cruzado de conexão, cartão tokenizado,
CustomerPlan e solicitação entre workspaces ou customers.
O contrato V1 rejeita outros meios de pagamento antes da chamada externa e não
repete automaticamente uma tentativa cujo resultado no provedor seja incerto.
Renovação paga confirmada conclui o ciclo anterior, preserva a âncora e inicia o
período seguinte com uma única franquia mesmo sob confirmações concorrentes.
Regularização manual materializa uma nova solicitação idempotente para o cartão
tokenizado escolhido. Antes da confirmação mantém `PAST_DUE`; depois reinicia a
âncora no instante confirmado e concede somente a franquia integral vigente.
Pagamento confirmado sem solicitação local abre um caso operacional idempotente,
preserva a evidência bruta e não altera plano, ciclo, entitlement ou crédito.

Fase concluída em 2026-09-14: cancelamento e revogação encerram cobranças
pendentes; recorrência `NONE`, OnDemand persistente e upgrade pago sem prorrata
estão cobertos. O gate acumulado `check-phase-gate.sh 7` está aprovado.

## Fase 8 — Primeiro conector: Stripe e cartão tokenizado

**Objetivo:** habilitar planos pagos e recargas sem armazenar dados sensíveis.

Entregas:

- setup seguro de cartão via superfície hospedada ou componente do Stripe;
- criação idempotente de cobrança e correlação com o domínio local;
- webhooks assinados para pagamento confirmado, falha e autenticação adicional;
- uma tentativa comercial na renovação e regularização somente manual;
- procedimentos para cobrança não conciliada e estorno executado externamente.

**Critério de saída:** eventos duplicados, atrasados ou fora de ordem não
duplicam cobrança, plano, ciclo ou crédito; adicionar outro conector não exige
alterar contratos ou invariantes de Subscription e Wallet.

Fase concluída em 2026-09-14: conexões, capacidades, setup e vínculos
tokenizados estão expostos; o adaptador Stripe cria PaymentIntent idempotente e
o webhook assinado normaliza confirmação, falha e ação adicional. Pagamentos
não correspondidos e estornos externos são observados sem efeito automático.
O gate acumulado `check-phase-gate.sh 8` está aprovado.

## Fase 9 — Promoções e ajustes administrativos

**Objetivo:** completar fontes não recorrentes de crédito com auditoria.

Entregas:

- `Voucher` de créditos persistentes e `Coupon` de desconto para compra inicial
  de assinatura e/ou recarga avulsa; validade e limites total/por workspace,
  reservas concorrentes, histórico e referências no extrato;
- `Compensation` com criação, aprovação quando necessária e execução;
- referências oficiais entre promoções, créditos, pagamentos e extratos;
- autorização administrativa derivada do sistema externo, sem persistir
  membros ou permissões no Subscription.

Voucher e Coupon têm implementação parcial na API e no admin-ui, incluindo
cadastro, edição de limites/validade/estado, histórico, resgate de voucher,
cotação, checkout pago com reserva e checkout integralmente descontado. O
recorte ainda depende da validação completa dos fluxos concorrentes e da suíte
Playwright para ser registrado como concluído. Compensation e autorização
administrativa externa permanecem pendentes; por isso, a Fase 9 continua aberta.

**Critério de saída:** vale, cupom ou Compensation nunca gera crédito duplicado,
inclusive sob concorrência, reentrega e mudança do estado do workspace.

## Fase 10 — Operação, reconciliação e prontidão de produção

**Objetivo:** provar a operação ponta a ponta e torná-la sustentável.

Entregas:

- reconciliação da projeção de workspaces com Accounts e de wallets, saldos,
  blocos, lotes, planos e cobranças dentro do Subscription;
- métricas e alertas para atraso, lacuna, conflito, dead-letter, backlog de
  inbox/outbox, provisionamento parcial e falha de webhook;
- testes de contrato entre serviços, compatibilidade de schema, replay,
  duplicidade, ordenação, indisponibilidade de Accounts e rotação de credencial;
- testes de carga, deadlock, recuperação pós-falha e disaster recovery;
- runbooks de replay/reconciliação, cobrança incerta, reparo auditado e resposta
  a comprometimento de credenciais.

**Critério de saída:** uma restauração seguida de replay converge sem duplicar
efeitos; atrasos e divergências são detectados e reparados sem editar histórico;
o serviço mantém isolamento por workspace durante falhas de integração.

## Ordem recomendada de liberação

1. Fases 0 e 1: contratos, segurança e entrega confiável com Accounts.
2. Fases 2 a 4: catálogo, provisionamento e razão de créditos.
3. Fases 5 e 6: planos gratuitos, franquias e consumo estrito.
4. Fases 7 e 8: Billing agnóstico e Stripe como primeiro conector.
5. Fase 9: promoções e ajustes administrativos.
6. Fase 10: escala operacional e preparação para produção.

Nenhuma liberação com mutações de customer antecede a autenticação, a projeção e
o provisionamento materializado do workspace. Essa ordem valida primeiro a
consistência entre identidade externa, uso, saldo e extrato; dinheiro e
fornecedores externos entram somente depois dessas garantias.
