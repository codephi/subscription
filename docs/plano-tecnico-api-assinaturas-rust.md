# Plano técnico da API de assinaturas em Rust

**Status:** planejamento arquitetural, sem implementação

**Data:** 25 de agosto de 2026

**Revisão:** `customer_wallet` em créditos, `item_wallet` em item units, Price `unit`/`tiered`, uso pendente e ciclos declarativos

**Objetivo:** definir o domínio, os contratos HTTP, as garantias transacionais e a sequência de entrega de uma API de créditos, assinaturas e consumo pós-pago por workspace.

## 1. Resumo executivo

A API mantém dois tipos de wallet/subledger. A `customer_wallet` é a carteira principal da conta/customer, guarda saldo fungível somente em `credit_units` e possui extrato completo. A `item_wallet`, única por customer + item, contabiliza item units consumidas, pendentes e convertidas, também com extrato próprio. Nenhuma delas armazena moeda ou valor monetário.

O consumo é sempre pós-pago. Uma chamada válida primeiro registra as item units e atualiza o medidor/extrato da `item_wallet`. Cada Price define a conversão por bloco entre item units e wallet credits, por exemplo `1 -> 1`, `10 -> 1` ou `10 -> 10`. Quando o acumulado completa blocos, a mesma transação emite o débito correspondente na `customer_wallet`; o resto permanece pendente na `item_wallet`. Na V1 `CREDIT_STRICT`, o débito só é aceito se mantiver o saldo maior ou igual a zero; insuficiência rejeita integralmente a chamada, sem registrar consumo, pendente, bloco ou lançamento. O modelo de saldo negativo e recibo `402` fica reservado à capacidade futura `CREDIT_FLEXIBLE`.

Produto é o agrupador de regras e elegibilidade e não contém preço. Product `CREDIT_METERED` possui Items com Prices versionados `unit` ou `tiered`, que convertem item units em débitos de wallet credits. Product `ENTITLEMENT_ONLY` possui Items/subitens de acesso, sem Price ou item wallet. A `customer_wallet` é única e seu saldo é a projeção das concessões/lotes de crédito ainda disponíveis; ela recebe créditos-base de ciclos de assinatura, concessão direta, Vale/Cupom ou Compensation. A assinatura é a fonte de entitlement e de sua franquia-base por ciclo.

O catálogo de `SubscriptionPlanVersion` de cada `Subscription` comercial declara a única oferta que pode criar, iniciar ou manter um `CustomerPlan` principal: preço, recorrência, admissão, meios aceitos e créditos-base por ciclo. Cobrança e confirmação pertencem ao Billing externo; a confirmação do ciclo efetiva atomicamente seu entitlement e, quando `granted_credit_units > 0`, sua concessão-base na Wallet. Plan nunca seleciona Voucher ou Cupom. `OnDemandPlan` é um catálogo separado de adicional de crédito dentro da mesma Subscription comercial e não é uma segunda assinatura nem um tipo de `SubscriptionPlanVersion`. A conversão do Price de Item é somente de item units consumidas para wallet credits debitados; não é câmbio nem conversão de dinheiro.

## 2. Decisões normativas da v1

1. Há exatamente uma `customer_wallet` por conta/customer e exatamente uma `item_wallet` por `(customer_id, item_id)` faturável de Product `CREDIT_METERED`. Items de Product `ENTITLEMENT_ONLY` não geram `item_wallet`. Na v1, a conta/customer de cobrança é o workspace já presente no contrato; não se introduz uma nova hierarquia de clientes.
2. A `customer_wallet` possui `balance_credit_units` inteiro com sinal. Wallet credit é unidade interna e fictícia, não moeda, token negociável ou saldo bancário.
   A `item_wallet` possui item units e contadores de consumo, não `balance_credit_units`. Item units de itens diferentes não são fungíveis nem transferíveis.
3. Créditos e quantidades de 64 bits são strings inteiras decimais no JSON para não perder precisão em clientes JavaScript. Exemplo: `"credit_units": "1250"`. Não se usa ponto flutuante.
4. O razão é imutável. Correções são novas Compensations executadas, nunca edição ou exclusão de lançamentos anteriores.
5. O saldo da `customer_wallet` é projeção atualizada atomicamente com seu razão. Toda alteração produz lançamento consultável; não existe atualização direta de `balance_credit_units`.
6. Cada chamada válida de consumo produz exatamente uma entrada na `item_wallet`, independentemente do saldo de créditos. Ela só produz débito na `customer_wallet` quando, somada ao pendente do item, completa pelo menos um bloco.
7. Um Price no modelo `unit` define `unit_block_size` item units e `credit_units` wallet credits por bloco, ambos inteiros positivos. Assim, `1 -> 1`, `10 -> 1` e `10 -> 10` são configurações válidas. No modelo `tiered`, cada faixa define sua própria razão `unit_block_size -> credit_units`, permitindo mudar tanto o tamanho do bloco quanto os créditos debitados conforme o acumulado avança.
8. Para cada `item_wallet` vale permanentemente `total_received_item_units = total_converted_item_units + pending_item_units`. O acumulador satisfaz `0 <= pending_item_units < pending_unit_block_size` e nenhuma item unit pode ser descartada, reaproveitada ou convertida duas vezes.
9. Na V1, se uma chamada completar blocos, o débito na `customer_wallet` só é registrado se o saldo resultante for zero ou positivo; caso contrário, a chamada é rejeitada integralmente sem efeito. Uma chamada que apenas aumenta o pendente produz somente `ItemWalletEntry`, sem transação na `customer_wallet`, e retorna `201`. O débito aceito independentemente do saldo e o `402` pertencem exclusivamente à futura capacidade `CREDIT_FLEXIBLE`.
10. Um Price tem `pricing_model = unit` ou `tiered`. Em `unit`, todo bloco converte para a quantidade fixa de wallet credits. Em `tiered`, faixas progressivas graduadas são aplicadas às item units convertidas acumuladas por customer + item + versão de Price + ciclo. `accumulation_cycle` ausente significa que o acumulado nunca reinicia; presente, contém uma regra recorrente declarativa escolhida pelo cliente. Cada bloco posterior usa a faixa correspondente sem reprecificar débitos já lançados.
11. A consulta de elegibilidade sempre exige entitlement efetivo. Para `CREDIT_METERED`, também exige `balance_credit_units >= 0`; para `ENTITLEMENT_ONLY`, não consulta saldo. Ela não reserva créditos e nunca autoriza sozinha consumo ou débito. A `item_wallet` não participa do saldo elegível.
12. Toda alteração externa de créditos e toda chamada de consumo recebem `transaction_id`. Para recorrência, a chave natural adicional é assinatura + período. Para cupom no modo por usuário, `unique_user_id` é uma chave de limite distinta do `transaction_id`.
13. Um `transaction_id` é único dentro do workspace/customer, em todos os tipos de operação. Qualquer reutilização retorna `409 transaction_already_exists`, independentemente do payload, e nunca cria novo efeito. Toda criação externa transacional também exige `Idempotency-Key`; qualquer reutilização dessa chave retorna `409 idempotency_key_already_used`, sem replay da resposta original.
14. Datas e períodos são armazenados em UTC. Agendamentos mensais usam calendário, e não uma duração fixa de dias.
15. Wallets são materializadas no provisionamento. Criar customer materializa sua `customer_wallet` e uma `item_wallet` somente para cada Item faturável aplicável de Product `CREDIT_METERED`; consumo não cria wallets de forma preguiçosa.
16. Product possui `usage_model` imutável após publicação: `CREDIT_METERED` ou `ENTITLEMENT_ONLY`. O primeiro usa Price, item wallet e saldo; o segundo concede apenas acesso por assinatura e proíbe Price, item wallet e lançamento de consumo.
17. CustomerPlan é a fonte de verdade de entitlement, calendário e franquia-base por ciclo; `Subscription` comercial é a raiz de catálogo. Billing é adaptador de cobrança assíncrona; resposta síncrona e webhook convergem para a mesma tentativa, e pagamento `CONFIRMED` efetiva o entitlement e a concessão-base daquele ciclo uma única vez.
18. A arquitetura modela `CREDIT_STRICT`, `CREDIT_FLEXIBLE` e `ENTITLEMENT_ONLY`, mas a V1 expõe, publica e comercializa exclusivamente `CREDIT_STRICT`; os demais perfis são capacidade futura e não podem ser ativados. `CREDIT_STRICT` fixa compatibilidade de Products, concessão de créditos e efeito da cobrança; combinações fora da matriz são rejeitadas na publicação.
19. O único meio de pagamento da V1 é `CARD` por provedor externo. O sistema guarda apenas IDs/tokens opacos e referências do provedor; nunca PAN, CVV ou qualquer dado sensível do cartão.
20. A primeira contratação cria a assinatura em estado comercial `ACTIVE`; quando o plano for pago, o entitlement só se torna efetivo após a confirmação definitiva do pagamento, inclusive depois de autenticação adicional exigida pelo emissor. Plano gratuito não cria cobrança. Uma renovação de plano pago usa o cartão salvo e realiza exatamente uma tentativa comercial na data de renovação: não há antecipação nem repetição automática dentro do ciclo.
21. A V1 é webhook-first: Billing processa somente eventos assinados do provedor, deduplicados e correlacionados à `CollectionRequest`/PaymentIntent persistida. Ausência, atraso, timeout ou resposta incerta não prova falha, não autoriza nova cobrança nem cancelamento; o estado conhecido é preservado e evento válido tardio ainda passa pela máquina de estados idempotente. Consulta automática ao provedor não existe na V1.
22. `SubscriptionPlanVersion` define a oferta comercial completa, incluindo `granted_credit_units` não negativo como franquia-base por ciclo; ele não possui nem seleciona Voucher ou Cupom. Voucher, Cupom e Compensation são agregados independentes e não são emitidos automaticamente por aderir a um plano.
23. A admissão da assinatura é decidida por uma política de elegibilidade independente do plano e do preço. Ela pode permitir adesão livre ou exigir evidências dinâmicas, como e-mail/identidade verificada ou referência antifraude de cartão validado pelo provedor. Essa política bloqueia ou permite criar a assinatura; uma campanha, separadamente, pode avaliar o contexto de assinatura e emitir um Voucher. Nenhuma regra de uso único é obrigatória sem aprovação explícita.
24. Toda concessão positiva materializa um lote de crédito auditável com origem, saldo residual e validade quando aplicável. A franquia-base de assinatura expira no fim do ciclo materializado enquanto continuar classificada como assinatura; crédito de `OnDemandPlan`, inclusive residual reclassificado em troca de plano, acumula e não expira pela renovação. O consumo `CREDIT_STRICT` aloca primeiro o lote de assinatura que expira no ciclo atual e depois créditos persistentes, sem aceitar débito quando a soma disponível não cobrir o valor.
25. Para cada par `customer_id` + `Subscription` comercial pode existir no máximo um `CustomerPlan` principal em condição comercial ativa. `CustomerPlan` chega à Subscription comercial exclusivamente por `CustomerPlan → SubscriptionPlanVersion → Subscription`; Products/Subscriptions comerciais distintos podem ter CustomerPlans ativos distintos. Troca de plano atua nesse vínculo único e `OnDemandPlan` nunca cria CustomerPlan.

## 3. Limites do escopo

Incluído na v1:

- `customer_wallet` e razão de wallet credits por conta/workspace;
- `item_wallet`, extrato e medidor de item units por customer + item faturável;
- catálogo de produtos, itens e versões de preço;
- Products medidos por crédito; Products somente por entitlement permanecem modelados para evolução, mas não são publicados na V1;
- elegibilidade de produto combinando plano, estado de entitlement e, quando aplicável, saldo;
- registro e acumulação de consumo pós-pago por customer + item;
- débitos por blocos completos e consulta de item units pendentes/convertidas;
- crédito direto;
- ofertas `CREDIT_STRICT`, Plans versionados com preço, recorrência e créditos-base por ciclo, subscriptions gratuitas ou pagas com adesão do customer e política de admissão extensível;
- Vouchers, Cupons e Compensations independentes da franquia-base do plano;
- fronteira de Billing para cartão tokenizado por provedor externo, uma tentativa comercial por renovação, webhooks e conciliação assíncrona;
- vales e cupons;
- Compensations administrativas;
- histórico, auditoria, idempotência e reconciliação.

Fora da v1:

- armazenamento ou processamento de dados brutos de cartão; meios ficam tokenizados no provedor;
- Pix, boleto, wallet de provedor e qualquer meio que não seja cartão tokenizado por provedor externo;
- `CREDIT_FLEXIBLE`, `ENTITLEMENT_ONLY`, saldo negativo e consumo com recibo `402`; a arquitetura os preserva apenas como capacidade futura não ativável;
- produto de estorno: a V1 não oferece tela, endpoint, elegibilidade, política comercial nem automação de refund; cancelamento de assinatura não gera estorno automático;
- fórmula dinâmica ou taxa de câmbio entre dinheiro e `credit_units`; o plano apenas persiste preço comercial e concessão de créditos como termos independentes;
- estorno que apaga uma operação original;
- reserva prévia de saldo;
- preço no produto;
- calendários baseados em fuso local; todas as fronteiras do ciclo são instantes UTC;
- cadastro ou interpretação de usuários finais do cliente;
- transferência de saldo entre workspaces;
- transferência de item units entre `item_wallets` ou conversão entre unidades de itens diferentes;
- saldo de wallet credits dentro da `item_wallet`;
- expiração geral/indeterminada de dívida ou saldo; a única expiração de crédito fechada na V1 é a baixa auditável do lote de franquia-base de assinatura no fim de seu ciclo.

## 4. Modelo de domínio

### 4.1 WorkspaceBillingConfig

Configuração 1:1 com o workspace.

Campos principais:

- `workspace_id`;
- `direct_credit_enabled`;
- `recurring_credit_enabled`;
- `created_at`, `updated_at`;
- versão de concorrência otimista para alterações administrativas.

Invariantes:

- os dois modos podem estar habilitados simultaneamente;
- desabilitar recorrência impede criar/reativar assinaturas, mas não apaga assinaturas nem o histórico;
- Compensations administrativas e resgates promocionais não dependem dessas duas flags;
- qualquer chamada que informe um workspace existente pode alterar a configuração.

### 4.2 Wallet comum e especializações

A tabela/modelo base `Wallet` materializa a hierarquia e a identidade comum:

- `wallet_id`;
- `customer_id`, igual ao principal de cobrança do workspace na v1;
- `wallet_type`, enum `CUSTOMER` ou `ITEM`;
- `parent_customer_wallet_id`, nulo em `CUSTOMER` e obrigatório em `ITEM`;
- `item_id`, nulo em `CUSTOMER` e obrigatório em `ITEM`;
- `created_at` e `provisioning_scope_version` de origem.

A linha de `Wallet` é permanente e integralmente imutável depois do `INSERT`: não aceita `UPDATE`, `DELETE`, soft delete, anonimização destrutiva nem troca de vínculo. O estado operacional não é uma coluna mutável nessa tabela. Ele é obtido do último `WalletLifecycleEvent`, razão append-only com estados `PROVISIONING`, `ACTIVE`, `DISABLED` ou `ERROR`, contendo `wallet_id`, sequência monotônica, estado anterior/novo, motivo, ator e timestamp. Uma correção de estado acrescenta novo evento; nunca altera ou apaga o anterior.

Chaves e restrições:

- `wallet_id` é chave primária global;
- índice único parcial em `customer_id` para `wallet_type = CUSTOMER` garante uma `customer_wallet` por customer;
- índice único parcial em `(customer_id, item_id)` para `wallet_type = ITEM` garante uma microcarteira por customer + item;
- check de tipo exige `CUSTOMER => parent_customer_wallet_id IS NULL AND item_id IS NULL`;
- check de tipo exige `ITEM => parent_customer_wallet_id IS NOT NULL AND item_id IS NOT NULL`;
- `parent_customer_wallet_id` referencia a Wallet `CUSTOMER` do mesmo `customer_id`;
- `item_id` referencia um Item faturável de Product `CREDIT_METERED` aplicável ao cluster/escopo do customer no instante do provisionamento;
- nenhuma coluna de `Wallet` muda depois da criação; restrições do banco devem impedir `UPDATE` e `DELETE`;
- `WalletLifecycleEvent` é único por `(wallet_id, sequence)`, só aceita inserção e não pode ser editado ou apagado;
- o estado efetivo pode ser materializado em uma projeção reconstruível, mas essa projeção não substitui nem modifica a Wallet ou seu histórico.

Exemplo: se o customer pertence a um cluster com 10 Items faturáveis aplicáveis e outros Items `ENTITLEMENT_ONLY`, o conjunto materializado contém 11 linhas em `Wallet`: uma `CUSTOMER` global e 10 `ITEM`, todas apontando para a mesma customer wallet como pai. Items sem medição não acrescentam Wallet.

#### Especialização CustomerWallet

Tabela/projeção 1:1 com `Wallet` de tipo `CUSTOMER`:

- `wallet_id`, PK e FK para `Wallet`;
- `balance_credit_units`, inteiro com sinal;
- `version`, incrementada a cada movimento;
- `updated_at`.

Invariantes:

- uma única especialização para cada Wallet `CUSTOMER`;
- não existe campo de moeda ou valor monetário;
- o tipo numérico suporta sinal para evolução, mas a V1 impede qualquer débito que deixaria saldo negativo;
- `balance_credit_units` após cada operação deve coincidir com `balance_after_credit_units` da última `CustomerWalletEntry`;
- a linha nasce materializada com saldo inicial zero durante o provisionamento do customer, nunca durante consumo ou crédito.

Somente a `customer_wallet` responde elegibilidade por saldo. A existência, pendência ou volume de uma `item_wallet` não substitui nem fragmenta esse saldo.

### 4.3 CustomerWalletEntry

Registro contábil imutável de créditos e unidade do extrato transacional completo da wallet.

Tipos:

- `DEBIT`;
- `DIRECT_CREDIT`;
- `SUBSCRIPTION_CREDIT`;
- `CREDIT_EXPIRY_FORFEITURE`;
- `VOUCHER_CREDIT`;
- `COMPENSATION`.

Campos principais:

- `customer_wallet_entry_id`;
- `customer_id`;
- `type`;
- `signed_credit_units`: inteiro com sinal; positivo adiciona saldo e negativo remove; zero é rejeitado;
- `balance_before_credit_units`, `balance_after_credit_units`;
- `transaction_id`, quando a origem for uma chamada externa;
- `description` opcional, texto humano imutável com tamanho limitado;
- `metadata` opcional, objeto JSON imutável com limites de tamanho, profundidade, quantidade de chaves e valores escalares permitidos;
- `source_channel`, distinguindo consumo por produto/item, crédito-base de ciclo de assinatura, crédito direto, vale direto, cupom, promoção baseada em vale e Compensation administrativa;
- instantâneo do cálculo de créditos;
- `created_at`, `request_id` e referência opcional de origem informada pelo chamador.

Invariantes:

- `balance_after_credit_units = balance_before_credit_units + signed_credit_units`;
- uma operação de domínio produz no máximo uma entrada no razão;
- toda mudança de saldo produz exatamente uma entrada no razão;
- toda operação que efetivamente movimenta créditos produz exatamente uma entrada; um ciclo de assinatura com `granted_credit_units > 0` produz exatamente uma entrada `SUBSCRIPTION_CREDIT`;
- lançamento zero é rejeitado para toda origem; ciclo com `granted_credit_units = 0` efetiva somente entitlement e não cria entrada;
- lançamentos não são alterados ou removidos;
- `description` e `metadata` são somente contexto auditável: não alteram valor, tipo, idempotência, Price, entitlement ou referências tipadas;
- `metadata` aceita chaves livres do cliente, é preservada na forma canônica e devolvida integralmente nas consultas da transação; não pode ser editada depois do commit;
- metadata não cria uma transação genérica/manual: a operação continua obrigada a ter uma origem de domínio válida, como consumo, crédito direto, pagamento de Plan, Vale ou Compensation;
- o registro de deduplicação preserva o hash de `description` e da forma canônica de `metadata` para auditoria, mas qualquer reutilização da chave retorna conflito, com payload igual ou diferente.

#### Lotes de crédito, alocação e expiração

Cada entrada positiva que disponibiliza créditos materializa um `CreditLot` imutável na origem e com estado materializado auditável: `credit_lot_id`, entrada concessora, `source_kind`, quantidade original, saldo residual, `expires_at` opcional, `customer_plan_cycle_id` quando a origem é a franquia-base e histórico append-only de consumo, reclassificação e expiração. O saldo único da `customer_wallet` continua sendo a projeção da soma dos lotes disponíveis, conciliada com o razão; o lote não substitui nem altera a `CustomerWalletEntry` original.

`SUBSCRIPTION_CREDIT` cria lote de assinatura vinculado ao ciclo materializado. Seu residual expira integralmente em `current_period_end`: não acumula na renovação, que cria somente a nova franquia do ciclo seguinte. `DIRECT_CREDIT` originado por `OnDemandPlan` cria lote de compra extra persistente: permanece disponível através de renovações e trocas de plano, sem política geral de expiração própria na V1. Outras origens declaram sua própria validade quando aplicável; ausência de validade não permite inferir expiração automática.

Em toda troca de plano, antes do reset do ciclo, `CreditLotReclassification` converte idempotente e auditavelmente o residual disponível de cada lote `SUBSCRIPTION_CREDIT` para `ON_DEMAND` persistente. O evento guarda lote, quantidade preservada, classificação/validade anterior e nova, `customer_plan_cycle_id`, `plan_transition_id`, ator e instante; é único por lote + transição. Ele preserva a mesma quantidade de `credit_units`, zera sua expiração de assinatura e conserva a proveniência do crédito original. Não cria nem edita `CustomerWalletEntry`, não altera `balance_credit_units` e não é preço, câmbio, desconto, conversão monetária ou prorrata. Depois disso, o residual convertido participa da segunda prioridade de consumo, junto com os créditos sob demanda persistentes.

No consumo `CREDIT_STRICT`, o débito bloqueia a Wallet e os lotes elegíveis, verifica que a soma de seus residuais cobre todo o valor e grava `CreditLotAllocation` imutável para cada lote utilizado. A ordem normativa é: (1) créditos de assinatura disponíveis que expiram no ciclo atual, depois (2) créditos persistentes, inclusive `OnDemandPlan`; os critérios de desempate dentro da mesma classe são estáveis e auditáveis. Se a soma não cobrir o débito, a chamada é rejeitada integralmente com `409 insufficient_credit`. Assim, o extrato e as alocações revelam exatamente quais concessões suportaram cada consumo.

Ao fim do ciclo, o residual de lote de assinatura é baixado por uma `CREDIT_EXPIRY_FORFEITURE` negativa, idempotente e referenciada ao lote/ciclo, e o estado materializado passa a expirado. Expiração jamais edita ou apaga o crédito/debito original, saldo ou alocações. A baixa e a atualização da projeção ocorrem atomicamente, ou por saga/outbox idempotente equivalente, preservando a reconciliação do razão com os lotes.

`WalletTransactionReference` é a tabela filha 1:N que mantém relações oficiais sem inflar `CustomerWalletEntry` com dezenas de colunas nulas:

- `wallet_transaction_reference_id`, `customer_wallet_entry_id` e `reference_kind`;
- exatamente um alvo tipado por linha, como `subscription_id` (Subscription comercial), `customer_plan_id`, `customer_plan_cycle_id`, `plan_version_id`, `credit_lot_id`, `voucher_id`, `coupon_id`, `product_id`, `item_id`, `usage_event_id`, `item_wallet_entry_id`, `debit_id`, `collection_request_id`, `billing_payment_id` ou `external_reference`;
- check exige exatamente um alvo compatível com `reference_kind` e FKs são aplicadas a todos os recursos internos;
- unicidade em `(customer_wallet_entry_id, reference_kind, target)` impede duplicação da mesma relação;
- índices por `(reference_kind, target, customer_wallet_entry_id)` permitem localizar o extrato a partir de plano, Vale, Product, pagamento ou outra origem;
- referências são inseridas no mesmo commit da entrada, são imutáveis e não aceitam exclusão;
- `metadata` nunca substitui `WalletTransactionReference`; IDs canônicos enviados dentro de metadata continuam apenas texto opaco e não criam relacionamento.

Referências esperadas por origem:

- consumo: `item_wallet_id`, `item_wallet_entry_id`, `usage_event_id`, `debit_id`, `product_id`, `item_id`, IDs dos blocos convertidos, versões de Price, item units recebidas/convertidas/pendentes, `CreditLotAllocation` e `transaction_id`;
- crédito-base de assinatura: `customer_plan_id`, `customer_plan_cycle_id`, `plan_version_id`, `credit_lot_id`, `collection_request_id`/`billing_payment_id` quando pago e instantâneo de `granted_credit_units`;
- crédito direto: `direct_credit_id`, referência externa opcional e `transaction_id`;
- vale direto: `voucher_id` e `transaction_id`;
- cupom: `coupon_id`, `voucher_id`, `coupon_user_redemption_id` quando aplicável e `transaction_id`;
- promoção: sempre referencia o `voucher_id` que materializou o benefício e, quando existir, campanha/cupom correlato; não há crédito promocional destrutivo ou sem vale;
- Compensation: `compensation_id`, tipo, motivo, solicitante, aprovador, referências correlatas e `transaction_id` de execução.

O extrato da `customer_wallet` não depende de reconstruir relações mutáveis para explicar um lançamento antigo. Nome do produto/item, instantâneo do Price, conversão aplicada e demais dados que precisem permanecer legíveis são copiados para a entrada, além das referências à `item_wallet` e aos recursos originais.

### 4.4 Product

Agrupa itens e regras de elegibilidade.

Campos principais:

- `product_id`, `name`, `description`;
- `usage_model`, enum `CREDIT_METERED` ou `ENTITLEMENT_ONLY`, imutável após publicação;
- estado `ACTIVE`, `INACTIVE` ou `ARCHIVED`;
- regras explícitas e versionadas;
- `created_at`, `updated_at`.

`CREDIT_METERED` possui Items faturáveis com Price, provisiona item wallets e converte item units em wallet credits. Sua elegibilidade exige entitlement efetivo e `balance_credit_units >= 0`. `ENTITLEMENT_ONLY` pode possuir Items e subitens como catálogo de funcionalidades, mas proíbe Price, item wallet, UsageEvent e qualquer sensibilização de saldo; sua elegibilidade depende somente do entitlement efetivo.

Na v1 não haverá uma linguagem genérica de regras. Ausência de entitlement, assinatura comercial inadimplente e insuficiência de créditos são fatos distintos na resposta. Crédito direto ou Vale pode aumentar saldo, mas não concede por si só entitlement a Products. Alterar `usage_model` exige nova versão ou novo Product, pois muda contratos, provisionamento e auditoria.

### 4.5 Item e PriceVersion

`Item` pertence a exatamente um Product e possui:

- `item_id`, `product_id`, `parent_item_id` opcional para subitens;
- `name`, `unit_name`;
- `quantity_scale`: obrigatório apenas para Items faturáveis e define a unidade atômica de consumo;
- estado `ACTIVE`, `INACTIVE` ou `ARCHIVED`;
- uma versão de preço ativa por instante somente quando o Product é `CREDIT_METERED`.

Em Product `CREDIT_METERED`, as item units são específicas do Item e descritas por `unit_name`/`quantity_scale`. O Price não cria uma unidade fungível: ele declara apenas quantos wallet credits a `customer_wallet` debita quando a `item_wallet` completa um bloco. Em Product `ENTITLEMENT_ONLY`, Items/subitens são apenas escopos de acesso; `unit_name`, `quantity_scale` e Price são ausentes.

`PriceVersion` é imutável após publicação:

- `price_version_id`, `item_id`;
- `pricing_model`, enum `unit` ou `tiered`;
- `unit_block_size`, inteiro positivo em átomos da unidade do item, obrigatório apenas no modelo `unit`;
- `credit_units`, inteiro positivo de wallet credits debitados por bloco, obrigatório apenas no modelo `unit`;
- `effective_from`, `effective_until` opcional;
- `accumulation_cycle`, opcional e permitido apenas em `tiered`;
- lista ordenada de faixas no modelo `tiered`;
- estado `DRAFT`, `SCHEDULED`, `ACTIVE` ou `RETIRED`.

No modelo `unit`, cada bloco de `unit_block_size` item units emite débito de exatamente `credit_units` wallet credits. No modelo `tiered`, os campos de conversão no nível raiz são proibidos e cada faixa possui `from_accumulated_units` inclusivo, `to_accumulated_units` exclusivo ou aberto, `unit_block_size` e `credit_units`. As faixas começam em zero, são contíguas, não se sobrepõem e a última cobre o restante. Cada faixa finita deve ter amplitude divisível por seu próprio `unit_block_size`, para que a passagem à faixa seguinte nunca abandone uma fração. A razão de conversão pode ficar mais cara ou mais barata, alterando o tamanho do bloco, os créditos por bloco ou ambos; nenhum valor pode ser zero ou negativo. O cálculo usa divisão inteira, resto, multiplicação e soma verificadas; overflow retorna `422` antes de qualquer gravação.

Exemplo simples: `unit_block_size = 1.000` e `credit_units = 100` significa débito de 100 créditos por 1.000 requests. Se o pendente era zero e chegam 1.012 requests, um bloco debita 100 créditos e 12 requests permanecem pendentes. O caso 1:1 usa bloco 1.

Exemplo graduado com razões diferentes: na faixa `0..10`, cada bloco é `1 item unit -> 1 wallet credit`; na faixa `10..20`, cada bloco é `2 item units -> 1 wallet credit`; a faixa `20..∞` pode definir outra razão. Uma chamada que atravessa o limite é particionada: cada bloco completo usa a razão da faixa correspondente, sem reprecificar o trecho anterior. Os limites finitos do exemplo são divisíveis pelo bloco de suas respectivas faixas. O acumulador inclui versão e ciclo, portanto dividir artificialmente o mesmo uso em várias chamadas dentro do ciclo não muda o total.

Quando presente, `accumulation_cycle` possui:

- `anchor_at`: primeira fronteira, como instante RFC 3339 em UTC;
- `recurrence_rule`: regra declarativa recorrente no formato iCalendar RRULE da [RFC 5545](https://www.rfc-editor.org/rfc/rfc5545.html), sem `COUNT` ou `UNTIL`, que produz uma sequência infinita e estritamente crescente de fronteiras.

Não há lista fechada de ciclos na API. Presets como semanal ou mensal pertencem ao front e apenas geram uma configuração declarativa. A API valida que a regra é determinística, não ambígua, não gera duas fronteiras iguais e que `anchor_at` corresponde à primeira ocorrência da RRULE. `accumulation_cycle` ausente usa uma única chave vitalícia sem término. Quando presente, as ocorrências da regra formam intervalos semiabertos `[cycle_start, cycle_end)` em UTC; um bloco completado exatamente em `cycle_end` pertence ao ciclo seguinte. Regras de calendário, inclusive meses de tamanhos diferentes, seguem a semântica da RRULE calculada a partir da âncora original, sem deslocar silenciosamente a âncora futura.

O ciclo de precificação é determinado por `accepted_at`, horário confiável do servidor no evento que completa o bloco. Um `occurred_at` informado pelo cliente pode ser preservado para auditoria, mas não altera retroativamente ciclo ou faixa. Isso mantém os lançamentos definitivos e impede que eventos atrasados reordenem blocos já cobrados.

Uma versão publicada nunca muda. Se uma nova versão entra em vigor enquanto existe uma fração pendente, essa fração preserva `pending_price_version_id`, faixa, tamanho do bloco e conversão antiga até completar exatamente esse bloco. Na chamada que o completa, o bloco usa os wallet credits da versão antiga; a quantidade restante passa a usar a versão ativa nova. Dentro de um Price `tiered`, o pendente também preserva a faixa atual; ao completá-lo, o restante da chamada avança para a próxima faixa e sua nova razão. Uso já recebido não é reprecificado. O débito guarda `price_version_id`, blocos, faixas, razões aplicadas e `debited_credit_units`. `expected_price_version_id` divergente retorna `409 price_version_changed` sem registrar consumo.

### 4.6 ItemWallet, ItemWalletEntry, PricingAccumulator e BillingBlock

`ItemWallet` é a especialização 1:1 de uma `Wallet` tipo `ITEM` e funciona como medidor e subledger de consumo:

- `wallet_id`, PK/FK e também `item_wallet_id` público;
- `customer_id`, `parent_customer_wallet_id` e `item_id`, herdados/validados pela Wallet base;
- `total_received_item_units`, soma de todo consumo aceito;
- `total_converted_item_units`, soma das item units já convertidas em blocos;
- `total_converted_blocks`, quantidade de blocos lógicos completos em todas as versões;
- `pending_item_units`;
- `pending_price_version_id`, `pending_tier_id`, `pending_unit_block_size` e `pending_credit_units`, quando `pending_item_units > 0`;
- `version`, incrementada em toda chamada inédita de consumo;
- `updated_at` e referência ao último evento de consumo.

`ItemWalletEntry` é imutável e existe para toda chamada de consumo aceita:

- `item_wallet_entry_id`, `item_wallet_id`, `usage_event_id`, `transaction_id`;
- `received_item_units` da chamada;
- `total_received_item_units_before` e `total_received_item_units_after`;
- `converted_item_units`, `converted_blocks` e `pending_item_units_after`;
- `emitted_debited_credit_units`, soma dos blocos emitidos, sem constituir saldo da item wallet;
- intervalo cumulativo de item units `[unit_offset_start, unit_offset_end)`;
- IDs dos `BillingBlock` formados;
- `debit_id` e `customer_wallet_entry_id` opcionais, ambos ausentes se nenhum bloco foi completado; quando há conversão, apontam ao débito e lançamento correspondentes;
- `accepted_at` e metadados imutáveis da origem.

`PricingAccumulator` é único por `(customer_id, item_id, price_version_id, cycle_key)` e contém `accumulated_converted_item_units` e `converted_blocks`. Sem `accumulation_cycle`, `cycle_key` é um sentinela vitalício estável; com regra declarada, é `cycle_start`. Ele determina a faixa de conversão de cada próximo bloco e só avança quando `BillingBlock`, débito e ambos os extratos são confirmados. Uma nova fronteira cria outro acumulador com zero; não zera nem altera a linha do ciclo anterior.

O modelo `unit` também mantém um `PricingAccumulator` vitalício para consulta e reconciliação, embora seu custo em créditos não dependa dele. No modelo `tiered`, essa entidade é parte do cálculo de créditos e deve ser bloqueada antes de determinar as faixas.

`BillingBlock` representa cada bloco lógico completo, mesmo quando vários blocos são consolidados no mesmo débito:

- `billing_block_id`, `customer_id`, `item_wallet_id`, `item_id`;
- `global_block_sequence` monotônica dentro de customer + item;
- `price_version_id` e `price_block_ordinal` usado nas faixas;
- `cycle_key`, `cycle_start` e `cycle_end`, sendo início/fim nulos apenas no caso vitalício;
- instantâneo de `anchor_at` e `recurrence_rule` quando houver ciclo;
- `accumulated_units_before` e `accumulated_units_after` dentro do ciclo;
- `unit_block_size` item units, `debited_credit_units` wallet credits e faixa aplicada;
- intervalo cumulativo de unidades `[unit_offset_start, unit_offset_end)`;
- `usage_event_id` que completou o bloco, `item_wallet_entry_id`, `debit_id` e `customer_wallet_entry_id`.

Cada `UsageEvent`/`ItemWalletEntry` guarda seu intervalo cumulativo `[unit_offset_start, unit_offset_end)`. A interseção permite provar quais chamadas contribuíram para cada bloco sem editar eventos anteriores. Blocos completos particionam o prefixo `0..total_converted_item_units`; o pendente é exatamente o sufixo `total_converted_item_units..total_received_item_units`.

Invariantes:

- `total_received_item_units = total_converted_item_units + pending_item_units`;
- `pending_item_units` nunca é negativo e é sempre menor que `pending_unit_block_size`;
- `pending_item_units = 0` implica referências pendentes nulas;
- `total_converted_item_units` cresce pela soma exata de `BillingBlock.unit_block_size`; não se presume bloco uniforme entre faixas;
- intervalos de `BillingBlock` são contíguos, não se sobrepõem e têm comprimento igual ao tamanho do bloco persistido;
- `(customer_id, item_id, global_block_sequence)` e `(customer_id, item_id, price_version_id, cycle_key, price_block_ordinal)` são únicos;
- em cada `PricingAccumulator`, `accumulated_converted_item_units` é a soma dos tamanhos dos blocos associados àquela chave e nunca diminui;
- a passagem de faixa afeta somente blocos cujo intervalo acumulado começa no novo limite; blocos anteriores preservam preço e faixa;
- mudança de ciclo cria uma nova progressão a partir da primeira faixa sem apagar o acumulador anterior;
- a mesma item unit recebida pertence a exatamente uma `ItemWalletEntry` e termina em exatamente um `BillingBlock` convertido ou no único pendente atual;
- todo `BillingBlock` referencia exatamente um débito e uma `CustomerWalletEntry`; a soma de `debited_credit_units` dos blocos é igual ao módulo de `signed_credit_units` dessa entrada;
- preço novo não altera uma fração pendente iniciada sob preço antigo;
- desativar produto/item não debita créditos nem descarta o pendente; a mudança acrescenta um `WalletLifecycleEvent` e nunca edita, arquiva por soft delete ou exclui a Wallet.

#### Provisionamento materializado

`WalletProvisioning` registra `customer_id`, `scope_version`, `status`, `expected_item_wallets`, `materialized_item_wallets`, timestamps e erro final. Ele é a fonte operacional de prontidão; não altera saldo nem aparece no extrato.

O provisionamento mantém o conjunto real de wallets igual ao conjunto esperado pelo cluster/escopo:

1. Ao criar um customer, resolver a versão estável do escopo e seus Items aplicáveis.
2. Criar a Wallet `CUSTOMER` e sua especialização `CustomerWallet` com saldo zero.
3. Para cada Item faturável aplicável de Product `CREDIT_METERED`, criar Wallet `ITEM`, vinculá-la à customer wallet e criar a especialização `ItemWallet` com contadores zero; ignorar Items `ENTITLEMENT_ONLY`.
4. Marcar o customer como apto para operações de wallet somente após confirmar o conjunto completo e registrar `provisioning_scope_version`.
5. Se um Item faturável se torna aplicável depois, materializar idempotentemente a `item_wallet` para todos os customers ativos do escopo antes de aceitar consumo desse Item.
6. Se um Item deixa de ser aplicável, acrescentar um `WalletLifecycleEvent` que leva seu estado efetivo a `DISABLED`; não atualizar a linha `Wallet` nem excluir extrato, contadores ou pendente. Reativação acrescenta outro evento e reutiliza a mesma Wallet.

O provisionador pode processar lotes em múltiplas transações, mas o estado do customer permanece `PROVISIONING` ou `ERROR` até convergir. Restrições únicas tornam retries seguros. Endpoints de consumo, crédito e elegibilidade nunca criam Wallet; quando falta uma Wallet esperada, retornam `503 wallet_not_provisioned` e geram alerta operacional, sem alterar qualquer ledger.

### 4.7 UsageEvent e Debit

`UsageEvent` representa a chamada de consumo enviada pelo cliente:

- `usage_event_id`;
- `customer_id`, `item_wallet_id`;
- `transaction_id`;
- `product_id`, `item_id`;
- `item_units`, inteiro positivo na unidade definida pelo Item;
- `expected_price_version_id` opcional;
- `occurred_at` opcional, informado pelo cliente apenas para auditoria;
- `accepted_at`, definido pelo relógio do banco após serializar o medidor;
- metadados limitados;
- `pending_item_units_before`, `converted_item_units`, `converted_blocks` e `pending_item_units_after`;
- `unit_offset_start` e `unit_offset_end`;
- detalhamento de alocação por versão de preço e faixa;
- referência obrigatória a `ItemWalletEntry` e referências opcionais a `Debit`/`CustomerWalletEntry`, ausentes quando nenhum bloco é completado;
- status HTTP original e recibo original; na V1, chamadas aceitas retornam `201` e insuficiência de crédito retorna `409` sem recibo de consumo.

`Debit` existe somente quando o evento completa um ou mais blocos. Ele agrega, em uma única `CustomerWalletEntry`, todos os blocos completados atomicamente pela chamada e preserva uma decomposição imutável por Price/faixa. `debited_credit_units` é a soma dos wallet credits configurados nos blocos, nunca inclui `pending_item_units_after` e nunca arredonda uma fração para cima.

O item deve pertencer ao produto informado e ambos devem estar ativos no primeiro processamento. Falhas de catálogo não registram evento nem alteram acumulador. Na V1 `CREDIT_STRICT`, se o débito completo necessário deixaria a Wallet negativa, a insuficiência bloqueia integralmente o evento, acumulador e débito.

### 4.8 IdempotencyRecord e unicidade transacional

Campos principais:

- chave única `(customer_id, idempotency_key)`, com `customer_id` resolvido do `workspace_id` informado e chave recebida no cabeçalho `Idempotency-Key`;
- chave única independente `(customer_id, transaction_id)` nos recursos/transações de domínio;
- `operation_kind`;
- hash SHA-256 da representação canônica dos campos semanticamente relevantes;
- estado interno `PROCESSING` ou `COMPLETED`;
- resultado da primeira tentativa, referência ao recurso criado e estado necessário para auditoria/consulta;
- referência ao recurso e timestamps.

Semântica de igualdade:

- ordem de propriedades JSON e diferenças de transporte não importam;
- strings, IDs, quantidade, metadados e campos opcionais normalizados compõem o hash;
- `idempotency_key` identifica o registro e não entra no hash;
- qualquer segundo uso da mesma `Idempotency-Key` retorna `409 idempotency_key_already_used`, mesmo com hash idêntico;
- nova `Idempotency-Key` com `transaction_id` já existente retorna `409 transaction_already_exists`;
- o conflito pode expor IDs seguros do recurso/transação existente e `Location` para consulta, mas não devolve novamente o corpo de sucesso original;
- a mesma chave em outro tipo de operação também é conflito.

Registros concluídos precisam ser retidos por todo o prazo em que o cliente puder repetir uma requisição; para lançamentos de créditos, a recomendação é retenção permanente ou arquivo consultável sem perda das chaves únicas. Esse comportamento é uma deduplicação estrita e difere do replay convencional de APIs idempotentes.

### 4.9 DirectCredit

Concessão de créditos sob demanda solicitada pelo workspace:

- `direct_credit_id`, `customer_id`, `transaction_id`;
- `credit_units`, inteiro estritamente positivo;
- `external_reference` opaca para correlacionar uma compra, plano ou autorização externa, quando houver;
- entrada correspondente no razão.

A API da wallet não precifica nem aprova pagamentos. Um serviço comercial externo pode cobrar dinheiro e, após sua própria confirmação, solicitar a concessão de uma quantidade explícita de `credit_units` com `external_reference`. O valor monetário, a moeda e a lógica que escolheu essa quantidade não entram na wallet nem no razão de créditos.

`OnDemandPlan` é um catálogo comercial separado e exclusivo de crédito extra, pertencente a uma única `Subscription` comercial. Ele não é um tipo de `SubscriptionPlanVersion`, não pode ser escolhido como plano inicial de adesão e nunca cria, inicia, mantém ou substitui um `CustomerPlan`. A única oferta capaz de criar/manter esse vínculo principal é `SubscriptionPlanVersion` (`FREE` ou `PAID`, recorrente ou `NONE`).

Uma compra OnDemand existe somente dentro de um `CustomerPlan` principal em condição comercial `ACTIVE`, com `activation_status = ACTIVATED` e versão não revogada, tanto na criação quanto na efetivação. `ACTIVE` comercial não basta enquanto a ativação original obrigatória ainda estiver pendente. O `OnDemandPlan` deve pertencer à mesma `Subscription` comercial inferida pelo `SubscriptionPlanVersion` contratado; plano sob demanda de outra Subscription comercial é rejeitado. Ela referencia esse CustomerPlan e o `OnDemandPlan`, cria uma `CollectionRequest` sujeita à `payment_completion_window` e, depois da cobrança confirmada e nova validação dessas condições, cria somente `DIRECT_CREDIT`/lote persistente na Wallet. Não possui ciclo, renovação ou entitlement próprios e não altera versão, recorrência, âncora ou plano principal. Saldo zero não encerra CustomerPlan nem impede a compra quando ele continua elegível. `PAST_DUE`/`RENEWAL_INACTIVE`, `EXPIRED`, `CANCELED` e `REVOKED` não podem iniciar nem efetivar nova compra OnDemand, ainda que a Wallet tenha saldo. Isso só veda a nova compra: consumo de crédito já disponível segue a elegibilidade `CREDIT_STRICT` própria. CustomerPlan inativo, ativação pendente, versão revogada, Subscription comercial diferente, expiração da solicitação ou confirmação ausente não produzem crédito.

Na V1, um CustomerPlan comercialmente `ACTIVE` e com `activation_status = ACTIVATED` pode acessar qualquer `OnDemandPlan` publicado e não revogado pertencente à mesma Subscription comercial. Não há allowlist por `SubscriptionPlanVersion` e não há acesso a OnDemandPlan de outra Subscription comercial. Essa compatibilidade e a ativação são verificadas antes de criar a `CollectionRequest`, sem transformar o adicional em plano de adesão ou entitlement.

### 4.10 Subscription, SubscriptionPlanVersion, OnDemandPlan e CustomerPlan

`Subscription` é o agregado comercial raiz, como `One Vibe Pro`, e possui catálogo próprio de `SubscriptionPlanVersion` e `OnDemandPlan`. Outra Subscription comercial possui catálogo separado. Ela não cobra, confirma pagamento nem movimenta Wallet. Na V1, ela é disponível no instante da criação e não possui máquina de estados: não há rascunho, publicação posterior, ativação posterior, pausa, arquivamento ou revogação da Subscription raiz.

Hierarquia canônica:

```text
Subscription comercial
├── SubscriptionPlanVersion (oferta aderível)
└── OnDemandPlan (crédito extra)

CustomerPlan ──> SubscriptionPlanVersion ──> Subscription comercial
```

`Subscription` identifica o catálogo comercial; `SubscriptionPlanVersion` é o plano de assinatura versionado desse catálogo; `OnDemandPlan` é o adicional de crédito desse mesmo catálogo; e `CustomerPlan` é o agregado individual rico do customer. O CustomerPlan não persiste `subscription_id`: essa relação é sempre derivada pelo plano contratado. A disponibilidade de adesão, troca e renovação é decidida exclusivamente pelas `SubscriptionPlanVersion` pertencentes à Subscription e pela revogação de cada versão; a raiz não oferece controles de lifecycle para isso.

Cada `Subscription` possui `subscription_model` imutável desde a criação, quando já se torna disponível; o contrato aceita exatamente os três valores abaixo e não existe um quarto modelo `HYBRID`. A criação de `SubscriptionPlanVersion` compõe dimensões explícitas de modelo comercial, recorrência, admissão e meios aceitos; essa composição substitui tipos rígidos de plano e pode ganhar novas dimensões/valores em versão futura sem reclassificar os planos já publicados.

- `CREDIT_STRICT`: único perfil publicável na V1; aceita somente Products `CREDIT_METERED`. Um Plan pode ser gratuito ou pago, e sua recorrência é uma dimensão enumerada própria. Falha definitiva/expiração de renovação paga coloca a renovação em condição inativa, sem novo ciclo, cobrança automática ou franquia nova, mas não revoga o uso de créditos já disponíveis. Consumo só é aceito se mantiver o saldo maior ou igual a zero.
- `CREDIT_FLEXIBLE`: capacidade futura não ativável/comercializável na V1. Quando habilitada em versão posterior, aceita Products `CREDIT_METERED` e `ENTITLEMENT_ONLY` na mesma oferta.
- `ENTITLEMENT_ONLY`: capacidade futura não ativável/comercializável na V1. Quando habilitada em versão posterior, aceita somente Products `ENTITLEMENT_ONLY`.

Matriz normativa:

| `subscription_model` | Nome funcional | Plano base | Products permitidos | Wallet | Regra de acesso |
| --- | --- | --- | --- | --- | --- |
| `CREDIT_STRICT` | assinatura estrita com créditos independentes | preço, recorrência e `granted_credit_units` como franquia-base do ciclo; Voucher/Cupom/Compensation/OnDemand continuam fontes separadas | somente `CREDIT_METERED` | saldo é verificado para uso, inclusive após renovação inativa | falha de renovação bloqueia apenas novo ciclo/franquia/cobrança automática; crédito já disponível continua utilizável |
| `CREDIT_FLEXIBLE` | capacidade futura | regras comerciais e de crédito a definir separadamente | `CREDIT_METERED` e/ou `ENTITLEMENT_ONLY` | regra futura | não ativável na V1 |
| `ENTITLEMENT_ONLY` | capacidade futura | preço e recorrência sem créditos embutidos | somente `ENTITLEMENT_ONLY` | não consulta saldo | não ativável na V1 |

Exemplos de decisão:

- `CREDIT_STRICT`: a Wallet ainda tem 40 créditos e a única tentativa comercial de renovação falhou definitivamente; `renewal_status = RENEWAL_INACTIVE`, mas `access_allowed = true` para Product habilitado enquanto o débito couber no saldo. Não há novo ciclo, cobrança automática ou franquia nova.
- `CREDIT_FLEXIBLE`: a renovação falhou e a Wallet tem 40 créditos; Products medidos permanecem disponíveis até o saldo deixar de ser positivo, enquanto Products `ENTITLEMENT_ONLY` continuam disponíveis até cancelamento. Não há concessão de novos créditos sem pagamento.
- `ENTITLEMENT_ONLY` recorrente: a renovação está confirmada; `access_allowed = true` sem ler Wallet. Se falhar definitivamente, `access_allowed = false`.
- `ENTITLEMENT_ONLY` vitalício: o único pagamento é confirmado, ativa sem crédito e persiste `next_renewal_at = null` e `entitlement_ends_at = null`; não existe scheduler de renovação.

#### Composição híbrida de `CREDIT_FLEXIBLE`

“Híbrida” é somente um caso de uso de `CREDIT_FLEXIBLE`, não enum, entidade, estado ou quarto perfil. Ela ocorre quando a mesma oferta flexível inclui ao menos um Product `CREDIT_METERED` e um `ENTITLEMENT_ONLY`. A API aplica a regra de cada Product individualmente: o primeiro depende de saldo; o segundo depende apenas do entitlement persistente até cancelamento. Uma oferta `CREDIT_FLEXIBLE` contendo apenas Products medidos continua sendo o mesmo modelo.

Cada `SubscriptionPlanVersion` pertence a uma única `Subscription` comercial e é a única oferta aderível: cria e mantém um `CustomerPlan`. Cada `OnDemandPlan` também pertence a uma única Subscription comercial, mas não é aderível nem é `SubscriptionPlanVersion`. O `CustomerPlan` referencia diretamente o `SubscriptionPlanVersion` que estava ativo e não revogado em sua ativação; a Subscription comercial é inferida por esse plano, sem campo redundante no CustomerPlan. Quando recorrente, ele usa essa versão em cada renovação enquanto ela permanecer não revogada. Na V1, ele pode nascer comercialmente `ACTIVE` com ativação ainda pendente: em plano pago, entitlement e `activation_status = ACTIVATED` só se tornam efetivos após a primeira confirmação definitiva do cartão, inclusive após eventual autenticação adicional exigida pelo emissor; em `FREE` que exige cartão, isso ocorre após a confirmação definitiva do setup/validação; e em `FREE` de adesão livre ocorre imediatamente. Plano gratuito não gera cobrança. O Plan descreve a composição comercial; o CustomerPlan materializa os períodos e resultados dessa composição, sem poder alterá-la.

Cada `SubscriptionPlanVersion` é imutável depois de publicado e define:

- `plan_version_id`, `subscription_id`, nome e vigência;
- `commercial_model`, enum `FREE | PAID`: `FREE` não possui cobrança, moeda ou valor; `PAID` exige `currency` ISO 4217 e valor monetário inteiro mínimo estritamente maior que zero. Não existe plano `PAID` de valor zero. Validação de cartão ou autorização de valor zero não é cobrança nem preço;
- `recurrence`, enum de V1 `NONE | WEEKLY | MONTHLY | QUARTERLY | ANNUALLY`. `NONE` é contratação única; os demais valores são recorrentes. O enum é uma escolha deliberada da V1 e pode ser estendido em versão futura sem criar tipos rígidos de plano;
- não possui duração, data de expiração nem término automático configurável. A recorrência somente define se existem ciclos/renovações futuras; `NONE` não cria cobrança nem ciclo futuro e não encerra o CustomerPlan pela passagem do tempo;
- `granted_credit_units`, inteiro não negativo de créditos-base da assinatura por ciclo. Zero representa entitlement sem créditos-base; valor positivo é concedido no máximo uma vez por ciclo materializado, nunca por Voucher/Cupom;
- `admission_policy_id`, referência à política versionada que controla a criação da assinatura;
- meios aceitos pelo plano quando houver cobrança ou validação. Na V1, o único meio permitido é `CARD` tokenizado via provedor; Pix, boleto e outros permanecem futuros e não podem ser ativados;
- conjunto versionado de `product_id` habilitados;
- estado `DRAFT`, `SCHEDULED`, `ACTIVE`, `RETIRED` ou `REVOKED`. `REVOKED` é uma revogação administrativa terminal da versão publicada, distinta de retirar uma versão ainda não contratável do catálogo.

`SubscriptionPlanVersion` é a regra de produto versionada: governa modelo comercial, preço, recorrência, política de admissão e meios aceitos. A recorrência pertence exclusivamente a essa versão do plano. Depois de publicada, a versão é imutável: não se edita preço, créditos, recorrência, admissão, meios aceitos ou Products; uma oferta comercial nova exige criar outra versão/plano. Uma versão nunca expira pela passagem do tempo; só fica indisponível por sua revogação administrativa.

`OnDemandPlan` possui ao menos `on_demand_plan_id`, `subscription_id`, quantidade positiva de `credit_units` e estado de publicação/revogação. Seu `subscription_id` referencia a mesma Subscription comercial que delimita a compra; ele não possui `recurrence`, política de admissão, ciclo ou estado de assinatura. Na V1, somente um OnDemandPlan publicado e não revogado da mesma Subscription comercial do CustomerPlan é comprável.

#### Revogação administrativa de `SubscriptionPlanVersion`

O operador autorizado pode revogar uma versão publicada, com razão, ator e instante auditáveis. A revogação de plano é terminal e tem efeito imediato sobre a oferta: bloqueia novas adesões/`CustomerPlan`, trocas para a versão e qualquer renovação futura dela. Não há migração ou substituição automática, nova cobrança, Voucher/Cupom, estorno, Compensation, e-mail ou outro outreach na V1.

Ela não é a revogação administrativa de um `CustomerPlan`: esta última encerra imediatamente um vínculo individual em `REVOKED`; a revogação de plano preserva os vínculos já confirmados somente até o `current_period_end` do ciclo em andamento. No fim materializado, o CustomerPlan encerra de modo idempotente em `EXPIRED` com `end_reason = PLAN_REVOKED`, sem tentativa de cobrança, novo ciclo ou nova franquia-base. Créditos já disponíveis na Wallet permanecem sob suas regras de origem/lote, sem remoção, estorno ou Compensation automática. Também não se confunde com `cancel_at_period_end` solicitado pelo customer nem com `PAST_DUE`/`RENEWAL_INACTIVE` por falha de pagamento.

Uma `CollectionRequest` de adesão, renovação, regularização, transição ou OnDemand que ainda esteja pendente para a versão revogada é terminalizada idempotentemente em `CANCELED` com `terminal_reason = PLAN_REVOKED`. A validação de efetivação confere que a versão continua não revogada, além de vínculo, valor, moeda, estado e prazo; webhook tardio dessa solicitação não pode efetivar ciclo, entitlement, crédito, troca de versão ou nova cobrança. Um pagamento externo sem solicitação ainda efetivável segue a trilha de `UnmatchedPaymentCase`/revisão manual, nunca uma reativação automática.

Matriz de composição da V1:

| Modelo comercial | Recorrência | Permanência do CustomerPlan | Admissão/início | Créditos-base | Cobrança futura |
| --- | --- | --- | --- | --- | --- |
| `FREE` | `NONE` | permanece ativo por tempo indeterminado, salvo transição explícita | livre ou exige cartão `CARD` já validado | uma vez na ativação, se positivo | não há cobrança automática |
| `FREE` | `WEEKLY`, `MONTHLY`, `QUARTERLY` ou `ANNUALLY` | permanece ativo; ciclos não são prazo de término | livre ou exige cartão `CARD` já validado | ativação e cada virada de ciclo, se positivo | não há cobrança automática; a recorrência é calendário do direito, não preço |
| `PAID` | `NONE` | permanece ativo por tempo indeterminado após a confirmação única, salvo transição explícita | exige pagamento inicial `CARD` definitivamente confirmado | uma vez após confirmação, se positivo | não há cobrança automática posterior |
| `PAID` | `WEEKLY`, `MONTHLY`, `QUARTERLY` ou `ANNUALLY` | permanece ativo enquanto cada ciclo for renovado ou até transição explícita | exige pagamento inicial `CARD` definitivamente confirmado | uma vez por ciclo pago confirmado, se positivo | uma única tentativa comercial no cartão salvo, na data de cada renovação |

São inválidas na V1: `PAID` sem moeda/valor positivo; `PAID` efetivar entitlement antes do primeiro pagamento confirmado; `FREE` com cobrança; qualquer meio ativo que não seja `CARD`; e criar nova cobrança para resolver uma resposta incerta.

#### Exemplos normativos de composição e campanhas

Um plano é composto por `commercial_model`, `recurrence`, política de admissão, meios aceitos e `granted_credit_units`. Voucher, Cupom e Compensation não são dimensões do plano. Os créditos-base são originados pelo ciclo materializado da assinatura e sua versão de plano; Voucher/campanha concede apenas bônus excepcional fora dessa franquia padrão. Nenhum plano possui duração ou expiração temporal configurável.

- **FREE, adesão livre:** a política aprova o CustomerPlan sem cartão e sem `CollectionRequest`; não existe cobrança nem término automático por tempo, e o ciclo de ativação concede `granted_credit_units` uma única vez quando positivo.
- **FREE, `CARD` validado, sem cobrança:** o CustomerPlan pode ficar comercialmente `ACTIVE` com `activation_status = PENDING_CARD_VALIDATION` enquanto o setup seguro ocorre no provedor. Essa validação não é preço, autorização de cobrança nem cobrança; somente o webhook assinado que a confirma muda para `ACTIVATED`, efetiva o entitlement `FREE` e permite OnDemand.
- **FREE com ciclo de calendário:** um plano `FREE + MONTHLY`, por exemplo, materializa ciclos mensais de entitlement e, sem Stripe, concede a franquia `granted_credit_units` de cada ciclo uma única vez quando positiva.
- **PAID + NONE:** o CustomerPlan é criado com `PENDING_INITIAL_PAYMENT`, gera uma única cobrança inicial `CARD` e efetiva o entitlement e a franquia `granted_credit_units` somente quando a confirmação definitiva o muda para `ACTIVATED`. Não há cobrança, ciclo ou término automático futuro.
- **PAID recorrente:** um plano `PAID + MONTHLY` ou `PAID + ANNUALLY` exige a cobrança inicial confirmada e, em cada ciclo posterior, faz exatamente uma tentativa comercial no cartão salvo na data de renovação. Cada confirmação concede a franquia do ciclo uma única vez quando positiva. Timeout ou resposta incerta são reconciliados na mesma tentativa; nunca originam segunda cobrança.

Campanhas promocionais são agregados próprios e podem manter uma relação opcional com `customer_plan_id`, `subscription_id` comercial e/ou `plan_version_id` somente como condição de elegibilidade, por exemplo “customer possui CustomerPlan ativo no plano X”. Essa relação não faz o plano conter, selecionar ou emitir Voucher/Cupom.

- **Voucher inicial de campanha:** depois de aprovada/criada uma Subscription, a campanha verifica suas condições (inclusive plano ativo, se houver), emite um Voucher próprio e rastreável e o crédito só ocorre no resgate desse Voucher. A criação da Subscription, por si, não cria o Voucher.
- **Cupom de campanha para cobrança futura:** uma campanha pode emitir um Cupom que habilita desconto ou condição numa futura cobrança `PAID`, conforme seu contrato próprio. O Cupom é avaliado na `CollectionRequest` elegível e não modifica preço, recorrência ou definição da `SubscriptionPlanVersion`; esse uso de desconto de cobrança requer o contrato de campanha/Billing correspondente antes de ser ativado.

#### Política de admissão da assinatura

`SubscriptionAdmissionPolicy` é uma política versionada, reutilizável e definida fora da precificação; o plano a referencia como uma dimensão de composição. Ela decide se uma adesão pode ser criada, sem alterar preço, cobrança, entitlement, Voucher, Cupom ou saldo. A política pode ser livre ou exigir fatos/evidências verificáveis, como e-mail ou identidade confirmados, ou uma referência antifraude para um cartão já validado pelo provedor. O sistema armazena somente a referência opaca e o resultado da validação; nunca dados sensíveis de cartão. Para `PAID` na V1, a política não substitui a confirmação definitiva do primeiro pagamento exigida antes de efetivar entitlement.

Uma política reprovada retorna erro de admissão e não cria CustomerPlan. Quando a única pendência é a validação de cartão exigida por um plano `FREE`, ela cria o CustomerPlan com ativação pendente, sem entitlement, crédito ou OnDemand até o webhook assinado confirmar o setup. Uma campanha promocional é outro conceito: ela pode analisar o contexto da assinatura criada e, se suas próprias regras forem satisfeitas, emitir um Voucher rastreável. Nem a aprovação da admissão nem a criação da assinatura causam Voucher ou crédito automaticamente. Limites como “um único Voucher por customer” pertencem à campanha e só existem quando explicitamente definidos nela.

`CustomerPlan` é o agregado individual rico do customer com uma `SubscriptionPlanVersion`. Ele mantém estado, datas, ciclos materializados, referências de cobrança, entitlement e efeitos de Wallet; não é uma mera join table e não escolhe nem redefine preço, recorrência ou admissão. Ele aponta diretamente ao plano contratado; a `Subscription` comercial é inferida pela relação `CustomerPlan → SubscriptionPlanVersion → Subscription` e não é armazenada redundante. A passagem do tempo isolada nunca encerra CustomerPlan:

- `customer_plan_id`, `customer_id`, `plan_version_id`;
- instantâneo de `commercial_model`, `recurrence`, `granted_credit_units`, política de admissão e meios aceitos da versão contratada;
- para recorrência diferente de `NONE`, âncora calendária UTC preservada, períodos `[current_period_start, current_period_end)` derivados do enum e consequência da falha; em `PAID` há exatamente uma tentativa comercial na data de renovação calculada por essa âncora, sem antecipação ou retentativa automática;
- `payment_completion_window`, política de comportamento da Subscription, distinta de qualquer carência de inadimplência/renovação: define por quanto tempo uma solicitação pode aguardar confirmação e é aplicada à contratação inicial `PAID`, renovação, regularização manual de renovação, transição paga e compra de `OnDemandPlan`;
- `unexpected_payment_policy = MANUAL_REVIEW` na V1, versionada e copiada para cada solicitação de cobrança; ela exige avaliação operacional e nunca aciona refund local;
- estado comercial derivado de eventos `ACTIVE` (inclusive enquanto aguarda a primeira confirmação), `ACTIVE_PAID`, `PAST_DUE`, `EXPIRED`, `CANCELED` ou `REVOKED`;
- `activation_status`, separado do estado comercial e usado pela elegibilidade de OnDemand: `PENDING_CARD_VALIDATION` enquanto um plano `FREE` exige cartão ainda não confirmado; `PENDING_INITIAL_PAYMENT` enquanto um plano `PAID` aguarda confirmação definitiva; ou `ACTIVATED` depois de satisfeitas todas as condições obrigatórias. `FREE` com admissão livre entra diretamente em `ACTIVATED`. O estado comercial `ACTIVE` com ativação pendente não concede entitlement nem autoriza OnDemand;
- `renewal_status`, derivado e separado do uso: `CURRENT` ou `RENEWAL_INACTIVE`. Falha definitiva ou expiração de `CollectionRequest` de renovação coloca `PAST_DUE`/`RENEWAL_INACTIVE` imediatamente, impede novo ciclo, franquia e cobrança automática, mas não é revogação de créditos já disponíveis;
- início, `current_period_end`/fim materializado quando aplicável, pedido de cancelamento opcional e referências comerciais externas;
- próximo período somente quando `recurrence != NONE` e histórico append-only de ativação, falha/regularização de renovação, troca de plano, cancelamento e revogação.

CustomerPlan aplica no tempo a composição já publicada do plano e nunca a modifica silenciosamente. Ele pode ficar `ACTIVE`, pendente de pagamento, encerrado ou em outro estado comercial, sem adquirir uma recorrência própria. Na V1, seu encerramento decorre somente de transição explícita: cancelamento, revogação administrativa individual ou revogação da versão de plano ao fim do ciclo vigente; fim de ciclo, expiração de lote, `CollectionRequest` expirada, `PAST_DUE` ou passagem do tempo isolada não o encerram. Uma mudança comercial futura exige nova `SubscriptionPlanVersion` ou transição explícita e auditável de plano; nunca mutação implícita do CustomerPlan. Cada ciclo materializado possui identidade única por CustomerPlan + período/ativação e pode gerar no máximo uma entrada `SUBSCRIPTION_CREDIT`, vinculada tipadamente ao ciclo e à versão do plano. Um domínio de Billing cobra e confirma o pagamento de um plano pago; essa confirmação efetiva atomicamente entitlement e, quando positivo, a franquia-base na `customer_wallet`.

#### Cálculo calendário dos ciclos da assinatura

Para recorrência diferente de `NONE`, o CustomerPlan conserva uma âncora calendária em UTC. Ela é capturada quando a série de ciclos inicia e é copiada em cada ciclo materializado; renovação normal calcula o próximo período a partir dessa mesma âncora, sem a substituir pela data encurtada de um mês anterior. Upgrade, downgrade e regularização manual confirmada iniciam uma nova série e redefinem a âncora no instante efetivo já definido para cada fluxo. Todo período é o intervalo semiaberto `[current_period_start, current_period_end)`: início inclusivo, fim exclusivo, sem sobreposição; um evento exatamente no fim pertence ao próximo período.

- `WEEKLY`: cada próximo fim é a âncora mais múltiplos de 7 dias, preservando dia da semana e horário UTC.
- `MONTHLY`: preserva dia e horário UTC originais da âncora. Se o mês de destino não possuir esse dia, usa seu último dia; o cálculo seguinte continua referenciado ao dia original, voltando a ele assim que o mês comportá-lo. Exemplo: `31 jan → 28/29 fev → 31 mar → 30 abr → 31 mai`.
- `QUARTERLY`: usa a mesma regra mensal para o mês obtido ao somar três meses por período à âncora preservada; mês curto não altera o dia-alvo dos trimestres seguintes.
- `ANNUALLY`: preserva mês, dia e horário UTC. Uma âncora em 29 de fevereiro cai no último dia de fevereiro em ano não bissexto e volta ao dia 29 quando o ano de destino for bissexto.
- `NONE`: não materializa nem agenda ciclo ou cobrança futura e não encerra o CustomerPlan pela passagem do tempo.

O scheduler usa essa aritmética exclusivamente para materializar a virada de ciclo e, quando `PAID`, agendar a única cobrança na respectiva `current_period_end`; `FREE` materializa a virada/concessão sem Stripe. Não há prorrata, cálculo monetário adicional, Stripe Subscriptions ou deslocamento de fuso local nessa regra.

#### Exclusividade da assinatura principal ativa

Há no máximo um CustomerPlan principal em condição comercial ativa por `(customer_id, subscription_id)` da Subscription comercial inferida pelo plano contratado. Para esta regra, a condição `ACTIVE` inclui a realização paga `ACTIVE_PAID`; `PAST_DUE`/`RENEWAL_INACTIVE`, `EXPIRED`, `CANCELED` e `REVOKED` não ocupam a chave. Como `CustomerPlan` não armazena `subscription_id`, a implementação cria/bloqueia uma reserva relacional `ActiveCustomerPlanSlot(customer_id, subscription_id, customer_plan_id)`, derivada de `plan_version_id → SubscriptionPlanVersion → Subscription`, com unicidade em `(customer_id, subscription_id)`. Essa garantia transacional não introduz `subscription_id` redundante no CustomerPlan e não limita o customer a uma única Subscription comercial.

A criação e qualquer transição que entre na condição ativa — inclusive confirmação inicial ou regularização manual — bloqueiam/verificam `ActiveCustomerPlanSlot` na mesma transação que materializa CustomerPlan, ciclo e efeitos. Duas adesões concorrentes com chaves de idempotência diferentes não podem vencer: uma cria/ativa o vínculo e a outra retorna `409 active_customer_plan_already_exists`, sem cobrança, ciclo, entitlement ou crédito novo. Repetição com a mesma `Idempotency-Key` preserva o contrato geral de `409 idempotency_key_already_used`; a API pode apontar o CustomerPlan ativo existente para consulta. Troca de plano não cria um segundo CustomerPlan: ela usa o vínculo que já ocupa a chave e altera sua versão somente pela transição auditável. `OnDemandPlan` referencia esse vínculo e cria somente sua `CollectionRequest`/crédito, nunca um CustomerPlan.

Regras:

- Subscription controla entitlement, mas nunca altera saldo diretamente;
- `payment_completion_window` não é carência de inadimplência nem política de falha de renovação; expirar uma solicitação encerra somente aquela solicitação, e a consequência comercial de uma falha de renovação continua sendo aplicada separadamente pela regra já publicada;
- `CREDIT_FLEXIBLE` e `ENTITLEMENT_ONLY` permanecem capacidade futura não ativável na V1;
- pagamento confirmado ativa ou renova o entitlement comercial e concede, uma única vez, `granted_credit_units` positivo do ciclo; nunca aplica Voucher ou Cupom;
- falha, pendência, estorno ou ausência de pagamento não geram franquia-base, Voucher ou Cupom e não alteram a Wallet;
- falha definitiva ou expiração de renovação muda a condição de renovação para inativa imediatamente, sem criar outro ciclo/tentativa; não muda, por si só, a autorização de consumir crédito já disponível;
- cancelamento e revogação afetam entitlement conforme seu instante efetivo e não revertem créditos anteriores;
- troca de plano é transição explícita, versionada e auditável; preserva o histórico e os lançamentos anteriores, mas pode reiniciar a âncora e o fim materializado do ciclo conforme a política V1 abaixo;
- um Product `ENTITLEMENT_ONLY` ignora saldo; um Product `CREDIT_METERED` exige entitlement e saldo elegível;
- Billing não define preço, calendário, política de retries, entitlement ou quantidade de créditos; ele materializa a composição publicada do plano. Para `FREE`, não cria cobrança; para `PAID`, executa a cobrança inicial e as renovações previstas.

#### Regularização manual de renovação inativa

Em `PAST_DUE`/`RENEWAL_INACTIVE`, não existe retry, carência automática ou pós-pago. O customer pode atualizar ou escolher cartão na superfície segura do provedor e iniciar explicitamente uma regularização. Essa ação cria uma nova `CollectionRequest` idempotente de propósito `RENEWAL_REGULARIZATION`, com novo `payment_expires_at`, nova idempotency key externa e correlação/metadata próprias; ela é distinta da solicitação falha ou expirada e nunca a reabre, reutiliza ou cobra novamente.

Até a confirmação definitiva, o CustomerPlan permanece `PAST_DUE`/`RENEWAL_INACTIVE`: não há novo ciclo, franquia-base nem cobrança automática, embora o consumo de crédito já disponível continue sob a elegibilidade `CREDIT_STRICT`. Somente o webhook assinado que confirma a nova solicitação, após validar PaymentIntent, vínculo, valor, moeda, transição de estado e idempotência, retorna o CustomerPlan à condição `ACTIVE_PAID`/`CURRENT`, inicia um novo ciclo com âncora na efetivação e cria no máximo uma franquia-base integral do plano vigente. Não há concessão retroativa do período `PAST_DUE`; lotes de assinatura e OnDemand preservam suas regras próprias de expiração/persistência.

#### Cancelamento normal, pendência e revogação administrativa

Não existe pausa nem retomada de `CustomerPlan` na V1, por customer ou por operação: não há estados, endpoints, transições ou mecanismo que congele ciclo, créditos ou cobrança. Os caminhos de interrupção/encerramento relevantes são cancelamento normal `cancel_at_period_end`, revogação administrativa e falha de renovação seguida de regularização manual; troca de plano continua sendo a transição comercial distinta já definida.

Na V1, cancelamento normal solicitado pelo customer de um `CustomerPlan` recorrente significa `cancel_at_period_end`: desabilita a próxima renovação e qualquer `CollectionRequest` futura, mas não encerra o ciclo já confirmado. Até `current_period_end` — ou a data final materializada do ciclo atual — o CustomerPlan permanece `ACTIVE`, mantém o entitlement, pode consumir saldo conforme `CREDIT_STRICT` e, se `activation_status = ACTIVATED`, pode iniciar/efetivar compra `OnDemandPlan` publicado/não revogado da mesma Subscription comercial. Cancelamento normal não gera estorno automático, não remove créditos e não cria Compensation.

No fim desse ciclo confirmado, a transição idempotente usa `CANCELED` como estado terminal canônico: não materializa novo ciclo, não cria nova `CollectionRequest` nem concede créditos-base. Repetir o pedido de cancelamento apenas retorna o mesmo agendamento/estado; nunca antecipa o término por acidente.

`PENDING_PAYMENT` é estado de solicitação/contratação ainda não confirmada, distinto de cancelar um CustomerPlan `ACTIVE` em período confirmado. Cancelar nessa fase encerra apenas a `CollectionRequest`/intenção pendente de forma idempotente, sem entitlement, ciclo efetivo ou crédito; ela não é reaberta e cliques posteriores preservam as regras de idempotência e prevenção de cobrança duplicada. Uma nova contratação posterior segue como nova intenção conforme a política de expiração já definida.

Revogação administrativa individual é ação distinta, restrita a operador autorizado e auditada com razão, ator, instante e referências correlatas. Ela é a transição de maior prioridade do CustomerPlan: no mesmo commit/serialização que o marca terminalmente como `REVOKED`, cancela ou terminaliza toda `CollectionRequest` ainda pendente vinculada a ele — adesão inicial, upgrade, downgrade que exija cobrança, regularização, renovação ou OnDemand — com `terminal_reason = CUSTOMER_PLAN_REVOKED`. A operação preserva as referências de cada solicitação e sua tentativa, mas nenhuma é reaberta, reutilizada ou substituída por nova cobrança.

Todo comando não administrativo e toda efetivação de webhook revalidam o CustomerPlan bloqueado antes de produzir efeitos. Depois de `REVOKED`, regularização, upgrade, downgrade, cancelamento normal, OnDemand, renovação e qualquer outra transição comercial retornam `409 customer_plan_revoked`; webhook tardio ou corrida que perder a revogação apenas converge/audita a solicitação terminalizada e não pode efetivar plano, ciclo, entitlement, crédito ou cobrança nova. A revogação vence pendências ainda não efetivadas e não permite reativação.

Ela não é cancelamento normal no fim do ciclo, nem revogação da `SubscriptionPlanVersion`, nem falha de renovação: somente a primeira encerra imediatamente este vínculo individual. Revogar não produz estorno, remoção de crédito, perda/edição de Wallet ou Compensation automática; qualquer consequência financeira continua excepcional, manual e submetida à política de Compensation/estorno externo da V1.

#### Troca de plano no meio do ciclo

Na V1, troca de plano é imediata após sua condição de entrada e reinicia a âncora na data/hora efetiva. `CustomerPlan` permanece o vínculo; `SubscriptionPlanVersion` é a regra escolhida; `CollectionRequest` é somente a cobrança que uma transição exigir. A nova versão deve pertencer à mesma Subscription comercial inferida pelo plano atual; a transição não cruza catálogos comerciais. A transição append-only registra versão anterior/nova, classificação `UPGRADE|DOWNGRADE`, ator, instante, âncora e `current_period_end` antigo/novo, além de referências de cobrança quando houver. Saldo de crédito nunca é estado de CustomerPlan.

**Upgrade.** Criar uma `CollectionRequest` idempotente de propósito `PLAN_UPGRADE` para o valor integral do novo plano e aguardar o webhook assinado de confirmação. Só então o residual de assinatura existente é reclassificado para `ON_DEMAND` persistente, o CustomerPlan muda para a nova versão da mesma Subscription comercial, inicia novo ciclo na hora efetiva e concede integralmente `granted_credit_units` do novo plano como novo crédito de assinatura expirável. Não há desconto, bônus, Voucher/Cupom, prorrata, preço convertido, câmbio ou cálculo proporcional. Enquanto a cobrança estiver `PENDING_PAYMENT` ou se falhar/expirar, plano, ciclo, entitlement, Wallet e âncora permanecem inalterados; a mesma intenção segue as regras existentes de idempotência e não duplica cobrança.

**Downgrade.** O residual de assinatura existente é imediatamente reclassificado para `ON_DEMAND` persistente, e então a nova versão é aplicada, sem `CollectionRequest`, cobrança ou concessão de crédito novo nesse momento, iniciando novo ciclo na hora da troca. A primeira cobrança do plano menor acontece apenas no fim desse novo ciclo; não há estorno, Compensation, desconto, Voucher/Cupom, preço convertido, câmbio ou proporcionalidade automáticos.

Nos dois sentidos, o saldo residual de lotes de assinatura existentes é preservado por reclassificação para crédito sob demanda persistente e fica utilizável junto aos lotes `OnDemandPlan`; não permanece classificado como franquia de assinatura nem expira no fim do novo ciclo. Lotes `OnDemandPlan`, inclusive os reclassificados, não expiram porque houve renovação/troca. A reclassificação não cria crédito adicional nem muda o saldo da Wallet. A concessão adicional do upgrade é exclusivamente a franquia-base integral do novo ciclo confirmado; downgrade não concede franquia nova.

### 4.11 BillingConnection, CollectionRequest, BillingPayment e WebhookInbox

Nosso sistema é sempre a fonte de verdade da assinatura. Na V1, um provedor externo de cartão armazena/tokeniza o meio e processa cobranças iniciadas pelo Billing, sem governar o plano ou calendário local. O sistema mantém somente IDs/tokens opacos e referências correlatas: PAN, CVV e dados sensíveis de cartão nunca transitam nem são persistidos aqui.

O núcleo de Billing é agnóstico a provedor. Um provedor só participa depois que um adaptador implementa `BillingConnector`; “suportar qualquer provedor” significa poder adicionar conectores sem alterar Subscription, Wallet ou a máquina de estados normalizada, não integração automática sem implementação.

#### Fluxo operacional de cartão na V1 (Stripe)

A aplicação apresenta o plano e aplica suas regras; Stripe não seleciona, cria nem conhece `Subscription` comercial, `SubscriptionPlanVersion`, `OnDemandPlan` ou `CustomerPlan` do domínio. O frontend usa uma superfície segura do Stripe, como Checkout hospedado ou Payment Element, para que PAN e CVV sejam enviados somente ao provedor, nunca ao backend.

**Etapa 1 — setup/validação do cartão.** O backend cria o contexto de setup, o cliente informa o cartão ao Stripe e a confirmação bem-sucedida produz um método de pagamento tokenizado/vaulted vinculado ao Customer Stripe. O domínio retém apenas IDs/referências opacas e metadados seguros. Essa confirmação prova que o cartão foi configurado/validado para uso futuro; não aprova nem garante qualquer cobrança futura. Na V1, o backend observa a confirmação definitiva por webhook/evento assinado do provedor; o retorno do navegador, isoladamente, não é autoridade. Para `FREE` que exige cartão, o CustomerPlan permanece `PENDING_CARD_VALIDATION` até esse evento; para `PAID`, setup válido ainda não remove `PENDING_INITIAL_PAYMENT`.

**Etapa 2 — cobrança condicional.** Para `FREE` cuja admissão exige `CARD` validado, a etapa 1 basta: não existe cobrança; o webhook de setup muda o CustomerPlan para `ACTIVATED`, efetiva entitlement e concede sua franquia-base quando positiva. Para `PAID`, Billing cria uma cobrança separada, rastreável e idempotente no cartão salvo, no valor daquela contratação. Só a confirmação definitiva dessa cobrança muda `PENDING_INITIAL_PAYMENT` para `ACTIVATED` e efetiva atomicamente o entitlement e a franquia-base do ciclo. Uma cobrança posterior pode falhar por saldo, limite, recusa ou autenticação adicional apesar de o cartão estar salvo/validado; nesse caso, o plano pago e quaisquer efeitos dependentes do pagamento, inclusive a franquia-base, não são efetivados.

**Correlação de cobrança e metadata.** Antes de chamar Stripe, Billing persiste a `CollectionRequest` interna e, ao criar o PaymentIntent, envia metadata opaca com ao menos `collection_request_id`, `customer_plan_id`, `plan_version_id`, `subscription_id` da Subscription comercial e `purpose` (`initial_subscription`, `renewal`, `renewal_regularization`, `top_up` ou `plan_upgrade`). `customer_id` ou `user_id` interno só pode ser incluído quando necessário e permitido pela política. Esses valores retornam nos eventos/webhooks e apoiam operação e conciliação. A autoridade continua sendo o registro Billing criado antes da chamada e seu vínculo persistido ao `payment_intent_id` do Stripe; metadata/description são redundância auditável, não autorização nem chave única de efetivação.

Metadata e description não podem conter e-mail, nome, endereço, PAN/CVV, documento ou qualquer PII/dado sensível. Na efetivação, o backend valida assinatura do webhook, PaymentIntent, vínculo interno, valor, moeda e transição de estado antes de convergir idempotentemente cobrança, entitlement e a única concessão-base do ciclo.

Renovação e recarga extra reutilizam a mesma fronteira: Billing, e não Stripe Subscriptions, decide data e valor e cria uma única cobrança rastreável no Stripe. A renovação segue a tentativa única na data prevista; a recarga extra é solicitada pelo usuário e só produz seu efeito após confirmação definitiva. Nenhuma dessas operações permite ao provedor criar a Subscription local ou mover a Wallet por conta própria.

**Múltiplos cartões e escolha.** Um Customer pode ter várias referências `CARD` tokenizadas/vaulted. Em uma cobrança iniciada pela aplicação, o Payment Element ou Checkout seguro configurado para esse Customer exibe cartões salvos e permite selecionar um ou cadastrar outro. O frontend não lê PAN/CVV: recebe apenas a referência opaca selecionada e o backend a vincula à `CollectionRequest`. Após sucesso, o domínio pode registrar essa referência como método padrão daquele `CustomerPlan` para renovações. Escolher um cartão para a cobrança atual não troca esse padrão; a troca do padrão é uma ação explícita e auditável do CustomerPlan.

**Recuperação manual da primeira cobrança `PAID`.** Setup/tokenização válida não garante a cobrança. A recuperação preserva a mesma `CollectionRequest` e a mesma intenção de pagamento do provedor como tentativa rastreável; trocar cartão ou voltar à superfície Stripe não cria uma segunda cobrança comercial.

| Resultado normalizado | Ação permitida |
| --- | --- |
| `REQUIRES_ACTION` / autenticação adicional | levar o usuário de volta ao Stripe para autenticar o mesmo cartão e a mesma intenção; |
| saldo insuficiente ou recusa recuperável | não efetivar o plano nem invalidar automaticamente o cartão; permitir que o usuário selecione cartão salvo ou cadastre outro no Stripe e continue a mesma intenção; |
| cartão expirado, removido ou definitivamente inutilizável | solicitar novo cartão no Stripe, registrar a nova referência e tornar a anterior inativa conforme evento/estado do provedor; |
| timeout, ausência ou estado incerto | preservar a mesma intenção e o estado conhecido; aguardar webhook assinado, sem criar outra cobrança ou iniciar consulta automática. |

Essa recuperação on-session de uma cobrança pendente é interação manual do usuário, não uma nova tentativa automática de renovação. A V1 continua sem retentativa automática de renovação: uma falha na única tentativa do ciclo só pode ser resolvida por fluxo manual explícito, sem duplicar a cobrança original.

`PaymentMethodBinding` mantém um estado normalizado mínimo: `ACTIVE/USABLE`, `REPLACED`, `DETACHED/INACTIVE` ou `INVALID`. Metadados exibíveis restringem-se a marca, últimos dígitos e expiração quando fornecidos pelo provedor. Atualização ou substituição do cartão pelo provedor é observada por evento assinado e auditada; falta de saldo não atualiza estado nem substitui token. A abstração continua agnóstica a provedor: cartões exigem setup/tokenização e cobrança futura, enquanto meios futuros podem não ser tokenizáveis. Na V1, somente `CARD` está ativo.

Contrato mínimo de `BillingConnector`:

- validar configuração e retornar capacidades do conector/conta;
- criar ou localizar customer externo quando o provedor exigir;
- criar sessão hospedada/SDK de setup e transformar o resultado em `PaymentMethodBinding` tokenizado;
- criar cobrança a partir de `CollectionRequest`, com idempotency key fornecida pelo núcleo;
- normalizar respostas síncronas e webhooks para os mesmos estados/eventos internos;
- produzir erro tipado, retryability e IDs externos.

Na V1, as capacidades obrigatórias são `CARD`, `supports_setup_session`, `supports_vault`, `supports_off_session_charge` e `supports_webhook`; consulta automática por pagamento, polling e watchdog não são capacidades ativas. Capacidades de outros meios permanecem apenas no desenho extensível e não podem ser habilitadas. O núcleo valida a operação contra essas capacidades antes de chamar o adaptador; ausência de capacidade nunca é tratada como sucesso.

- `BillingConnection`: tipo de adaptador, customer/merchant externo, meios suportados e configuração de webhook; capacidades de refund podem existir apenas como informação de conciliação futura, não autorizam ação local na V1;
- `PaymentMethodBinding`: token/ID opaco do meio salvo no provedor, consentimento/mandato e estado;
- `CollectionRequest`: cobrança criada pelo motor de Subscription, única por assinatura + período/compra/transição/regularização, com `purpose` (`INITIAL_SUBSCRIPTION`, `RENEWAL`, `RENEWAL_REGULARIZATION`, `ON_DEMAND` ou `PLAN_UPGRADE`), valor/moeda do snapshot do Plan, snapshot de `unexpected_payment_policy`, `payment_expires_at` imutável e `terminal_reason` tipado quando encerrada. Revogação administrativa do CustomerPlan terminaliza qualquer solicitação pendente correlata em `CANCELED/CUSTOMER_PLAN_REVOKED`, impedindo nova tentativa ou efetivação tardia;
- `CollectionAttempt`: execução individual e imutável de uma `CollectionRequest`, numerada desde 1, com `scheduled_at`, início/fim, conector, meio, chave idempotente, resultado normalizado e próxima ação; uma nova tentativa nunca sobrescreve a anterior;
- `BillingPayment`: pagamento normalizado associado à solicitação e à tentativa que o originou, com estados `PENDING`, `REQUIRES_ACTION`, `CONFIRMED`, `FAILED` ou `CANCELED`;
- `WebhookInbox`: evento bruto imutável, ID externo único, instante recebido/processado e resultado;
- `ExternalSubscriptionBinding`: correlação técnica opcional; nunca torna a assinatura externa fonte de verdade.

`UnmatchedPaymentCase` representa pagamento confirmado que não corresponde de forma inequívoca a uma `CollectionRequest` pendente. Guarda provedor, pagamento, valor/moeda, motivo tipado, assinatura candidata opcional, evidências, estado `OPEN`, `RECONCILED` ou `CLOSED_WITH_JUSTIFICATION`, ator e histórico imutável. Na V1 ele nunca dispara devolução automática.

Suspeita de webhook perdido, divergência de valor/moeda/vínculo ou situação que o evento não permita decidir entra em investigação manual diretamente no painel do provedor. A V1 não cria polling, watchdog, cron de consulta, fila automática de divergências nem autoefetivação a partir dessa suspeita. Se a investigação justificar uma consequência interna excepcional, o operador usa a operação manual e auditável aplicável, como Compensation; isso não transforma a exceção em cobrança, confirmação ou refund local automático.

Quando um operador, por incidente, erro ou decisão comercial excepcional fora do produto, executa o estorno diretamente no provedor, Billing somente concilia o evento/resultado externo em um `ExternalRefundObservation` imutável, com IDs opacos do provedor, `BillingPayment`/`CollectionRequest` correlatos, valor, moeda, estado normalizado, instante e evidências. Isso não é uma solicitação de refund nem faz Billing disparar, repetir ou prometer devolução.

#### Política de estorno da V1

A V1 não oferece estorno como feature de produto: não há endpoint, tela, botão, solicitação pelo customer, regra automática ou política comercial de elegibilidade de refund. Cancelar uma Subscription não gera estorno automático. Em necessidade excepcional, o operador autorizado usa diretamente os controles do meio de pagamento/provedor (por exemplo, Stripe, PayPal ou outro) e o domínio local apenas recebe e concilia o fato externo. Billing não inicia, não duplica e não simula um refund.

Após a conciliação de um estorno externo, uma Compensation pode ser criada manualmente, se o operador concluir que há consequência interna de créditos. Ela deve usar tipo versionado, motivo explícito, referências tipadas à cobrança/`CollectionRequest` e ao identificador externo do refund quando existir, além de metadata canônica limitada e sem PII/dados de cartão. A Compensation é decisão administrativa independente: o estorno não a cria automaticamente. Ela pode creditar ou debitar unidades, sempre como lançamento imutável; nunca altera ou apaga saldo diretamente.

Voucher é benefício promocional de campanha; Cupom modifica condição/desconto de cobrança; Compensation é ajuste excepcional administrativo. Em `CREDIT_STRICT`, um débito compensatório deve respeitar as invariantes documentadas de saldo e estado. Se o caso não puder respeitá-las, ele permanece caso operacional explicitamente auditado, sem inventar pós-pago, saldo negativo ou fluxo automático.

Resposta síncrona e webhook alimentam a mesma máquina de estados, mas a confirmação definitiva da V1 é efetivada por webhook assinado. Mesmo uma confirmação imediata é normalizada como o mesmo evento interno. Eventos duplicados ou fora de ordem não podem repetir renovação ou concessão-base. Falha definitiva informada pelo provedor encerra a única tentativa comercial do ciclo; timeout, ausência ou resposta incerta preservam a solicitação e jamais criam nova cobrança, cancelamento ou consulta automática. Depois de validar a confirmação, Billing solicita a transação idempotente que efetiva o ciclo e sua franquia-base na `customer_wallet`.

#### 4.11.1 Orquestração durável de cobranças e retentativas

`CustomerPlan` é dono da intenção comercial: regra e âncora de recorrência, consequência da falha e `payment_completion_window`. Esta última é a janela de conclusão da solicitação de cobrança, não uma carência de inadimplência/renovação. Na V1, a política é fixa: uma única tentativa comercial exatamente na data de renovação, sem tentativa antecipada nem retentativa automática. Ao abrir um período ou uma compra sob demanda, esses valores são copiados para a `CollectionRequest`; alterações posteriores no CustomerPlan só afetam solicitações futuras.

Billing é dono da execução operacional. Um scheduler durável apenas encontra ações comerciais já agendadas, como a única tentativa na data de renovação calculada pela âncora UTC preservada ou a expiração comercial em `payment_expires_at`; ele materializa as viradas de ciclo com a regra calendária do CustomerPlan e adquire lease/lock antes de chamar o conector com uma chave idempotente estável quando a ação for a cobrança prevista. Ele não consulta o provedor, não faz polling/watchdog de webhook e não cancela por ausência de evento. Reinício do processo, execução concorrente do scheduler ou timeout externo não pode criar duas tentativas lógicas para o mesmo número nem duas cobranças efetivas. A unicidade mínima é `(collection_request_id, attempt_number)` e a chave enviada ao provedor deriva dessa identidade.

Cada `CollectionRequest` expõe o progresso completo: total permitido, tentativas realizadas, tentativas restantes, última falha normalizada, `next_action_at`, `payment_expires_at` e estado `SCHEDULED`, `COLLECTING`, `PENDING_PAYMENT`, `PAID`, `EXHAUSTED`, `EXPIRED`, `CANCELED` ou `UNMATCHED`. `PENDING_PAYMENT` cobre a espera por confirmação ou autenticação adicional antes de `payment_expires_at`. `EXPIRED` é o estado terminal canônico para o prazo comercial de conclusão vencido sem confirmação; “abortada” é apenas linguagem/motivo operacional, não um segundo estado sinônimo. A expiração não é watchdog técnico de webhook nem cancelamento da Subscription. Na V1, total permitido é um; resultado incerto preserva a mesma tentativa e aguarda webhook assinado, e falha definitiva não é repetida automaticamente.

Ao criar qualquer solicitação, CustomerPlan calcula e persiste `payment_expires_at`; Billing apenas o observa, não inventa nem estende o prazo. Até esse instante a solicitação pode ficar `PENDING_PAYMENT`, mas não efetiva novo entitlement, ciclo ou crédito. Após o prazo sem confirmação definitiva, uma transição atômica e idempotente marca `EXPIRED` e não ativa plano/ciclo nem concede crédito. A transição disputa o mesmo lock/compare-and-set da confirmação de webhook: confirmação válida antes do vencimento pode efetivar uma única vez; expiração já efetivada faz evento posterior convergir sem renovar ou creditar.

Enquanto a mesma intenção do cliente estiver pendente e dentro da janela, cliques repetidos localizam e devolvem a mesma `CollectionRequest` e a mesma intenção externa, sem duplicar cobrança. Depois de `EXPIRED`, uma nova intenção explícita cria nova `CollectionRequest`; a expirada nunca é reaberta nem reutilizada como se fosse nova. Expirar solicitação `ON_DEMAND` encerra somente a solicitação e não altera o vínculo existente. Expirar renovação encerra a solicitação e aplica a regra comercial própria: `PAST_DUE`/`RENEWAL_INACTIVE`, sem cancelar a Subscription, revogar saldo ou criar nova cobrança.

Quando a única tentativa comercial de renovação termina em falha definitiva reportada pelo provedor, ou sua `CollectionRequest` expira, Billing marca a cobrança como `EXHAUSTED`/`EXPIRED`, publica o fato e o CustomerPlan transita idempotentemente para `PAST_DUE` com `renewal_status = RENEWAL_INACTIVE`. Não cria novo ciclo, nova cobrança automática nem franquia-base. Essa condição não é revogação: Product habilitado e crédito já disponível continuam utilizáveis sob `CREDIT_STRICT`; Billing não decide débito de Wallet. Ausência de webhook não esgota tentativa nem cancela jobs. Evento válido tardio continua a ser processado pela máquina de estados; se o estado comercial já for terminal incompatível, ele é auditado/convergido sem renovar automaticamente outro período.

#### 4.11.2 Eventos de Billing e fronteira com Notifications

Billing mantém uma outbox transacional e publica eventos versionados depois de persistir a mudança de estado. O conjunto mínimo inclui `collection.created`, `collection.attempt_scheduled`, `collection.attempt_started`, `collection.awaiting_customer`, `collection.attempt_failed`, `collection.retry_scheduled`, `collection.exhausted`, `collection.expired`, `payment.confirmed`, `payment.unmatched` e `external_refund.observed`. Cada evento contém IDs correlatos, workspace/customer, assinatura e período quando aplicáveis, estado, motivo tipado e instante.

Regra normativa de cobertura: toda ação financeira ou integração externa observável do Billing produz pelo menos um evento de intenção antes do efeito externo e exatamente um resultado terminal correspondente depois, sem exigir eventos para cada passo puramente interno. Pares mínimos incluem cobrança `attempt.requested` → `attempt.succeeded|failed|pending`, processamento de webhook `webhook.received` → `webhook.applied|unmatched|rejected`, concessão `credit_grant.requested` → `credit_grant.applied|failed`, e estorno externo já realizado `external_refund.observed` → `external_refund.reconciled|inconclusive`. Estados intermediários podem gerar eventos adicionais, mas nunca substituem o resultado terminal.

Todos os eventos usam um envelope comum com `billing_event_id`, `event_type`, `schema_version`, `occurred_at`, `workspace_id`, `correlation_id`, `causation_id`, identificadores da entidade/agregado, `attempt_number` quando aplicável e payload tipado. `correlation_id` acompanha toda a jornada da cobrança; `causation_id` aponta para o evento/comando que provocou o próximo passo. A unicidade de `billing_event_id` e a outbox transacional tornam a publicação ao menos uma vez, portanto consumidores devem deduplicar. A ordem é garantida somente dentro do mesmo agregado; consumidores não devem presumir ordem global.

O catálogo completo, os destinos e o transporte dos eventos — broker, webhook de saída, API de polling ou combinação — serão detalhados em contrato próprio. Esta pendência não altera a obrigação atual de registrar eventos duráveis, versionados e correlacionáveis em cada fronteira de ação do Billing.

`collection.attempt_scheduled` é o fato apropriado para avisar antecipadamente que uma cobrança será tentada, com `attempt_number` e `scheduled_at`; ele pode ser emitido na criação da tentativa ou nos offsets de aviso declarados pela Subscription. `collection.attempt_started` significa que Billing efetivamente iniciou a chamada ao provedor e não deve ser usado como promessa antecipada. A política pode declarar `pre_charge_notice_offsets`, mas Notifications decide canal e conteúdo.

Após iniciar uma tentativa, Billing classifica a resposta em três grupos. Uma confirmação terminal só efetiva efeitos após o webhook assinado correspondente. Na V1, uma falha terminal, como recusa confirmada pelo provedor, persiste o motivo tipado e encerra o ciclo sem nova tentativa. Um resultado `PENDING` ou `REQUIRES_ACTION` não é tratado como falha apenas porque o pagamento ainda não apareceu: Billing aguarda webhook até o prazo comercial, sem consulta automática. O scheduler só executa ações comerciais persistidas; ele não infere ausência de pagamento pela passagem do cron nem atua como watchdog técnico.

Billing decide que um fato financeiro ocorreu e que ele deve ser comunicado; não envia e-mail, WhatsApp ou mensagem de canal diretamente. Um domínio separado de Notifications consome os eventos, escolhe template, destinatário, canal e provedor, controla preferências e entrega. A aplicação também pode registrar webhooks de saída para receber os mesmos fatos. Falha de notificação não reabre cobrança, não repete pagamento e não altera conciliação; o consumidor possui idempotência própria pelo `billing_event_id`.

### 4.12 Voucher

Vale é um ativo promocional independente:

- `voucher_id`;
- `name`, `description`;
- `credit_units`, inteiro estritamente positivo e persistido no próprio vale;
- estado `AVAILABLE`, `USED` ou `VOID`;
- `coupon_id` opcional;
- ao usar: workspace beneficiado, instante, forma de resgate e entrada do razão.

Invariantes:

- um vale só transita de `AVAILABLE` para `USED` ou `VOID`;
- `USED` e `VOID` são terminais;
- o resgate usa `credit_units` persistido, nunca uma quantidade da requisição;
- marcar usado, criar entrada e atualizar a wallet acontecem na mesma transação;
- vale vinculado a cupom só pode ser consumido pelo fluxo desse cupom;
- não existe reserva persistente intermediária na v1.

### 4.13 Coupon e CouponUserRedemption

Cupom é um recurso público separado:

- `coupon_id` interno;
- `code` customizado, globalmente único após normalização;
- modo `OPEN` ou `ONCE_PER_EXTERNAL_USER`;
- estado administrativo `ACTIVE`, `DISABLED` ou `ARCHIVED`;
- um Vale específico ou lote fechado de Vales pré-criados;
- validade opcional `valid_from` e `valid_until`.

Na v1, todos os vales ligados ao mesmo cupom devem ter a mesma quantidade de `credit_units`. Isso torna o benefício previsível; a quantidade continua pertencendo a cada vale, e não ao cupom. O estado público `EXHAUSTED` é derivado quando não existe vale `AVAILABLE`, evitando duas fontes de verdade.

O código aceita apenas ASCII em caixa alta, dígitos, hífen e sublinhado. A API remove espaços externos e converte letras ASCII para caixa alta antes de validar e persistir; a unicidade usa esse valor normalizado. O código original não é uma segunda identidade.

No modo `ONCE_PER_EXTERNAL_USER`, a requisição exige `unique_user_id`. A string é opaca: comparação exata de bytes UTF-8, sem trim, mudança de caixa ou normalização Unicode. A restrição única representa `(coupon_id, unique_user_id)`. A plataforma não cria perfil, não valida documento e não atribui significado ao valor.

`CouponUserRedemption` referencia cupom, digest do identificador, vale consumido, workspace e entrada do razão. Sua inserção, seleção do vale e crédito são atômicos.

### 4.14 Compensation

`Compensation` é um recurso administrativo rastreável que registra uma intenção de correção antes de qualquer movimentação. Ele não representa consumo e sua criação não altera a Wallet.

`CompensationType` é o catálogo versionado de motivos permitidos no workspace:

- `compensation_type_id`, código estável, nome e descrição;
- sinais permitidos: crédito, débito ou ambos; zero é proibido;
- limites opcionais por ocorrência e regras de evidência;
- `approval_required` opcional como regra de workflow;
- estado administrativo e versão; o tipo não fixa obrigatoriamente o valor da ocorrência.

- `compensation_id`, `workspace_id`, `customer_id` e `customer_wallet_id`;
- `compensation_type_id` e versão aplicada;
- `signed_credit_units`, inteiro diferente de zero, positivo ou negativo;
- descrição/justificativa obrigatória e imutável depois da submissão;
- referências tipadas à transação original, Vale, Plano, pagamento, `CollectionRequest`, incidente ou outra origem quando aplicável; para consequência de estorno externo, referência ao identificador do refund/provedor quando disponível;
- identidade do solicitante, aprovador quando exigido, executor, observação opcional de execução e timestamps;
- `compensation_batch_id` opcional;
- estados `DRAFT`, `PENDING_APPROVAL`, `READY`, `EXECUTED`, `CANCELED` ou `REJECTED`.

Criar, editar enquanto `DRAFT`, submeter, aprovar quando aplicável e executar são ações separadas e auditadas. Na submissão, um tipo sem aprovação obrigatória transita diretamente para `READY`; um tipo que exige aprovação transita para `PENDING_APPROVAL` e somente uma ação explícita de aprovação o leva a `READY`. Apenas `READY` pode ser executada. A execução exige `transaction_id` e `Idempotency-Key`, cria exatamente uma entrada `COMPENSATION` na `customer_wallet` com referência oficial à Compensation e muda seu estado para `EXECUTED` no mesmo commit. Nova execução ou reutilização de qualquer das chaves retorna `409`; alteração posterior do valor ou cancelamento depois de executada também são rejeitados.

Compensação pode creditar ou debitar, mas nunca aceita delta zero, nunca edita/apaga lançamento anterior e não pode ser usada como atalho para consumo, pagamento de Plano ou resgate de Vale. Não é criada automaticamente por refund: um operador avalia o fato externo e decide se cria a ocorrência. Em `CREDIT_STRICT`, um débito compensatório somente executa quando respeita as invariantes de saldo/estado; caso contrário, permanece pendente como exceção operacional auditada, sem converter o domínio em pós-pago. `approval_required` controla apenas a passagem de estado do workflow.

`CompensationBatch` é um agrupador operacional opcional para ações em massa. Guarda descrição/motivo comum, origem/importação, criador, contagens e estado, mas não movimenta saldo. Uma operação para cem customers materializa cem Compensations filhas, cada uma ligada a exatamente um customer, com valor, estado, execução e transação próprios. Falha de uma filha não desfaz as já executadas; retry atua apenas nas pendentes e nunca recria uma ocorrência executada.

## 5. Contratos HTTP principais

Prefixo sugerido: `/v1`. O `workspace_id` vem no caminho ou no contrato da operação, e a aplicação valida sua existência e a consistência do escopo.

Toda operação externa que possa gerar `CustomerWalletEntry` exige `transaction_id`, cabeçalho `Idempotency-Key`, `description` opcional e `metadata` opcional. O servidor copia os campos de domínio para o lançamento e registra separadamente a chave de deduplicação. `metadata` é contexto opaco da aplicação cliente, não substitui referências tipadas como assinatura, plano, Product, Item, Vale, Cupom ou Billing. Campos reservados pelo servidor não podem ser sobrescritos pelo cliente.

### 5.1 Consulta e configuração

- `GET /v1/workspaces/{workspace_id}/wallets` — hierarquia materializada, customer wallet, item wallets, estados e versão do escopo.
- `GET /v1/workspaces/{workspace_id}/wallet-provisioning` — conjunto esperado/materializado e prontidão.
- `POST /v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile` — reprocessamento idempotente e restrito da materialização; não cria wallets dentro de uma chamada de consumo.
- `GET /v1/workspaces/{workspace_id}/customer-wallet` — `balance_credit_units`, versão, timestamp e ID da última `CustomerWalletEntry`; não retorna moeda.
- `GET /v1/workspaces/{workspace_id}/customer-wallet/statement?cursor=...` — extrato paginado, com filtros por data, tipo, origem, `transaction_id`, `reference_kind` e `reference_id`; retorna descrição, metadata e referências tipadas.
- `GET /v1/workspaces/{workspace_id}/customer-wallet/statement/{customer_wallet_entry_id}` — detalhe do lançamento de créditos e correlações.
- `GET /v1/workspaces/{workspace_id}/customer-wallet/transactions/{transaction_id}` — localiza a operação no workspace informado e retorna lançamento, descrição, metadata canônica e referências tipadas; para consumo ainda pendente, retorna o `UsageEvent`/`ItemWalletEntry` mesmo sem lançamento global.
- `GET /v1/workspaces/{workspace_id}/billing-config` — flags de crédito.
- `PUT /v1/workspaces/{workspace_id}/billing-config` — alteração administrativa com versão esperada.
- `GET /v1/workspaces/{workspace_id}/products/{product_id}/eligibility` — consulta não vinculante que retorna `usage_model`, estado comercial, `renewal_status`, entitlement efetivo e, somente para `CREDIT_METERED`, suficiência de créditos; `RENEWAL_INACTIVE` não bloqueia por si só o uso de saldo existente.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet` — medidor atual de item units, vínculo à customer wallet e próximo bloco.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement?cursor=...` — extrato completo de consumo do item, incluindo chamadas que não emitiram débito.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement/{item_wallet_entry_id}` — detalhe e correlação opcional com o débito na customer wallet.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/pricing-accumulators?price_version_id=...&cursor=...` — acumuladores vitalícios ou por ciclo.

#### Decisão de elegibilidade de uso

A elegibilidade de cada consumo `CREDIT_STRICT` avalia quatro conceitos distintos, nesta ordem prática: (1) **vínculo utilizável** — existe um `CustomerPlan` que não está pendente, encerrado, cancelado ou revogado; `ACTIVE`, `ACTIVE_PAID` e `PAST_DUE` são vínculos utilizáveis para saldo já disponível; (2) **entitlement de produto** — a versão de plano efetivada habilita o Product/Item solicitado; (3) **condição de renovação** — `CURRENT` ou `RENEWAL_INACTIVE`, exposta separadamente e sem decidir sozinha o uso; (4) **disponibilidade de crédito** — a `CustomerWallet` e a `ItemWallet` materializada permitem o débito calculado sem tornar o saldo negativo. O resultado principal é `customer_plan_not_active`, `entitlement_not_granted`, `insufficient_credit` ou `eligible`. `renewal_inactive` é condição/reason secundário informativo, não bloqueio de consumo quando os três demais requisitos permitem.

`insufficient_credit` bloqueia somente o consumo cujo débito não cabe no saldo. Ele não cancela, encerra, rebaixa nem altera o status do CustomerPlan: é resultado derivado da Wallet, nunca estado persistido de CustomerPlan. Analogamente, `RENEWAL_INACTIVE` impede somente novo ciclo, cobrança automática e franquia nova; não revoga créditos já disponíveis, qualquer que seja sua origem/lote compatível. Assim, um CustomerPlan `PAST_DUE` com Product habilitado e saldo suficiente pode consumir em `CREDIT_STRICT`; se o saldo não basta, o resultado é `insufficient_credit`, não `customer_plan_not_active`. O customer pode regularizar manualmente cartão/cobrança pela UI segura do provedor e pelos fluxos já definidos, sem retentativa automática ou carência automática. A compra `OnDemandPlan` é uma decisão separada: exige CustomerPlan em condição comercial `ACTIVE`, `activation_status = ACTIVATED` e mesma Subscription comercial tanto na criação quanto na confirmação. Enquanto a ativação inicial estiver pendente, retorna `409 customer_plan_activation_pending` e não cria/efetiva `CollectionRequest`; `PAST_DUE`/`RENEWAL_INACTIVE` e estados terminais também não podem iniciar nem efetivar compra, mesmo com saldo existente. Ela não é condição para usar outro crédito já disponível.

Resumo da hierarquia para um cluster com 10 Items faturáveis:

```json
{
  "customer_id": "ws_...",
  "scope_version": "cluster-items-v42",
  "provisioning_status": "ACTIVE",
  "expected_item_wallets": "10",
  "materialized_item_wallets": "10",
  "customer_wallet": {
    "wallet_id": "wallet_customer_...",
    "wallet_type": "CUSTOMER",
    "balance_credit_units": "250"
  },
  "item_wallets": [
    {
      "wallet_id": "wallet_item_1",
      "wallet_type": "ITEM",
      "parent_customer_wallet_id": "wallet_customer_...",
      "item_id": "item_1",
      "status": "ACTIVE"
    }
  ],
  "item_wallets_returned": "10"
}
```

Resposta de elegibilidade:

```json
{
  "product_id": "prod_...",
  "usage_model": "CREDIT_METERED",
  "eligible": true,
  "reason": "eligible",
  "customer_plan": {
    "commercial_status": "PAST_DUE",
    "renewal_status": "RENEWAL_INACTIVE",
    "renewal_reason": "renewal_inactive",
    "subscription_model": "CREDIT_STRICT",
    "entitled": true
  },
  "wallet": {
    "wallet_type": "customer_wallet",
    "balance_credit_units": "250",
    "version": "42"
  },
  "evaluated_at": "2026-08-25T20:00:00Z"
}
```

Razões normativas principais: `customer_plan_not_active`, `entitlement_not_granted`, `insufficient_credit` ou `eligible`. `renewal_inactive` é razão secundária de renovação, presente com `PAST_DUE`, e pode coexistir com `eligible`; ela informa que não haverá ciclo/franquia/cobrança automática nova, não que o saldo existente foi bloqueado. Em `CREDIT_STRICT`, saldo zero só é elegível quando o débito calculado para o consumo também for zero; um consumo positivo que não caiba retorna `insufficient_credit`. Em `ENTITLEMENT_ONLY`, futuro/fora da V1, `wallet` e `credit_sufficient` são nulos/omitidos. A resposta é informativa e pode mudar antes do consumo.

Cada item do extrato da `customer_wallet` expõe, no mínimo: sequência estável, `customer_wallet_entry_id`, instante, tipo, canal de origem, `signed_credit_units`, `balance_before_credit_units`, `balance_after_credit_units`, `transaction_id` e referências à `item_wallet` quando a origem for consumo. Não há campos de item balance, moeda ou dinheiro.

Cada item do extrato da `item_wallet` expõe `item_wallet_entry_id`, item units recebidas, totais antes/depois, item units convertidas e pendentes, blocos gerados e `customer_wallet_entry_id` opcional. Ele registra inclusive consumo que ainda não debitou wallet credits; nesse caso não existe lançamento global. Quando há conversão, os dois extratos permanecem separados e são correlacionáveis por `transaction_id`, `usage_event_id`, `billing_block_id` e IDs recíprocos das entradas. Ambos usam cursor monotônico próprio; paginação por offset não é adequada.

Resposta do medidor de uso:

```json
{
  "customer_id": "ws_...",
  "product_id": "prod_...",
  "item_id": "item_...",
  "item_wallet_id": "wallet_item_...",
  "parent_customer_wallet_id": "wallet_customer_...",
  "total_received_item_units": "1012",
  "total_converted_item_units": "1000",
  "total_converted_blocks": "1",
  "pending_item_units": "12",
  "pending_price_version_id": "price_...",
  "pending_unit_block_size": "1000",
  "units_until_next_block": "988",
  "pricing_accumulators": [
    {
      "price_version_id": "price_...",
      "cycle_key": "2026-08-01T00:00:00Z",
      "cycle_start": "2026-08-01T00:00:00Z",
      "cycle_end": "2026-09-01T00:00:00Z",
      "accumulated_converted_item_units": "1000",
      "converted_blocks": "1",
      "current_tier_from_accumulated_units": "0",
      "next_tier_at_accumulated_units": "100000"
    }
  ],
  "version": "1",
  "updated_at": "2026-08-25T20:00:00Z"
}
```

Essa consulta mostra o estado atual e não substitui o extrato da `item_wallet` nem o recibo histórico. Uma repetição da chave retorna `409`; o cliente consulta a transação/evento original pelo identificador ou `Location` informado no conflito.

### 5.2 Catálogo administrativo

- `POST /v1/products`, `GET /v1/products/{id}`, `PATCH /v1/products/{id}` — `usage_model` não muda após publicação;
- `POST /v1/products/{product_id}/items`, `GET /v1/items/{id}`, `PATCH /v1/items/{id}`;
- `POST /v1/items/{item_id}/price-versions` — cria rascunho;
- `GET /v1/price-versions/{id}` — retorna modelo, bloco, faixas e regra declarativa imutável;
- `POST /v1/price-versions/{id}/publish` — valida faixas e agenda/ativa a versão.

Preço publicado não é editado. Correções criam nova versão com vigência posterior. Operações de catálogo preservam controle de concorrência.

Exemplo de preço `unit`:

```json
{
  "pricing_model": "unit",
  "unit_block_size": "1000",
  "credit_units": "100",
  "effective_from": "2026-09-01T00:00:00Z"
}
```

Exemplo `tiered` sem ciclo, portanto acumulado vitalício:

```json
{
  "pricing_model": "tiered",
  "accumulation_cycle": null,
  "tiers": [
    { "from_accumulated_units": "0", "to_accumulated_units": "10", "unit_block_size": "1", "credit_units": "1" },
    { "from_accumulated_units": "10", "to_accumulated_units": "20", "unit_block_size": "2", "credit_units": "1" },
    { "from_accumulated_units": "20", "to_accumulated_units": null, "unit_block_size": "10", "credit_units": "3" }
  ],
  "effective_from": "2026-09-01T00:00:00Z"
}
```

Exemplo `tiered` com ciclo declarativo trimestral, sem enum específico na API:

```json
{
  "pricing_model": "tiered",
  "accumulation_cycle": {
    "anchor_at": "2026-01-01T00:00:00Z",
    "recurrence_rule": "FREQ=MONTHLY;INTERVAL=3"
  },
  "tiers": [
    { "from_accumulated_units": "0", "to_accumulated_units": "100000", "unit_block_size": "1000", "credit_units": "10" },
    { "from_accumulated_units": "100000", "to_accumulated_units": null, "unit_block_size": "2000", "credit_units": "7" }
  ],
  "effective_from": "2026-09-01T00:00:00Z"
}
```

Na publicação, `pricing_model` determina campos permitidos, cada faixa deve declarar sua razão e toda faixa finita deve ter amplitude divisível pelo próprio `unit_block_size`; `anchor_at` deve ser anterior ou igual a `effective_from`, e a RRULE precisa produzir uma próxima fronteira válida para qualquer ciclo alcançável. Configuração inválida retorna `422 invalid_price` ou `422 invalid_accumulation_cycle` antes de o preço ser ativado.

### 5.3 Registro de consumo e eventual débito

`POST /v1/workspaces/{workspace_id}/usage-events`

`GET /v1/workspaces/{workspace_id}/usage-events/{usage_event_id}` devolve o recibo histórico persistido, sem recalcular o estado atual.

Requisição:

```json
{
  "transaction_id": "usage-2026-08-25-000123",
  "product_id": "prod_...",
  "item_id": "item_...",
  "item_units": "1012",
  "expected_price_version_id": "price_...",
  "occurred_at": "2026-08-25T19:59:58Z",
  "metadata": { "source_event": "evt_..." }
}
```

Na V1, toda chamada válida que não leve a saldo negativo cria `UsageEvent` e `ItemWalletEntry`. Se não completar bloco, retorna `201 Created` sem `Debit` e sem `CustomerWalletEntry`; se completar bloco e `balance_after_credit_units` for zero ou positivo, cria o débito global e retorna `201`. Se o débito deixaria saldo negativo, retorna `409 insufficient_credit` sem persistir nenhum efeito da chamada. O `402` é reservado à futura capacidade `CREDIT_FLEXIBLE` e não é emitido na V1.

Exemplo para bloco de 1.000 requests a 100 créditos, recebendo 1.012 com pendente inicial zero:

```json
{
  "usage_event_id": "usage_...",
  "item_wallet_id": "wallet_item_...",
  "item_wallet_entry_id": "item_entry_...",
  "debit_id": "debit_...",
  "customer_wallet_entry_id": "customer_entry_...",
  "transaction_id": "usage-2026-08-25-000123",
  "received_item_units": "1012",
  "pending_item_units_before": "0",
  "converted_item_units": "1000",
  "converted_blocks": "1",
  "pending_item_units_after": "12",
  "allocations": [
    {
      "price_version_id": "price_...",
      "pricing_model": "unit",
      "unit_block_size": "1000",
      "cycle_key": "lifetime",
      "accumulated_units_before": "0",
      "accumulated_units_after": "1000",
      "converted_blocks": "1",
      "debited_credit_units": "100"
    }
  ],
  "debited_credit_units": "100",
  "balance_before_credit_units": "500",
  "balance_after_credit_units": "400",
  "credit_status": "SUFFICIENT",
  "product_eligible_after": true,
  "created_at": "2026-08-25T20:00:00Z"
}
```

Quando nenhum bloco é completado, `item_wallet_entry_id` continua obrigatório, enquanto `customer_wallet_entry_id` e `debit_id` são `null`; `converted_item_units`, `converted_blocks` e `debited_credit_units` são zero e `billing_status` é `PENDING_BLOCK`. O uso pendente aparece somente no extrato da item wallet até que uma conversão gere débito real.

Reutilizar a mesma `Idempotency-Key` retorna `409 idempotency_key_already_used`; reutilizar o mesmo `transaction_id` com nova chave retorna `409 transaction_already_exists`. Ambos deixam as wallets intactas e podem informar referência segura ao `UsageEvent`/lançamento original para consulta, sem devolver novamente o corpo de sucesso.

### 5.4 Créditos

- `POST /v1/workspaces/{workspace_id}/credits/direct` — exige modo direto habilitado, `transaction_id`, `credit_units` positivo e `external_reference` opcional.
- `POST /v1/workspaces/{workspace_id}/customer-plans/{id}/on-demand-purchases` — compra adicional dentro de um CustomerPlan já comercialmente `ACTIVE` e com `activation_status = ACTIVATED`, não uma adesão. Exige versão não revogada e `OnDemandPlan` publicado/não revogado da mesma Subscription comercial; ativação pendente retorna `409 customer_plan_activation_pending` sem criar cobrança. A operação cria cobrança idempotente vinculada ao CustomerPlan e só credita a Wallet se todas essas condições ainda valerem na confirmação, sem criar CustomerPlan, ciclo, renovação, entitlement ou alterar recorrência/plano principal. `PAST_DUE`/`RENEWAL_INACTIVE`, estados terminais, versão revogada ou Subscription comercial diferente são rejeitados mesmo com saldo.
- `POST /v1/workspaces/{workspace_id}/customer-plans/{id}/renewal-regularizations` — disponível para `PAST_DUE`/`RENEWAL_INACTIVE`; cria/retorna nova `CollectionRequest` idempotente de regularização, nunca reutiliza a tentativa terminal anterior e só reativa após webhook confirmado.
- `POST /v1/workspaces/{workspace_id}/customer-plans/{id}/plan-transitions` — solicita `UPGRADE` ou `DOWNGRADE` para nova `SubscriptionPlanVersion` da mesma Subscription comercial; upgrade cria/retorna a única `CollectionRequest` pendente, downgrade aplica a transição imediata auditável e ambos reiniciam a âncora somente quando efetivados.
- `POST /v1/subscriptions` e `POST /v1/subscriptions/{id}/plans` — catálogo administrativo de Subscription comercial, suas versões de plano de assinatura e seus OnDemandPlans. Criar a Subscription raiz a torna disponível imediatamente, sem endpoint ou estado posterior de publicação/ativação/pausa/arquivamento/revogação; versões publicadas de plano continuam não editáveis. Caso exista caminho histórico `/subscription-offerings`, ele é apenas alias de catálogo e não muda o domínio canônico.
- `POST /v1/subscription-plans/{id}/revoke` — revoga administrativamente a versão, com razão auditável; bloqueia adesões, transições e renovações futuras, terminaliza solicitações pendentes sem efetivação e não revoga imediatamente os CustomerPlans já confirmados.
- `GET /v1/subscriptions/{id}` e `GET /v1/subscription-plans/{id}` — catálogo comercial: a Subscription expõe seus `SubscriptionPlanVersion` e `OnDemandPlan` publicados; o plano de assinatura expõe a composição `FREE|PAID`, preço quando `PAID`, recorrência enumerada, política de admissão, meios aceitos e `granted_credit_units`, sem Voucher, Cupom ou duração/expiração temporal.
- `POST /v1/workspaces/{workspace_id}/customer-plans` — única rota canônica de adesão: recebe `SubscriptionPlanVersion`, avalia a política de admissão e cria um CustomerPlan principal `CREDIT_STRICT`, depois de reservar `ActiveCustomerPlanSlot` para `(customer_id, subscription_id)` comercial derivado. `OnDemandPlan` não é aceito nessa rota nem pode criar CustomerPlan. `FREE` livre entra diretamente em `ACTIVATED`; `FREE` que exige `CARD` fica em `PENDING_CARD_VALIDATION` até webhook de setup válido, sem cobrança; e `PAID` fica em `PENDING_INITIAL_PAYMENT` até confirmação definitiva do pagamento inicial, inclusive após autenticação adicional. Os dois estados pendentes não efetivam entitlement, ciclo, franquia-base ou OnDemand. Se já existir CustomerPlan ativo na mesma Subscription comercial, ou se uma criação concorrente vencer a reserva, retorna `409 active_customer_plan_already_exists` sem nova cobrança, ciclo, entitlement ou crédito; Subscriptions comerciais distintas não conflitam.
- `GET /v1/workspaces/{workspace_id}/customer-plans/{id}`. Caso uma API externa mantenha temporariamente o caminho histórico `/subscriptions`, ele é apenas alias de recurso de CustomerPlan e não muda a terminologia nem o modelo canônico;
- `POST /v1/workspaces/{workspace_id}/customer-plans/{id}/cancel` — para recorrente confirmada, agenda `cancel_at_period_end`; em `PENDING_PAYMENT`, encerra somente a solicitação pendente. Não encerra o período confirmado nem dispara estorno/Compensation;
- `POST /v1/admin/workspaces/{workspace_id}/customer-plans/{id}/revoke` — ação administrativa prioritária, com motivo, ator, instante e referências auditáveis. Serializa a transição para `REVOKED` e terminaliza em `CANCELED/CUSTOMER_PLAN_REVOKED` toda `CollectionRequest` pendente do vínculo; não dispara estorno/Compensation e não permite reativação;
- `POST /v1/workspaces/{workspace_id}/billing-connections` e `GET /v1/workspaces/{workspace_id}/billing-connections/{id}` — configura adaptador e expõe endpoint de webhook;
- `GET /v1/workspaces/{workspace_id}/billing-connections/{id}/capabilities` — na V1 expõe somente capacidades de cartão;
- `POST /v1/workspaces/{workspace_id}/billing-connections/{id}/payment-method-setup-sessions` — inicia tokenização, hosted fields ou checkout do provedor e devolve somente artefatos públicos necessários ao cliente;
- `POST /v1/workspaces/{workspace_id}/payment-method-bindings` e `GET /v1/workspaces/{workspace_id}/payment-method-bindings` — conclui/lista vínculos entre customer, conexão e, opcionalmente, CustomerPlan;
- `POST /v1/workspaces/{workspace_id}/customer-plans/{id}/collection-requests` — cria/retorna solicitação idempotente; normalmente responde `202`;
- `GET /v1/workspaces/{workspace_id}/collection-requests/{id}` — estado normalizado da única tentativa comercial, ação adicional de cartão quando aplicável e resultado do processamento de eventos/webhooks;
- `POST /v1/billing/webhooks/{connection_id}` — entrada de eventos por conexão;
- `GET /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments?cursor=...` — fila operacional com motivo, evidências e assinatura candidata;
- `POST /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments/{id}/reconcile` — vincula a uma `CollectionRequest` válida após revalidação completa;
- `POST /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments/{id}/close` — encerra sem devolução com justificativa auditável;
- `POST /v1/vouchers/{voucher_id}/redeem` — fluxo promocional interno, com workspace beneficiado e `transaction_id`;
- `POST /v1/coupons/redemptions` — aceita `coupon_id` ou `code`, workspace beneficiado, `transaction_id` e, no modo restrito, `unique_user_id`;
- `POST /v1/admin/workspaces/{workspace_id}/compensations` — cria a intenção sem movimentar saldo;
- `POST /v1/admin/workspaces/{workspace_id}/compensation-types` e `GET /v1/admin/workspaces/{workspace_id}/compensation-types` — administra/consulta o catálogo versionado de tipos e suas políticas;
- `PATCH /v1/admin/workspaces/{workspace_id}/compensations/{id}` — altera somente uma Compensation em `DRAFT`;
- `POST /v1/admin/workspaces/{workspace_id}/compensations/{id}/submit` — valida tipo, sinal, limites e descrição; transita para `READY` ou `PENDING_APPROVAL` conforme a política;
- `POST /v1/admin/workspaces/{workspace_id}/compensations/{id}/approve` e `/reject` — disponíveis somente quando a ocorrência exige aprovação;
- `POST /v1/admin/workspaces/{workspace_id}/compensations/{id}/execute` — exige `transaction_id` e executa atomicamente o único lançamento;
- `GET /v1/admin/workspaces/{workspace_id}/compensations/{id}` e `GET /v1/admin/workspaces/{workspace_id}/compensations?cursor=...` — detalhe e fila operacional.
- `POST /v1/admin/workspaces/{workspace_id}/compensation-batches` e `GET /v1/admin/workspaces/{workspace_id}/compensation-batches/{id}` — cria/consulta agrupador em massa e suas ocorrências individuais; execução continua sendo idempotente por Compensation.

Contratos de quantidade:

- concessão direta recebe `transaction_id`, `credit_units` estritamente positivo e `external_reference` opcional;
- `SubscriptionPlanVersion` define a composição comercial (modelo `FREE|PAID`, preço quando pago, recorrência, admissão, meios e `granted_credit_units` não negativo); CustomerPlan materializa essa composição e seus ciclos;
- confirmação do Billing valida pagamento/período por referências idempotentes e efetiva entitlement mais a única concessão-base positiva daquele ciclo;
- criação administrativa de vale recebe `credit_units` estritamente positivo persistido no vale;
- resgate de vale/cupom não recebe quantidade: usa `credit_units` do vale selecionado;
- Compensation recebe `signed_credit_units` diferente de zero, tipo, descrição obrigatória, referências e ator; aprovação é exigida apenas pela política aplicada. Somente o endpoint de execução recebe `transaction_id` e movimenta a Wallet.

Contratos de Wallet, consumo, vale, cupom e Compensation não aceitam moeda nem preço comercial. Apenas o catálogo `SubscriptionPlanVersion` armazena preço monetário, recorrência e `granted_credit_units`, separados da Wallet. A franquia-base vem exclusivamente do ciclo materializado e da versão de plano; compras diretas, Voucher/Cupom e Compensation continuam fontes independentes com suas próprias regras.

Enviar simultaneamente `coupon_id` e `code` é `422`. Código inexistente, desabilitado ou fora da validade não revela estoque além do necessário. Cupom sem vales retorna `409 coupon_exhausted`. Segundo usuário externo igual no mesmo cupom retorna `409 coupon_already_redeemed_by_user`. Reutilização de `Idempotency-Key` ou `transaction_id` retorna o conflito correspondente antes de qualquer novo resgate.

### 5.5 Erros

Erros que não representam um débito aceito usam `application/problem+json`, com `type`, `title`, `status`, `code`, `detail`, `request_id` e campos inválidos quando pertinente.

Mapeamento mínimo:

- `400`: JSON ou sintaxe inválida;
- `403 product_not_entitled`: o plano/entitlement não habilita o Product;
- `404`: recurso inexistente no workspace informado;
- `409 idempotency_key_already_used`: `Idempotency-Key` já utilizada, com payload igual ou diferente;
- `409 transaction_already_exists`: `transaction_id` já existente, com payload igual ou diferente;
- `409 price_version_changed`: versão esperada divergente;
- `409 coupon_exhausted` ou segundo resgate por usuário;
- `409 feature_disabled`: crédito direto ou recorrência desabilitado;
- `409 customer_plan_activation_pending`: condições obrigatórias da ativação inicial ainda não foram confirmadas; OnDemand não cria `CollectionRequest` nem crédito;
- `409 customer_plan_revoked`: CustomerPlan foi revogado administrativamente; nenhuma transição comercial, `CollectionRequest` nova ou efetivação tardia é permitida;
- `422`: quantidade, `credit_units`, faixas, período ou transição de estado inválidos;
- `503 wallet_not_provisioned`: hierarquia materializada ausente/incompleta; nenhuma wallet ou entrada é criada pelo endpoint que detectou a falha;
- `409 insufficient_credit`: o débito necessário excederia o saldo disponível; não há `UsageEvent`, pendente, bloco, lançamento ou alteração de saldo.

`402` não é emitido na V1; ele permanece reservado à futura capacidade `CREDIT_FLEXIBLE`.

`POST /usage-events` é permitido apenas para Product `CREDIT_METERED`. Product `ENTITLEMENT_ONLY` usa somente a consulta de elegibilidade; tentativa de registrar consumo retorna `409 product_not_metered` sem criar evento ou Wallet.

## 6. Fluxos transacionais

### 6.1 Registro de consumo e débito por bloco

1. Validar a forma do `workspace_id` e a existência do workspace.
2. Validar forma básica sem consultar saldo.
3. Abrir transação de banco e tentar inserir `IdempotencyRecord(customer_id, idempotency_key)` e reservar `(customer_id, transaction_id)`. Restrições únicas fazem uma chamada concorrente aguardar o commit ou rollback da dona das chaves.
4. Se qualquer chave já existir após a espera, devolver o `409` específico sem executar o domínio nem reproduzir a resposta original.
5. Validar produto/item, aplicabilidade ao cluster/escopo, entitlement do Product em um `CustomerPlan` utilizável (`ACTIVE`, `ACTIVE_PAID` ou `PAST_DUE`) e estado `ACTIVE` da `customer_wallet` e `item_wallet` já materializadas. `PAST_DUE`/`RENEWAL_INACTIVE` não bloqueia consumo de saldo existente. Product fora do plano retorna `403 product_not_entitled` sem registrar uso; Wallet ausente retorna `503 wallet_not_provisioned`. Este fluxo nunca cria Wallet.
6. Bloquear a `ItemWallet` de `(customer_id, item_id)` e ler seu estado mais recente. Nesse instante, obter `accepted_at` do relógio do banco; essa ordem serial define versão ativa e fronteira de ciclo.
7. Resolver a versão de preço ativa em `accepted_at` e conferir `expected_price_version_id`, quando enviado.
8. Atribuir ao evento o intervalo cumulativo de item units e somar `item_units` uma única vez.
9. Primeiro completar, se existir, o bloco pendente preso à versão antiga. Para a quantidade restante, usar a versão ativa validada. Calcular `converted_blocks`, `converted_item_units` e `pending_item_units_after` com divisão inteira e resto.
10. Para cada bloco completo, resolver `cycle_key` pela regra declarativa da respectiva versão em `accepted_at`. Bloquear os `PricingAccumulator` tocados em ordem lexicográfica de `(price_version_id, cycle_key)`, aplicar a faixa do intervalo acumulado e inserir um `BillingBlock` imutável.
11. Inserir `UsageEvent` e exatamente uma `ItemWalletEntry`, atualizar contadores/pending da `ItemWallet` e associar os `BillingBlock` formados.
12. Se nenhum bloco foi completado, persistir o resultado e as chaves de deduplicação com `customer_wallet_entry_id = null`; não bloquear nem criar entrada na `CustomerWallet`.
13. Se houve blocos, bloquear a `CustomerWallet` pai e os `CreditLot` elegíveis, calcular o débito e alocar primeiro os lotes de assinatura que expiram no ciclo atual, depois os lotes persistentes. Se a soma residual disponível não cobrir o débito, abortar a transação inteira com `409 insufficient_credit`; caso contrário, inserir um único `Debit` e `CustomerWalletEntry`, `CreditLotAllocation` imutável por lote, atualizar residuais e `balance_credit_units` e ligar ambos os extratos aos mesmos blocos.
14. Persistir `PricingAccumulator`, vínculos e resultado da operação no mesmo commit.
15. Responder `201` para toda chamada aceita. A V1 não emite `402` para consumo.

Não existe commit intermediário entre sensibilizar a `item_wallet` e, quando houver bloco, debitar a `customer_wallet`. Uma falha reverte evento, entrada do item, pendente, bloco, entrada de créditos e saldo. A consulta prévia de elegibilidade e as leituras dos extratos não participam desta transação.

### 6.2 Crédito direto e Compensation

Crédito direto segue a ordem: idempotência, validação da flag, bloqueio da `CustomerWallet`, `CustomerWalletEntry`, saldo de créditos, recibo e commit.

Compensation possui duas fases. Criar/submeter/aprovar persiste somente o recurso e sua auditoria, sem bloquear ou movimentar a Wallet. Executar uma Compensation aprovada valida idempotência e estado, bloqueia a `CustomerWallet`, cria a única `CustomerWalletEntry`, atualiza o saldo, grava referências/recibo e marca `EXECUTED` no mesmo commit. Nenhuma dessas fontes toca `ItemWallet`.

### 6.3 Ativação e renovação após pagamento confirmado

1. A primeira adesão aceita exclusivamente uma `SubscriptionPlanVersion`: o CustomerPlan é criado comercialmente `ACTIVE` depois de aprovada a admissão, de confirmar que a versão está ativa e não revogada e de reservar atômica e exclusivamente `ActiveCustomerPlanSlot(customer_id, subscription_id)` derivado por `plan_version_id → SubscriptionPlanVersion → Subscription`; ele materializa seu ciclo inicial somente ao completar ativação. `OnDemandPlan` não é plano de adesão e não percorre esse fluxo. Se a reserva já estiver ocupada por outro CustomerPlan ativo da mesma Subscription comercial, a contratação falha com `409 active_customer_plan_already_exists`, sem `CollectionRequest`, ciclo, entitlement ou crédito. `FREE` de adesão livre entra diretamente em `ACTIVATED`, efetiva o ciclo/entitlement e, se positiva, `granted_credit_units` uma única vez. `FREE` que exige cartão fica `PENDING_CARD_VALIDATION` sem ciclo, entitlement, crédito ou OnDemand até o webhook assinado de setup; então efetiva uma vez. `PAID` fica `PENDING_INITIAL_PAYMENT`, cria uma única `CollectionRequest` com `payment_expires_at` imutável e só efetiva ciclo, entitlement e franquia-base após confirmação definitiva. Em cada renovação `PAID`, somente se a versão ainda não foi revogada, cria exatamente uma `CollectionRequest` na data de renovação, usando o snapshot do plano, o cartão tokenizado salvo e a mesma política de janela. `NONE` nunca cria cobrança futura. Uma compra `OnDemandPlan` só cria sua `CollectionRequest` quando o CustomerPlan está `ACTIVE` e `ACTIVATED`, sua versão não está revogada e o OnDemandPlan pertence à mesma Subscription comercial; ela volta a validar todas essas condições antes de confirmar crédito, cria somente lote persistente de crédito e nunca modifica/cria ciclo, renovação, entitlement ou CustomerPlan.
2. Um trabalhador cria uma única `CollectionAttempt`/`BillingPayment` e chama o adaptador com chave idempotente estável. A resposta pode exigir autenticação adicional, confirmar, falhar ou ser incerta.
3. A resposta síncrona é transformada no mesmo evento normalizado usado por webhook; nenhum caminho especial emite Voucher/Cupom ou cria concessão duplicada.
4. O webhook entra em `WebhookInbox`, é deduplicado pelo ID do provedor e tenta avançar o mesmo `BillingPayment`; eventos antigos não regressam estado terminal.
5. Em resposta incerta, timeout ou ausência de webhook, Billing preserva a mesma tentativa e o estado conhecido; ele nunca cria segunda cobrança, cancela o CustomerPlan ou consulta automaticamente o provedor. Em renovação, falha definitiva recebida do provedor não reprograma tentativa, não inicia próximo ciclo e muda imediatamente a condição para `RENEWAL_INACTIVE`, sem revogar saldo já disponível.
6. Antes de `payment_expires_at`, a solicitação pode permanecer `PENDING_PAYMENT`, sem novo entitlement, ciclo ou crédito. Esse prazo é comercial e não watchdog técnico de webhook. Depois do prazo sem confirmação definitiva, a transição terminal para `EXPIRED` é atômica com o processamento de webhook e não produz efeito comercial ou na Wallet; quando for renovação, também deixa o CustomerPlan em `PAST_DUE`/`RENEWAL_INACTIVE`, sem cancelá-lo nem bloquear saldo existente. Cliques repetidos antes do vencimento reutilizam a mesma solicitação; uma nova intenção depois da expiração cria outra, sem reabrir a anterior.
7. Na primeira transição válida para `CONFIRMED`, bloquear/revalidar CustomerPlan e validar que ele não está `REVOKED`, além de versão do Plan ainda não revogada, valor, moeda, período/compra e que a solicitação ainda não tenha expirado. Solicitação terminalizada por revogação de plano ou `CUSTOMER_PLAN_REVOKED` não regressa e webhook tardio não produz efetivação.
8. Em `PAID`, confirmação definitiva muda `PENDING_INITIAL_PAYMENT` para `ACTIVATED`, concede entitlement/renova o período e cria, se positiva, a única `CustomerWalletEntry` `SUBSCRIPTION_CREDIT` e seu `CreditLot` do ciclo. Em `FREE` que exige cartão, o webhook de setup muda `PENDING_CARD_VALIDATION` para `ACTIVATED` e efetiva o direito e a mesma concessão-base/lote do ciclo, sem pagamento; `FREE` livre já o faz na criação. Em `ON_DEMAND`, somente creditar a compra confirmada em lote persistente se o CustomerPlan ainda estiver `ACTIVE` e `ACTIVATED`, sua versão não estiver revogada e o OnDemandPlan pertencer à mesma Subscription comercial; ativação pendente, `PAST_DUE`/`RENEWAL_INACTIVE`, estado terminal, versão revogada ou catálogo comercial diferente impedem efetivação inclusive em webhook tardio. OnDemand não altera ciclo, recorrência ou entitlement. A franquia residual de assinatura é baixada no fim do ciclo por `CREDIT_EXPIRY_FORFEITURE`; não acumula na renovação. Persistir ciclo, estado comercial, referências tipadas, lotes e Wallet de forma atômica ou por saga com outbox e consumidor idempotente, sem janela capaz de renovar ou creditar duas vezes.
9. Cancelamento normal de CustomerPlan recorrente confirmado só desabilita a renovação seguinte: até `current_period_end`, o CustomerPlan segue `ACTIVE` e o ciclo confirmado não é alterado. No fim, uma transição idempotente para `CANCELED` impede novo ciclo e nova `CollectionRequest`. Cancelar `PENDING_PAYMENT` fecha somente a solicitação pendente, sem entitlement/crédito. Revogação administrativa é fluxo separado e prioritário: marca `REVOKED` e, atomica/serializadamente, terminaliza cada solicitação pendente em `CANCELED/CUSTOMER_PLAN_REVOKED`; operação concorrente posterior ou webhook tardio revalida, perde e não efetiva nada.
10. Revogar uma versão de plano encerra idempotentemente em `CANCELED`/`PLAN_REVOKED` suas `CollectionRequest` pendentes de adesão, renovação, regularização, transição ou OnDemand, sem efetivação. Os CustomerPlans já confirmados não são revogados individualmente: concluem apenas o ciclo em andamento e, no fim, ficam `EXPIRED`/`PLAN_REVOKED`, sem cobrança, ciclo ou franquia novos.
11. Em transição de plano, downgrade reclassifica imediatamente o residual de assinatura para crédito `ON_DEMAND` persistente e reinicia âncora/ciclo sem nova concessão; upgrade só efetiva após webhook confirmado da `CollectionRequest` `PLAN_UPGRADE`, quando reclassifica o residual, inicia novo ciclo e concede a nova franquia integral. A reclassificação é idempotente, não muda saldo nem envolve preço/câmbio/proporcionalidade; falha ou expiração de upgrade não altera versão, ciclo, entitlement, Wallet ou âncora.
12. Em `PAST_DUE`/`RENEWAL_INACTIVE`, regularização manual cria uma nova `CollectionRequest` `RENEWAL_REGULARIZATION`, distinta da tentativa terminal anterior e sujeita à mesma `payment_completion_window`. Antes de webhook confirmado, não há ciclo, franquia ou cobrança automática nova; após validação idempotente, retorna a `ACTIVE_PAID`/`CURRENT`, ancora novo ciclo na efetivação e concede uma única franquia integral do plano vigente, sem retroagir o período inativo.

O vencimento de plano `PAID` recorrente não revogado cria a única intenção de cobrança exatamente na data de renovação; `FREE` e `NONE` não geram cobrança automática. A virada de ciclo `FREE` materializa a concessão-base sem Stripe enquanto a versão não estiver revogada. Webhooks podem chegar pelo menos uma vez e atrasados; as chaves naturais garantem uma única tentativa comercial, uma única confirmação/renovação e no máximo uma concessão-base por ciclo. A V1 não faz polling, watchdog ou consulta automática para suprir ausência de webhook.

#### Pagamento inesperado, conciliação e estorno externo

1. Toda confirmação deve corresponder a uma `CollectionRequest` criada pelo nosso sistema, com assinatura, período/compra, Plan, valor, moeda e provedor compatíveis.
2. Evento duplicado é reconhecido sem novo efeito. Evento atrasado de uma solicitação legítima é conciliado pelo ID, não pela hora de chegada.
3. Sem correspondência inequívoca, criar/atualizar `UnmatchedPaymentCase`; não renovar, não ativar entitlement e não conceder franquia-base na Wallet.
4. Tentar conciliação determinística antes de encerrar o caso. Nunca inferir renovação apenas por proximidade de data ou assinatura candidata.
5. Na V1, `MANUAL_REVIEW` aguarda ação manual de reconciliar ou encerrar com justificativa; não existe política automática, endpoint nem chamada local de devolução.
6. Se o operador decidir excepcionalmente estornar, ele o faz diretamente no provedor. O resultado assíncrono é recebido/conciliado como fato externo, deduplicado e preserva IDs externos; Billing não dispara ou repete a ação.
7. Caso o estorno externo tenha consequência de créditos, o operador cria uma Compensation manual e auditável; ela nunca edita/apaga lançamento original nem é criada automaticamente pelo evento de refund.

### 6.4 Resgate de vale

1. Resolver idempotência dentro da transação de créditos.
2. Bloquear o vale por ID.
3. Verificar `AVAILABLE` e ausência de vínculo que obrigue fluxo por cupom.
4. Bloquear a `CustomerWallet` beneficiada.
5. Transicionar vale para `USED`, criar `CustomerWalletEntry` e atualizar saldo no mesmo commit.

Duas requisições diferentes ao mesmo vale resultam em um único crédito; a perdedora recebe `409 voucher_unavailable`.

### 6.5 Resgate de cupom

1. Resolver idempotência dentro da transação de créditos, antes de verificar estoque ou limite do usuário.
2. Resolver cupom por ID ou código e validar estado/validade.
3. No modo por usuário, calcular o digest opaco e reservar a unicidade `(coupon_id, digest)`.
4. Selecionar um vale `AVAILABLE` do lote com bloqueio de linha e estratégia equivalente a `SKIP LOCKED`.
5. Bloquear a `CustomerWallet` beneficiada.
6. Marcar vale `USED`, inserir limite do usuário quando aplicável, criar `CustomerWalletEntry`, atualizar saldo e concluir idempotência no mesmo commit.

Se não houver vale, toda a transação é revertida, inclusive a reserva do usuário. Assim, o esgotamento não deixa um registro parcial que pareça um resgate consumado.

## 7. Concorrência, ordenação e consistência

Ordem global de aquisição de recursos para reduzir deadlocks:

1. chave de idempotência;
2. recurso exclusivo da operação: especialização/projeção operacional da Wallet, execução de recorrência, registro de usuário do cupom ou vale; a linha base imutável de `Wallet` é somente referenciada;
3. no consumo, `PricingAccumulator` em ordem lexicográfica de `(price_version_id, cycle_key)`;
4. Wallet base `CUSTOMER` + `CustomerWallet`;
5. inserção nos extratos e projeções.

Cenários obrigatórios:

- duas chamadas diferentes para o mesmo customer + item são serializadas pela `ItemWallet`; a segunda enxerga exatamente o pendente confirmado pela primeira;
- chamadas para `item_wallets` diferentes podem acumular em paralelo, mas débitos convergem na mesma `CustomerWallet` e são serializados antes do saldo;
- duas chamadas com o mesmo `transaction_id` e mesmo payload criam exatamente um efeito; a primeira conclui e a segunda recebe `409 transaction_already_exists`;
- mesma chave com payloads diferentes produz um lançamento no máximo e um `409`;
- duas chamadas concorrentes de 600 item units, com bloco 1.000 e pendente zero, resultam em total recebido 1.200, exatamente um bloco convertido e 200 item units pendentes, independentemente de qual chamada obtiver o lock primeiro;
- uma chamada que apenas cria pendente não bloqueia nem grava na customer wallet; concorrência permanece serializada na item wallet e no acumulador aplicável;
- `accepted_at` é obtido depois do lock da `ItemWallet`, de modo que chamadas ao mesmo item têm uma ordem única inclusive quando disputam uma fronteira de ciclo ou de versão de Price;
- duas chamadas em lados opostos de uma fronteira usam acumuladores diferentes; a primeira no novo ciclo começa na primeira faixa e nenhum bloco do ciclo anterior é alterado;
- duas chamadas que atravessam a mesma faixa são serializadas no mesmo `PricingAccumulator`; cada bloco recebe ordinal/faixa única e blocos anteriores não são reprecificados;
- dois resgates do último vale do cupom produzem um crédito e um `coupon_exhausted`;
- dois resgates com o mesmo `unique_user_id`, mas `transaction_id` diferentes, produzem um crédito e um conflito de limite;
- duas execuções do mesmo período da assinatura produzem um crédito;
- falha de processo antes do commit não deixa saldo, pendente, bloco, contador, vale ou execução parcial;
- após falha de rede pós-commit, retry recebe `409` com referência segura e o cliente consulta o recurso original.

Concorrência de provisionamento:

- duas tentativas de materializar a mesma wallet convergem pela chave única e retornam a mesma identidade;
- consumo nunca disputa uma criação lazy: aceita apenas Wallet `ACTIVE` já materializada;
- se o evento de desativação do Item é confirmado antes da validação bloqueada do consumo, o consumo é rejeitado; se o consumo já validou e bloqueou a projeção operacional em estado `ACTIVE`, ele conclui atomicamente antes do novo evento de ciclo de vida;
- a `customer_wallet` não passa a `ACTIVE` para operações enquanto `WalletProvisioning.materialized_item_wallets != expected_item_wallets` para a versão de escopo vigente.

O nível de isolamento pode ser `READ COMMITTED` com bloqueios explícitos e restrições únicas, desde que todos os fluxos obedeçam à mesma ordem. Se o banco escolhido não oferecer essas garantias, a implementação deverá provar comportamento equivalente antes de avançar.

## 8. Observabilidade e reconciliação

Métricas mínimas:

- débitos aceitos por status `201` e rejeições `409 insufficient_credit` sem efeito;
- item units recebidas, convertidas e pendentes por Item em agregações sem labels de customer;
- customers em `PROVISIONING`/`ERROR`, diferença entre item wallets esperadas e materializadas e tempo de convergência por escopo;
- blocos formados por `pricing_model` e transições de faixa/ciclo;
- idade do pendente mais antigo e falhas de avaliação de regra recorrente;
- `credit_units` debitados e concedidos por tipo, sem usar labels de alta cardinalidade;
- conflitos de deduplicação externa e deduplicações técnicas internas;
- atraso e falhas de recorrência;
- saldo de vales disponíveis por cupom em canal administrativo;
- deadlocks, retries transacionais e latência de lock da carteira.

Rotina de reconciliação:

- recalcular, em leitura consistente, a soma de `signed_credit_units` das `CustomerWalletEntry` por customer;
- comparar com `balance_credit_units` e o último `balance_after_credit_units`;
- comparar, por customer e `scope_version`, o conjunto de Wallets `ITEM` com o conjunto de Items aplicáveis;
- por `item_wallet`, verificar `total_received_item_units = total_converted_item_units + pending_item_units`;
- verificar uma `ItemWalletEntry` por `UsageEvent` aceito e, quando houver blocos, exatamente uma `CustomerWalletEntry` correlata;
- verificar que intervalos de `BillingBlock` particionam exatamente o prefixo convertido sem lacunas ou sobreposição;
- por versão + ciclo, comparar `PricingAccumulator` com a soma dos blocos e confirmar faixa/`credit_units` de cada ordinal;
- comparar a soma de `CreditLot` disponíveis com `balance_credit_units`, cada `CreditLotAllocation` com seu débito, cada `CREDIT_EXPIRY_FORFEITURE` com o residual de lote ainda classificado como assinatura e cada `CreditLotReclassification` com a quantidade residual preservada e a transição correspondente;
- emitir alerta e bloquear apenas operações administrativas de reparo, não apagar histórico;
- reparar somente por procedimento auditado ou reconstrução comprovada da projeção.

Eventos para integrações futuras devem sair por outbox gravada na mesma transação do razão. Consumidores externos não participam do commit de créditos.

## 9. Critérios de aceitação

### Créditos internos e Price

- Wallet, seus extratos, Price de Item, vale, cupom e Compensation não possuem campo de moeda ou dinheiro; somente `SubscriptionPlanVersion` contém preço comercial monetário, isolado dos créditos;
- contratos da wallet não aceitam preço comercial ou taxa de conversão; uma referência externa não altera créditos por si mesma;
- todo `credit_units` usa inteiro; parsing rejeita decimal, notação científica, negativo onde não permitido e overflow;
- `pricing_model = unit`, bloco 1, debita exatamente `credit_units` por unidade;
- configurações `1 item unit -> 1 wallet credit`, `10 -> 1` e `10 -> 10` produzem exatamente as conversões declaradas, sem dinheiro ou taxa implícita;
- `pricing_model = unit`, bloco 1.000, não debita 999 unidades e debita um bloco ao atingir 1.000;
- `tiered` aceita razões de conversão crescentes ou decrescentes entre faixas, inclusive `1 -> 1` seguido de `2 -> 1`, e aplica cada razão somente ao trecho posterior ao limite;
- limites não contíguos, sobrepostos, faixa finita cuja amplitude não seja divisível pelo próprio bloco ou ausência de faixa final aberta são rejeitados;
- `accumulation_cycle` ausente acumula por toda a vida da versão de preço e nunca reinicia;
- uma regra declarativa `FREQ=MONTHLY;INTERVAL=3` cria ciclos trimestrais sem exigir enum `quarterly` na API;
- objeto `accumulation_cycle` presente sem regra, ou com regra inválida, finita, ambígua ou sem próxima fronteira válida, não permite ativar o Price;
- instante exatamente na fronteira pertence ao novo intervalo UTC semiaberto;
- testes de fronteira cobrem cada início/fim de faixa, fronteira de ciclo e multiplicação máxima;
- `debited_credit_units` e a versão de Price de um débito histórico permanecem iguais após publicar nova versão.

### Hierarquia e provisionamento de wallets

- customer em cluster com 10 Items faturáveis aplicáveis materializa exatamente 11 Wallets: uma `CUSTOMER` e dez `ITEM`; qualquer quantidade de Items `ENTITLEMENT_ONLY` não altera esse total;
- duas tentativas concorrentes de provisionar o mesmo customer não criam segunda customer wallet nem segunda item wallet para o mesmo item;
- toda Wallet `ITEM` referencia a Wallet `CUSTOMER` do mesmo customer e exatamente um Item;
- checks rejeitam Wallet `CUSTOMER` com pai/item e Wallet `ITEM` sem pai/item;
- nenhuma coluna de `Wallet` aceita edição após a criação, e não existem operações de exclusão física ou virtual;
- desativação, reativação e erro são novos `WalletLifecycleEvent` imutáveis; a sequência completa reconstrói o estado efetivo;
- consumo nunca cria Wallet; hierarquia ausente/incompleta retorna `503 wallet_not_provisioned` sem evento ou entrada;
- customer só fica pronto quando a quantidade de item wallets materializadas coincide com o conjunto faturável aplicável na `scope_version`;
- tornar novo Item faturável aplicável materializa uma item wallet por customer do escopo antes do consumo;
- remover aplicabilidade registra a desativação sem editar a Wallet e sem apagar pendente ou extrato; reativação registra novo evento e reutiliza a mesma identidade;
- nenhuma `item_wallet` expõe `balance_credit_units` ou participa diretamente da elegibilidade.

### Consumo, blocos, débito e elegibilidade

- Product `ENTITLEMENT_ONLY` publicado rejeita Price e não materializa item wallet para seus Items/subitens;
- elegibilidade de `ENTITLEMENT_ONLY` ignora saldo e depende somente do entitlement efetivo; `POST /usage-events` retorna `409 product_not_metered` sem gravação;
- Product `CREDIT_METERED` exige Price nos Items faturáveis, item wallet materializada, entitlement e regra de saldo;
- bloco 1.000 -> 100 créditos, consumo 1.012: cria `ItemWalletEntry` de 1.012 item units, converte 1.000, preserva 12 e cria uma `CustomerWalletEntry` de `-100` créditos;
- após o cenário anterior, consumo 988 cria outra entrada em cada extrato, converte exatamente outro bloco e deixa pendente zero;
- consumo 999 com pendente zero cria `UsageEvent` e `ItemWalletEntry`, não cria `CustomerWalletEntry`, não altera `balance_credit_units` e expõe 999 item units pendentes;
- duas chamadas concorrentes de 600 item units criam duas `ItemWalletEntry`, exatamente um `BillingBlock`/`CustomerWalletEntry` e deixam 200 item units pendentes;
- `total_received_item_units = total_converted_item_units + pending_item_units` depois de sequências aleatórias, retries e concorrência;
- cada intervalo de unidade aparece em um único `BillingBlock` ou no sufixo pendente, nunca nos dois;
- uma chamada que completa vários blocos cria vários `BillingBlock`, uma `ItemWalletEntry` e uma `CustomerWalletEntry` consolidada com decomposição exata;
- versão de preço nova não reprecifica o bloco parcial antigo; completa-o no Price anterior e usa o novo somente na quantidade restante;
- no modelo `tiered`, atravessar uma faixa em uma chamada converte cada bloco com `unit_block_size` e `credit_units` da faixa correspondente, sem alterar blocos anteriores nem perder unidades na fronteira;
- ao iniciar um novo ciclo, o primeiro bloco usa a primeira faixa; acumulador e blocos do ciclo anterior permanecem inalterados;
- bloco iniciado antes e completado depois da fronteira usa o Price preso ao pendente e o ciclo determinado pelo `accepted_at` da conclusão;
- com bloco 1 e custo de 3 créditos, saldo `5` após uma unidade: entrada criada, saldo `2`, HTTP `201`;
- com bloco 1 e custo de 5 créditos, saldo `5` após uma unidade: entrada criada, saldo `0`, HTTP `201`, elegível por créditos;
- com bloco 1 e custo de 7 créditos, saldo `5` após uma unidade: retorna `409 insufficient_credit`, sem entrada, saldo continua `5` e não há pendente;
- com saldo negativo não há caso alcançável por consumo na V1; a futura capacidade `CREDIT_FLEXIBLE` deve especificar separadamente seu comportamento;
- produto/item inválido ou item wallet desabilitada não cria evento, pendente ou entrada;
- para `CREDIT_METERED`, a parcela financeira da elegibilidade depende somente de `CustomerWallet.balance_credit_units`; item units pendentes não são tratadas como saldo ou crédito;
- consulta de elegibilidade concorrente pode ficar obsoleta, mas nunca corrompe nem condiciona o débito.

### Deduplicação e idempotência interna

- qualquer reutilização de `Idempotency-Key` retorna `409 idempotency_key_already_used`, independentemente da equivalência do JSON;
- qualquer reutilização de `transaction_id`, mesmo com nova chave e mesmo payload, retorna `409 transaction_already_exists`;
- conflitos não incrementam `total_received_item_units`, não criam entrada/bloco/transação e não mudam saldo ou pendente;
- o `409` pode indicar o recurso existente para que o cliente consulte seu resultado atual/histórico;
- timeout depois de commit é recuperável por consulta após o conflito, sem duplicar movimento;
- idempotência interna de workers, webhooks e chamadas a provedores continua usando chaves naturais/determinísticas e pode adotar replay técnico, sem alterar o contrato externo de `409`.

### Créditos e recorrência

- cada flag bloqueia somente sua própria fonte de crédito;
- criar/ativar CustomerPlan materializa o ciclo inicial e, em `FREE` ou após confirmação `PAID`, pode conceder uma única franquia-base positiva do plano;
- todo ciclo recorrente materializado persiste âncora calendária UTC e período `[current_period_start, current_period_end)`, sem sobreposição; a renovação normal conserva a âncora da série, enquanto upgrade, downgrade e regularização confirmada a redefinem no instante efetivo;
- `WEEKLY` preserva dia da semana/horário UTC a cada 7 dias; para âncora mensal em `2026-01-31T10:00:00Z`, os fins seguintes são `2026-02-28T10:00:00Z`, `2026-03-31T10:00:00Z`, `2026-04-30T10:00:00Z` e `2026-05-31T10:00:00Z`, sem escorregar para o dia 28;
- `QUARTERLY` aplica o mesmo último-dia-apenas-no-mês-curto ao destino de três meses e volta ao dia original quando possível; `ANNUALLY` transforma `2024-02-29T10:00:00Z` em `2025-02-28T10:00:00Z`, `2026-02-28T10:00:00Z`, `2027-02-28T10:00:00Z` e `2028-02-29T10:00:00Z`;
- `NONE` não cria agenda de ciclo/cobrança futura nem término automático do CustomerPlan;
- existe no máximo um CustomerPlan principal em condição `ACTIVE` por `(customer_id, subscription_id)` comercial inferido; a condição inclui `ACTIVE_PAID` e exclui `PAST_DUE`/`RENEWAL_INACTIVE` e estados terminais. `ActiveCustomerPlanSlot` único/bloqueio transacional equivalente impede criação/ativação concorrente sem armazenar `subscription_id` no CustomerPlan; a perdedora retorna `409 active_customer_plan_already_exists` sem cobrança, ciclo, entitlement ou crédito. Subscriptions comerciais distintas permanecem independentes;
- troca de plano opera o único CustomerPlan da Subscription comercial e não cria outro vínculo; a nova `SubscriptionPlanVersion` pertence à mesma Subscription comercial. `OnDemandPlan` é crédito extra vinculado ao CustomerPlan e não participa da chave nem cria CustomerPlan;
- Product fora do plano ativo retorna `403 product_not_entitled` sem registrar consumo, mesmo que haja saldo;
- elegibilidade expõe separadamente `usage_model`, `commercial_status`, `customer_plan_entitled`, `credit_sufficient` quando aplicável, `access_allowed` e motivo;
- decisão de uso avalia CustomerPlan utilizável, entitlement, condição de renovação e crédito; retorna `customer_plan_not_active`, `entitlement_not_granted`, `insufficient_credit` ou `eligible`. `renewal_inactive` é reason secundário e não bloqueia uso de saldo; crédito insuficiente não altera o CustomerPlan;
- `CREDIT_STRICT` aceita somente `CREDIT_METERED`; o Plan define preço, recorrência e franquia-base por ciclo. Falha definitiva ou expiração de renovação deixa `PAST_DUE`/`RENEWAL_INACTIVE`, bloqueia apenas novo ciclo, franquia e cobrança automática, mas permite consumir créditos disponíveis se o débito couber no saldo;
- versão publicada de `SubscriptionPlanVersion` é imutável: não se altera preço, créditos, recorrência, admissão, meios ou Products; ela não expira pela passagem do tempo e só fica indisponível por revogação administrativa; uma nova oferta exige nova versão/plano;
- revogação administrativa de versão de plano bloqueia imediatamente novas adesões, transições e renovações; não migra/substitui automaticamente, não cria cobrança, Voucher/Cupom, estorno, Compensation ou e-mail/notificação na V1;
- CustomerPlan já confirmado em plano revogado conserva somente o ciclo em curso e encerra idempotentemente em `EXPIRED`/`PLAN_REVOKED` no seu fim, sem cobrança, novo ciclo ou franquia; créditos existentes na Wallet não são removidos nem compensados automaticamente;
- revogação de plano é diferente de revogação administrativa do vínculo: apenas esta última muda imediatamente o CustomerPlan individual para `REVOKED`; ambas são distintas de cancelamento do customer e de `PAST_DUE` por pagamento;
- revogação administrativa individual é terminal, prioritária e auditada com motivo, ator, instante e referências: na mesma serialização marca `REVOKED` e terminaliza todas as `CollectionRequest` pendentes do CustomerPlan em `CANCELED/CUSTOMER_PLAN_REVOKED`, sem estorno, remoção/edição de Wallet ou Compensation automática;
- depois de `REVOKED`, regularização, upgrade, downgrade, cancelamento normal, OnDemand, renovação e demais comandos não administrativos retornam `409 customer_plan_revoked`; webhooks tardios ou operações concorrentes perdedoras só convergem/auditam e não podem efetivar plano, ciclo, entitlement, crédito ou nova cobrança;
- `CollectionRequest` pendente de adesão, renovação, regularização, transição ou OnDemand para plano revogado é terminalizada idempotentemente sem entitlement, ciclo ou crédito; webhook tardio é validado contra a versão revogada e não pode efetivar esses efeitos;
- `FREE + NONE` é gratuito, não cobra, não renova e permanece `ACTIVE` e `ACTIVATED` por tempo indeterminado depois de consumir sua concessão inicial; crédito zero e passagem do tempo não encerram o CustomerPlan. `PAID + NONE`, depois da única confirmação, também não renova nem expira automaticamente;
- cancelamento normal de CustomerPlan recorrente confirmado apenas desabilita renovação futura; até `current_period_end` ele permanece `ACTIVE`, mantém entitlement, pode consumir em `CREDIT_STRICT` e, somente se `activation_status = ACTIVATED`, comprar qualquer `OnDemandPlan` publicado/não revogado da mesma Subscription comercial;
- no fim de `current_period_end`, cancelamento normal transita idempotentemente para `CANCELED`, sem novo ciclo, `CollectionRequest` ou créditos-base; ele não gera estorno, remoção de crédito ou Compensation;
- cancelamento de `PENDING_PAYMENT` encerra somente a solicitação/contratação pendente, sem entitlement ou crédito; revogação administrativa é ação separada, auditada e imediata para `REVOKED`, sem efeitos financeiros automáticos;
- V1 publica exclusivamente `CREDIT_STRICT`, com Products `CREDIT_METERED`; `CREDIT_FLEXIBLE` e `ENTITLEMENT_ONLY` continuam apenas como modelos futuros não ativáveis;
- confirmação interna correlacionada de pagamento cria no máximo uma entrada `SUBSCRIPTION_CREDIT` para o ciclo, quando `granted_credit_units > 0`, e nunca cria Voucher ou Cupom;
- toda concessão positiva materializa `CreditLot` com origem, residual e validade quando aplicável; cada débito registra `CreditLotAllocation` e a soma de residuais disponíveis concilia com a Wallet;
- franquia-base de assinatura residual expira integralmente no fim do ciclo por `CREDIT_EXPIRY_FORFEITURE` idempotente, sem editar entrada original, somente enquanto ainda classificada como assinatura; renovação normal concede somente a franquia do novo ciclo;
- crédito de `OnDemandPlan`, inclusive residual de assinatura reclassificado em troca, é lote persistente: acumula através de renovação/troca e não expira por esses eventos na V1;
- em `CREDIT_STRICT`, consumo aloca primeiro créditos de assinatura que expiram no ciclo atual e depois lotes persistentes; se a soma não cobre o débito, retorna `409 insufficient_credit` sem efeito;
- `signed_credit_units = 0` é rejeitado; crédito direto, Vale, consumo e Compensation com zero são rejeitados sem lançamento;
- concessão direta usa a quantidade solicitada, Vale usa a quantidade persistida e Cupom apenas seleciona o Vale; Voucher/Cupom e Compensation são bônus/correção independentes da franquia-base do plano;
- `OnDemandPlan` só pode ser iniciado e efetivado por CustomerPlan principal em condição comercial `ACTIVE`, com `activation_status = ACTIVATED`, versão não revogada e pertencente à mesma Subscription comercial; ele não é `SubscriptionPlanVersion`, não participa de adesão, não cria CustomerPlan, ciclo, renovação ou entitlement próprio e não altera recorrência ou plano principal. `PENDING_INITIAL_PAYMENT` e `PENDING_CARD_VALIDATION` retornam `409 customer_plan_activation_pending`, sem `CollectionRequest` nem crédito. Saldo zero não encerra CustomerPlan nem impede essa compra após ativação. `PAST_DUE`/`RENEWAL_INACTIVE`, `EXPIRED`, `CANCELED` e `REVOKED` não podem iniciar nem efetivar nova compra, mesmo com crédito disponível; essa vedação não bloqueia o consumo `CREDIT_STRICT` de saldo já existente. Sua `CollectionRequest` recebe `payment_expires_at` imutável e, após confirmação dentro da janela e revalidação de estado e ativação, cria somente crédito extra persistente na Wallet. CustomerPlan ativo e ativado pode acessar qualquer OnDemandPlan publicado/não revogado da mesma Subscription comercial; não há allowlist por plano de assinatura nem acesso entre Subscriptions comerciais;
- troca de plano é transição auditável e idempotente: upgrade cria uma única `CollectionRequest` `PLAN_UPGRADE` e, somente após confirmação do valor integral, reclassifica residual de assinatura para `ON_DEMAND` persistente e muda versão/ciclo/âncora; falha ou expiração não altera nada;
- downgrade reclassifica imediatamente residual de assinatura para `ON_DEMAND` persistente e muda a versão sem cobrança, estorno, Compensation, desconto, Voucher/Cupom ou proporcionalidade; ambos os sentidos reiniciam a âncora e a primeira cobrança posterior ocorre no fim do novo ciclo;
- reclassificação preserva a mesma quantidade de `credit_units`, é auditável/idempotente e não edita lançamentos, não muda saldo, não cria crédito e não envolve preço monetário, câmbio ou desconto; o residual convertido passa à segunda prioridade de consumo e não expira no fim do novo ciclo;
- entregas concorrentes da mesma confirmação/período convergem para uma única renovação e, se positiva, uma única concessão-base;
- resposta síncrona confirmada e webhook duplicado convergem para o mesmo `BillingPayment`, uma única efetivação de entitlement e no máximo uma concessão-base;
- adicionar um novo provedor exige somente um `BillingConnector` compatível; não altera contratos, estados ou invariantes de Subscription e Wallet;
- o núcleo rejeita uma operação antes da chamada externa quando o conector não declara a capacidade ou o meio solicitado;
- na V1, toda `CollectionRequest` usa exclusivamente cartão tokenizado no provedor;
- `PaymentMethodBinding` pertence ao mesmo workspace, customer e `BillingConnection` da cobrança; vínculo opcional com assinatura não permite uso cruzado;
- webhook assinado, incluindo entrega tardia válida, converge idempotentemente para o mesmo estado sem duplicar cobrança, renovação ou lançamento; ausência de webhook preserva o estado conhecido;
- webhook inválido não altera cobrança; webhook antigo não regride estado terminal; evento válido pode ser reprocessado pela `WebhookInbox`;
- na V1, não há retentativa automática, polling, watchdog, cron de consulta ao provedor, fila automática de divergências ou cancelamento por ausência/atraso de webhook;
- toda `CollectionRequest` de contratação inicial `PAID`, renovação, regularização manual, `OnDemandPlan` ou transição paga persiste `payment_expires_at` a partir da `payment_completion_window` da Subscription; Billing não altera esse prazo;
- antes do prazo, `PENDING_PAYMENT` não efetiva entitlement, ciclo nem crédito; após o prazo, `EXPIRED` é terminal, idempotente e atômico contra webhook concorrente, sem efetivar qualquer desses efeitos;
- cliques repetidos antes da expiração reutilizam a mesma solicitação; uma nova intenção após `EXPIRED` cria nova `CollectionRequest`, sem reabrir a anterior;
- expirar `OnDemandPlan` não cancela nem muda a Subscription já `ACTIVE`; expirar renovação não a cancela, mas a leva imediatamente a `PAST_DUE`/`RENEWAL_INACTIVE`. `payment_completion_window` é prazo comercial e não watchdog de webhook;
- após falha definitiva ou expiração de renovação, não há nova `CollectionRequest`, ciclo ou `SUBSCRIPTION_CREDIT` automático; o vínculo passa a `PAST_DUE`/`RENEWAL_INACTIVE`, sem revogar créditos já disponíveis por qualquer origem/lote compatível;
- customer pode regularizar cartão/cobrança por fluxo manual na UI segura do provedor; isso cria nova `CollectionRequest` `RENEWAL_REGULARIZATION`, idempotente e distinta da tentativa terminal, sem retentativa automática, carência automática ou pós-pago;
- antes da confirmação da regularização, `PAST_DUE`/`RENEWAL_INACTIVE` não cria ciclo/franquia/cobrança automática e preserva o uso de saldo existente; webhook assinado confirmado retorna a `ACTIVE_PAID`/`CURRENT`, inicia ciclo na efetivação e cria no máximo uma franquia integral vigente, sem crédito retroativo;
- repetição do comando de regularização com a mesma idempotência retorna a mesma solicitação pendente; webhooks duplicados dessa solicitação convergem para uma única reativação, ciclo e franquia;
- pagamento confirmado sem `CollectionRequest` compatível cria `UnmatchedPaymentCase` e não renova, não ativa nem concede franquia-base;
- webhook atrasado de cobrança legítima é conciliado por IDs mesmo fora da janela cronológica esperada;
- a V1 não expõe endpoint, tela, botão, solicitação de customer, regra de elegibilidade nem refund automático; cancelar Subscription não gera estorno;
- estorno excepcional é executado diretamente pelo operador no provedor e Billing somente concilia o resultado externo, sem disparar, repetir ou simular devolução;
- consequência interna de estorno externo exige Compensation manual, com tipo versionado, motivo, referências tipadas à cobrança/`CollectionRequest` e ID externo quando houver; não há Compensation automática;
- Voucher é campanha/promocional, Cupom altera condição de cobrança e Compensation é ajuste excepcional administrativo; nenhum deles edita/apaga lançamento original;
- débito de Compensation em `CREDIT_STRICT` respeita invariantes de saldo/estado ou permanece caso operacional auditado, sem saldo negativo ou pós-pago implícito;
- troca de plano, cancelamento e revogação preservam histórico e entitlement por vigência; cancelamento normal só é efetivo no fim do ciclo confirmado, revogação possui razão/ator auditáveis e pausa/retomada não existem na V1;
- falha antes do commit permite retry; falha depois do commit encontra a concessão concluída.

### Vale e cupom

- o contrato de resgate não aceita `credit_units`; usa a quantidade persistida no vale;
- dois resgates do mesmo vale nunca geram dois créditos;
- último vale sob concorrência é consumido uma vez;
- no modo livre, usuários diferentes ou iguais podem resgatar enquanto houver vales, usando `transaction_id` distintos;
- no modo por usuário, igualdade exata dentro do mesmo cupom impede segundo resgate, sem afetar outro cupom;
- cupom esgotado não deixa registro parcial de usuário;
- repetição de resgate retorna `409` pela chave reutilizada e nunca seleciona outro Vale, mesmo após mudança posterior do estoque.

### Compensações e auditoria

- criar Compensation ou CompensationBatch não altera saldo; apenas uma Compensation `READY` pode ser executada;
- tipo sem aprovação obrigatória vai a `READY` na submissão; tipo com aprovação vai a `PENDING_APPROVAL` e exige uma ação explícita de aprovação antes de `READY`;
- concorrência ou repetição na execução gera exatamente uma entrada `COMPENSATION`; a chamada perdedora recebe `409` e delta zero é rejeitado;
- Compensation executada é terminal e preserva vínculo com tipo/versionamento, descrição, solicitante, aprovador quando houver, executor, Batch e lançamento original quando aplicável;
- uma operação em massa materializa uma Compensation por customer; o Batch não produz lançamento agregado nem compartilha uma execução entre customers;
- consultas e mutações permanecem logicamente filtradas pelo `workspace_id` informado;
- toda Compensation contém solicitante, descrição e tipo; quando houver aprovação, o ator e o instante são preservados;
- toda transação aceita preserva `description`, metadata canônica e zero ou mais `WalletTransactionReference` oficiais no mesmo commit;
- consultar por `transaction_id` devolve a metadata originalmente persistida sem depender de logs; metadata nunca é usada para calcular ou substituir a referência oficial da origem;
- buscar por referência de plano, assinatura, Vale, Cupom, Product, Item ou pagamento encontra todas as entradas correlatas sem consultar metadata;
- ID colocado somente em metadata não satisfaz integridade referencial nem filtros de referência oficiais;
- soma do extrato da `customer_wallet` reconcilia com saldo após testes concorrentes;
- soma dos extratos das `item_wallets` reconcilia item units recebidas, convertidas e pendentes, sem alterar o saldo global por conta própria.

### Extratos transacionais

- cada alteração de saldo aparece somente no extrato da `customer_wallet`, com `signed_credit_units`, saldo anterior/resultante, origem e referências;
- toda chamada de consumo aparece no extrato da `item_wallet`, mesmo sem bloco completo ou débito global;
- lançamentos de item identificam produto, item, item units, Price, ciclo, faixa e acumulado sem depender do estado atual do catálogo;
- quando há conversão, ambos os extratos identificam os mesmos `BillingBlock` e se correlacionam por IDs recíprocos;
- resgate por cupom identifica simultaneamente cupom e vale efetivamente consumido;
- promoção é rastreável ao vale persistido que forneceu `credit_units`;
- atualizar nome, preço, assinatura, cupom ou vale não altera a representação histórica já registrada;
- não existe endpoint de editar ou excluir lançamento;
- correção administrativa executa uma Compensation, adiciona lançamento compensatório e preserva o original;
- percorrer todas as páginas de cada cursor estável retorna cada entrada daquele extrato exatamente uma vez, mesmo com novas entradas concorrentes;
- `balance_credit_units` retornado por `GET /customer-wallet` coincide com `balance_after_credit_units` da `CustomerWalletEntry` mais recente.

## 10. Sequência futura de implementação

Este documento não autoriza nem executa código. Quando a implementação for aprovada, a ordem recomendada é:

1. **Descoberta do template e ADRs:** mapear framework HTTP, banco, migrações e convenções do template; registrar as decisões de `credit_units`, razão/projeção e idempotência.
2. **Primitivas do domínio:** tipos de domínio para IDs, créditos inteiros, quantidades, períodos e erros; testes de parsing, overflow e cálculo graduado.
3. **Hierarquia materializada:** tabela `Wallet`, especializações `CustomerWallet`/`ItemWallet`, chaves parciais, vínculo pai/item, `WalletProvisioning` e reconciliação de escopo.
4. **Customer wallet e idempotência:** `CustomerWalletEntry`, saldo global, transação comum e testes concorrentes; esta é a fundação das concessões e débitos.
5. **Catálogo e Price versionado:** Product `CREDIT_METERED`/`ENTITLEMENT_ONLY`, Items/subitens, conversões `unit`/`tiered`, regra declarativa de ciclo, validação de faixas e publicação imutável.
6. **Item wallet e consumo:** `ItemWalletEntry`, `PricingAccumulator`, `BillingBlock`, extratos correlatos, bloqueio atômico por saldo insuficiente (`409` sem efeito), pendentes e testes concorrentes.
7. **Crédito direto e configuração:** flags por workspace e fronteira com confirmação externa.
8. **Subscription e Billing:** ofertas, Plans com preço/recorrência, política de admissão, entitlement, cobrança local, conexões/adaptadores, tokenização referenciada, `CollectionRequest`, `BillingPayment` e `WebhookInbox` idempotente.
9. **Vales:** ciclo de vida, resgate direto e auditoria promocional.
10. **Cupons:** lote, seleção concorrente, modo livre e limite por identificador externo.
11. **Compensações administrativas:** lifecycle de intenção/aprovação/execução, auditoria de atores e lançamento idempotente.
12. **Operação:** outbox, métricas, reconciliação de wallets/extratos, testes de carga/lock e documentação OpenAPI.

Cada etapa deve terminar com migrações reversíveis, contrato OpenAPI atualizado, testes unitários e de integração, cenários concorrentes no banco real e evidência de que os invariantes anteriores continuam válidos. Consumo não começa antes do provisionamento materializado e da verificação de entitlement; concessão por pagamento confirmado, vale e cupom não começam antes de `CustomerWalletEntry`/idempotência passarem nos testes de concorrência.

## 11. Pontos a ratificar antes da implementação

O plano adota defaults definidos para não bloquear o desenho, mas há decisões e detalhamentos que devem ser fechados antes de codificar seus módulos:

1. **Eventos de domínio e entrega:** catalogar eventos correlacionados para toda ação e transição de estado relevante de Product, Price, Wallet, Subscription, Billing, Vale, Cupom e Compensation; definir pares antes/depois quando houver efeito externo, schemas/versionamento, ordem por agregado, outbox, retry, deduplicação, dead-letter, retenção e transporte por broker, webhook de saída, polling ou combinação. Também definir cadastro de destinos, filtros, replay operacional e observabilidade de entrega.
2. **Pausa e retomada futuras:** se forem introduzidas depois da V1, definir instante efetivo, efeitos no período já pago, entitlement, saldo, créditos, cobrança, congelamento ou não de ciclo, Products habilitados e eventos. Na V1 não há pausa/retomada; cancelamento no fim do período, revogação administrativa e falha/regularização de renovação são os únicos caminhos relevantes além de troca de plano.
3. **Consultas, relatórios e métricas:** separar consultas operacionais de recursos individuais, relatórios agregados exportáveis e métricas técnicas. Definir filtros, paginação, consistência temporal, exportação e retenção para extratos, consumo, saldo, concessões, assinaturas, cobranças, falhas, Vales/Cupons e Compensations. Métricas são séries agregadas para operação; relatórios são dados de negócio consultáveis/exportáveis e não substituem o razão.
4. **Rotinas de reconciliação:** detalhar jobs, frequência, cursores, tolerâncias, alertas e reparos para comparar saldo com extrato, item units com blocos/pendente, assinatura com cobrança, estoque de Vales com resgates e Compensations com lançamentos. Divergência nunca permite edição destrutiva; reparo usa reprocessamento idempotente ou lançamento compensatório.
5. **Evolução de meios de pagamento:** a V1 já fecha cartão tokenizado via provedor externo, primeira confirmação após autenticação adicional quando exigida, uma tentativa comercial na renovação e reconciliação sem segunda cobrança. Pix, boleto, wallets de provedor e qualquer outro meio exigem desenho e aprovação próprios antes de ativação futura.
6. **Produto de estorno futuro:** antes de expor solicitação, tela, endpoint ou automação de refund, definir elegibilidade comercial, cálculo proporcional de créditos já usados, reserva, disputas/chargebacks, aprovações, reconciliação e efeitos no entitlement/Wallet. Nenhum desses comportamentos está ativo na V1.
7. **Conciliação operacional futura:** definir prazo técnico/watchdog de webhook, consulta do mesmo PaymentIntent, reconciliação periódica, fila estruturada de revisão manual, alertas e regras de escalonamento. Nada disso está ativo na V1 webhook-first.

Decisão ratificada nesta revisão: todos os Vales vinculados ao mesmo Cupom devem possuir exatamente a mesma quantidade de `credit_units`. A publicação/vinculação rejeita lote heterogêneo, e alterações posteriores não podem quebrar essa homogeneidade.

Alterar qualquer uma dessas respostas exige atualizar este documento e os critérios de aceitação antes da implementação; nenhuma delas impede concluir o planejamento atual.
