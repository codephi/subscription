# Plano técnico da API de assinaturas em Rust

**Status:** planejamento arquitetural, sem implementação

**Data:** 25 de agosto de 2026

**Revisão:** `customer_wallet` em créditos, `item_wallet` em item units, Price `unit`/`tiered`, uso pendente e ciclos declarativos

**Objetivo:** definir o domínio, os contratos HTTP, as garantias transacionais e a sequência de entrega de uma API de créditos, assinaturas e consumo pós-pago por workspace.

## 1. Resumo executivo

A API mantém dois tipos de wallet/subledger. A `customer_wallet` é a carteira principal da conta/customer, guarda saldo fungível somente em `credit_units` e possui extrato completo. A `item_wallet`, única por customer + item, contabiliza item units consumidas, pendentes e convertidas, também com extrato próprio. Nenhuma delas armazena moeda ou valor monetário.

O consumo é sempre pós-pago. Uma chamada válida primeiro registra as item units e atualiza o medidor/extrato da `item_wallet`. Cada Price define a conversão por bloco entre item units e wallet credits, por exemplo `1 -> 1`, `10 -> 1` ou `10 -> 10`. Quando o acumulado completa blocos, a mesma transação emite o débito correspondente na `customer_wallet`; o resto permanece pendente na `item_wallet`. Se esse débito produzir saldo de créditos negativo, tudo é persistido e a resposta é `402 Payment Required`. O `402` informa insuficiência de wallet credits após consumo aceito; não significa rollback nem cobrança monetária.

Produto é o agrupador de regras e elegibilidade e não contém preço. Product `CREDIT_METERED` possui Items com Prices versionados `unit` ou `tiered`, que convertem item units em débitos de wallet credits. Product `ENTITLEMENT_ONLY` possui Items/subitens de acesso, sem Price ou item wallet. A `customer_wallet` recebe créditos por concessão direta, pagamento confirmado de plano, Vale ou Compensation e também registra pagamentos de plano com delta zero; ela é a única fonte da parcela financeira da elegibilidade. A assinatura é a fonte de entitlement.

O catálogo de Subscription pode declarar o preço comercial monetário de um plano e quantos wallet credits um pagamento confirmado concede, mantendo os dois campos conceitualmente separados. Cobrança e confirmação do pagamento pertencem ao Billing externo; a Wallet recebe somente a concessão inteira de créditos autorizada. A conversão do Price de Item é somente de item units consumidas para wallet credits debitados; não é câmbio nem conversão de dinheiro.

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
9. Se uma chamada completar blocos, o débito na `customer_wallet` é registrado independentemente do saldo. Saldo resultante negativo produz `402`; saldo zero ou positivo produz `201`. Uma chamada que apenas aumenta o pendente produz somente `ItemWalletEntry`, sem transação na `customer_wallet`, e retorna `201`.
10. Um Price tem `pricing_model = unit` ou `tiered`. Em `unit`, todo bloco converte para a quantidade fixa de wallet credits. Em `tiered`, faixas progressivas graduadas são aplicadas às item units convertidas acumuladas por customer + item + versão de Price + ciclo. `accumulation_cycle` ausente significa que o acumulado nunca reinicia; presente, contém uma regra recorrente declarativa escolhida pelo cliente. Cada bloco posterior usa a faixa correspondente sem reprecificar débitos já lançados.
11. A consulta de elegibilidade sempre exige entitlement efetivo. Para `CREDIT_METERED`, também exige `balance_credit_units >= 0`; para `ENTITLEMENT_ONLY`, não consulta saldo. Ela não reserva créditos e nunca autoriza sozinha consumo ou débito. A `item_wallet` não participa do saldo elegível.
12. Toda alteração externa de créditos e toda chamada de consumo recebem `transaction_id`. Para recorrência, a chave natural adicional é assinatura + período. Para cupom no modo por usuário, `unique_user_id` é uma chave de limite distinta do `transaction_id`.
13. Um `transaction_id` é único dentro do workspace/customer, em todos os tipos de operação. Qualquer reutilização retorna `409 transaction_already_exists`, independentemente do payload, e nunca cria novo efeito. Toda criação externa transacional também exige `Idempotency-Key`; qualquer reutilização dessa chave retorna `409 idempotency_key_already_used`, sem replay da resposta original.
14. Datas e períodos são armazenados em UTC. Agendamentos mensais usam calendário, e não uma duração fixa de dias.
15. Wallets são materializadas no provisionamento. Criar customer materializa sua `customer_wallet` e uma `item_wallet` somente para cada Item faturável aplicável de Product `CREDIT_METERED`; consumo não cria wallets de forma preguiçosa.
16. Product possui `usage_model` imutável após publicação: `CREDIT_METERED` ou `ENTITLEMENT_ONLY`. O primeiro usa Price, item wallet e saldo; o segundo concede apenas acesso por assinatura e proíbe Price, item wallet e lançamento de consumo.
17. Subscription é a fonte de verdade de entitlement e calendário. Billing é adaptador de cobrança assíncrona; resposta síncrona e webhook convergem para a mesma tentativa, e somente pagamento `CONFIRMED` pode conceder créditos recorrentes.
18. Uma `SubscriptionOffering` declara exatamente um de três perfis: `CREDIT_STRICT`, `CREDIT_FLEXIBLE` ou `ENTITLEMENT_ONLY`. O perfil fixa compatibilidade de Products, concessão de créditos e efeito da cobrança; combinações fora da matriz são rejeitadas na publicação.

## 3. Limites do escopo

Incluído na v1:

- `customer_wallet` e razão de wallet credits por conta/workspace;
- `item_wallet`, extrato e medidor de item units por customer + item faturável;
- catálogo de produtos, itens e versões de preço;
- Products medidos por crédito e Products somente por entitlement;
- elegibilidade de produto combinando plano, estado de entitlement e, quando aplicável, saldo;
- registro e acumulação de consumo pós-pago por customer + item;
- débitos por blocos completos e consulta de item units pendentes/convertidas;
- crédito direto;
- ofertas de Subscription, Plans versionados com preço/créditos, subscriptions recorrentes ou não renováveis, Products habilitados, modalidade de entitlement e adesão do customer;
- concessão recorrente de créditos somente após confirmação externa de pagamento;
- fronteira de Billing, conectores de provedor, tentativas, webhooks e conciliação assíncrona;
- vales e cupons;
- Compensations administrativas;
- histórico, auditoria, idempotência e reconciliação.

Fora da v1:

- armazenamento ou processamento de dados brutos de cartão; meios ficam tokenizados no provedor;
- regras fiscais, impostos, câmbio, emissão fiscal e semântica completa de refund/chargeback; integrações iniciais apenas normalizam esses fatos para tratamento posterior;
- fórmula dinâmica ou taxa de câmbio entre dinheiro e `credit_units`; o plano apenas persiste preço comercial e concessão de créditos como termos independentes;
- estorno que apaga uma operação original;
- reserva prévia de saldo;
- preço no produto;
- calendários baseados em fuso local; todas as fronteiras do ciclo são instantes UTC;
- cadastro ou interpretação de usuários finais do cliente;
- transferência de saldo entre workspaces;
- transferência de item units entre `item_wallets` ou conversão entre unidades de itens diferentes;
- saldo de wallet credits dentro da `item_wallet`;
- expiração automática de dívida ou de saldo.

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
- saldo negativo é válido;
- `balance_credit_units` após cada operação deve coincidir com `balance_after_credit_units` da última `CustomerWalletEntry`;
- a linha nasce materializada com saldo inicial zero durante o provisionamento do customer, nunca durante consumo ou crédito.

Somente a `customer_wallet` responde elegibilidade por saldo. A existência, pendência ou volume de uma `item_wallet` não substitui nem fragmenta esse saldo.

### 4.3 CustomerWalletEntry

Registro contábil imutável de créditos e unidade do extrato transacional completo da wallet.

Tipos:

- `DEBIT`;
- `DIRECT_CREDIT`;
- `SUBSCRIPTION_PAYMENT`;
- `VOUCHER_CREDIT`;
- `COMPENSATION`.

Campos principais:

- `customer_wallet_entry_id`;
- `customer_id`;
- `type`;
- `signed_credit_units`: inteiro com sinal; positivo adiciona saldo e negativo remove. Zero é aceito exclusivamente em `SUBSCRIPTION_PAYMENT` de Plan com `granted_credit_units = 0`;
- `balance_before_credit_units`, `balance_after_credit_units`;
- `transaction_id`, quando a origem for uma chamada externa;
- `description` opcional, texto humano imutável com tamanho limitado;
- `metadata` opcional, objeto JSON imutável com limites de tamanho, profundidade, quantidade de chaves e valores escalares permitidos;
- `source_channel`, distinguindo consumo por produto/item, crédito direto, recorrência, vale direto, cupom, promoção baseada em vale e Compensation administrativa;
- instantâneo do cálculo de créditos;
- `created_at`, `request_id` e referência opcional de origem informada pelo chamador.

Invariantes:

- `balance_after_credit_units = balance_before_credit_units + signed_credit_units`;
- uma operação de domínio produz no máximo uma entrada no razão;
- toda mudança de saldo produz exatamente uma entrada no razão;
- toda operação que efetivamente movimenta créditos produz exatamente uma entrada; pagamento confirmado de Plan produz uma entrada mesmo quando `granted_credit_units = 0`;
- lançamento zero é válido somente para `SUBSCRIPTION_PAYMENT` e exige referência oficial ao Plan, assinatura e pagamento confirmado; qualquer outra origem com zero é rejeitada;
- lançamentos não são alterados ou removidos;
- `description` e `metadata` são somente contexto auditável: não alteram valor, tipo, idempotência, Price, entitlement ou referências tipadas;
- `metadata` aceita chaves livres do cliente, é preservada na forma canônica e devolvida integralmente nas consultas da transação; não pode ser editada depois do commit;
- metadata não cria uma transação genérica/manual: a operação continua obrigada a ter uma origem de domínio válida, como consumo, crédito direto, pagamento de Plan, Vale ou Compensation;
- o registro de deduplicação preserva o hash de `description` e da forma canônica de `metadata` para auditoria, mas qualquer reutilização da chave retorna conflito, com payload igual ou diferente.

`WalletTransactionReference` é a tabela filha 1:N que mantém relações oficiais sem inflar `CustomerWalletEntry` com dezenas de colunas nulas:

- `wallet_transaction_reference_id`, `customer_wallet_entry_id` e `reference_kind`;
- exatamente um alvo tipado por linha, como `subscription_id`, `plan_version_id`, `voucher_id`, `coupon_id`, `product_id`, `item_id`, `usage_event_id`, `item_wallet_entry_id`, `debit_id`, `collection_request_id`, `billing_payment_id` ou `external_reference`;
- check exige exatamente um alvo compatível com `reference_kind` e FKs são aplicadas a todos os recursos internos;
- unicidade em `(customer_wallet_entry_id, reference_kind, target)` impede duplicação da mesma relação;
- índices por `(reference_kind, target, customer_wallet_entry_id)` permitem localizar o extrato a partir de plano, Vale, Product, pagamento ou outra origem;
- referências são inseridas no mesmo commit da entrada, são imutáveis e não aceitam exclusão;
- `metadata` nunca substitui `WalletTransactionReference`; IDs canônicos enviados dentro de metadata continuam apenas texto opaco e não criam relacionamento.

Referências esperadas por origem:

- consumo: `item_wallet_id`, `item_wallet_entry_id`, `usage_event_id`, `debit_id`, `product_id`, `item_id`, IDs dos blocos convertidos, versões de Price, item units recebidas/convertidas/pendentes e `transaction_id`;
- crédito direto: `direct_credit_id`, referência externa opcional e `transaction_id`;
- pagamento de plano: `subscription_id`, `plan_version_id`, `collection_request_id`, `billing_payment_id`, período/compra e `payment_credit_grant_id`;
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
- status HTTP original (`201` ou `402`) e recibo original.

`Debit` existe somente quando o evento completa um ou mais blocos. Ele agrega, em uma única `CustomerWalletEntry`, todos os blocos completados atomicamente pela chamada e preserva uma decomposição imutável por Price/faixa. `debited_credit_units` é a soma dos wallet credits configurados nos blocos, nunca inclui `pending_item_units_after` e nunca arredonda uma fração para cima.

O item deve pertencer ao produto informado e ambos devem estar ativos no primeiro processamento. Falhas de catálogo não registram evento nem alteram acumulador. Falta de saldo nunca bloqueia o evento, o acumulador ou um débito devido.

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

### 4.10 SubscriptionOffering, SubscriptionPlan e CustomerSubscription

`SubscriptionOffering` é o modelo comercial, como `One Vibe Pro`, e agrupa planos alternativos. Ele não cobra, confirma pagamento nem movimenta Wallet.

Cada oferta possui `subscription_model` imutável após publicação. O contrato aceita exatamente os três valores abaixo; não existe um quarto modelo `HYBRID`. Recorrência não é atributo do Plan: a `CustomerSubscription` define `renewal_mode = RECURRING | NON_RENEWING`, regra/âncora e política de retentativas.

- `CREDIT_STRICT`: aceita somente Products `CREDIT_METERED`; exige `renewal_mode = RECURRING` e um Plan com `granted_credit_units > 0`. Falha definitiva da renovação suspende todos os Products mesmo com saldo restante. Antes do vencimento, saldo insuficiente bloqueia o Product, mas não cancela a assinatura.
- `CREDIT_FLEXIBLE`: aceita Products `CREDIT_METERED` e `ENTITLEMENT_ONLY` na mesma oferta, exige `renewal_mode = RECURRING` e um Plan com `granted_credit_units > 0`. Falha da renovação deixa o estado comercial inadimplente, mas preserva entitlement até cancelamento. Cada Product decide o acesso: `ENTITLEMENT_ONLY` continua disponível; `CREDIT_METERED` continua somente enquanto `balance_credit_units > 0`.
- `ENTITLEMENT_ONLY`: aceita somente Products `ENTITLEMENT_ONLY` e usa Plan com `granted_credit_units = 0`. A Subscription pode ser `RECURRING`, no qual cada renovação mantém o acesso e a falha definitiva o suspende, ou `NON_RENEWING`, no qual um único pagamento confirmado ativa o entitlement sem prazo final.

Matriz normativa:

| `subscription_model` | Nome funcional | Plano base | Products permitidos | Wallet | Regra de acesso |
| --- | --- | --- | --- | --- | --- |
| `CREDIT_STRICT` | recorrente de créditos estrita | obrigatório, com concessão de créditos após pagamento confirmado | somente `CREDIT_METERED` | saldo é verificado durante o período ativo | bloqueia todos os Products mesmo com créditos restantes |
| `CREDIT_FLEXIBLE` | recorrente de créditos flexível | obrigatório, com concessão de créditos após pagamento confirmado | `CREDIT_METERED` e/ou `ENTITLEMENT_ONLY` | saldo é verificado somente por cada Product medido | após falha, Products sem medição continuam; Products medidos exigem `balance_credit_units > 0` |
| `ENTITLEMENT_ONLY` | somente por entitlement | Plan com zero créditos; Subscription `RECURRING` ou `NON_RENEWING` | somente `ENTITLEMENT_ONLY` | não consulta saldo; cada pagamento gera lançamento zero na customer wallet | recorrente bloqueia se não renovar; não renovável ativa sem vencimento |

Exemplos de decisão:

- `CREDIT_STRICT`: a Wallet ainda tem 40 créditos, mas a renovação falhou após todas as tentativas; `access_allowed = false` por `SUBSCRIPTION_PAYMENT_REQUIRED`.
- `CREDIT_FLEXIBLE`: a renovação falhou e a Wallet tem 40 créditos; Products medidos permanecem disponíveis até o saldo deixar de ser positivo, enquanto Products `ENTITLEMENT_ONLY` continuam disponíveis até cancelamento. Não há concessão de novos créditos sem pagamento.
- `ENTITLEMENT_ONLY` recorrente: a renovação está confirmada; `access_allowed = true` sem ler Wallet. Se falhar definitivamente, `access_allowed = false`.
- `ENTITLEMENT_ONLY` vitalício: o único pagamento é confirmado, ativa sem crédito e persiste `next_renewal_at = null` e `entitlement_ends_at = null`; não existe scheduler de renovação.

#### Composição híbrida de `CREDIT_FLEXIBLE`

“Híbrida” é somente um caso de uso de `CREDIT_FLEXIBLE`, não enum, entidade, estado ou quarto perfil. Ela ocorre quando a mesma oferta flexível inclui ao menos um Product `CREDIT_METERED` e um `ENTITLEMENT_ONLY`. A API aplica a regra de cada Product individualmente: o primeiro depende de saldo; o segundo depende apenas do entitlement persistente até cancelamento. Uma oferta `CREDIT_FLEXIBLE` contendo apenas Products medidos continua sendo o mesmo modelo.

Cada `CustomerSubscription` referencia exatamente um `SubscriptionPlanVersion` vigente usado em sua ativação e, se recorrente, em cada renovação. O mesmo Plan descreve preço e créditos; a Subscription decide se ele será cobrado novamente. Outros Plans da oferta podem ser comprados sob demanda para créditos extras, sem alterar ou quitar a renovação.

Cada `SubscriptionPlanVersion` é imutável depois de publicado e define:

- `plan_version_id`, `offering_id`, nome e vigência;
- preço comercial em unidade monetária inteira mínima e `currency` ISO 4217; esses campos pertencem somente ao catálogo comercial, nunca à Wallet;
- `granted_credit_units`, inteiro não negativo aplicado a cada pagamento confirmado que use essa versão do Plan;
- conjunto versionado de `product_id` habilitados;
- estado `DRAFT`, `SCHEDULED`, `ACTIVE` ou `RETIRED`.

`CustomerSubscription` representa a adesão do customer a uma versão do plano:

- `subscription_id`, `customer_id`, `plan_version_id`;
- `renewal_mode`, enum `RECURRING` ou `NON_RENEWING`;
- em `RECURRING`, regra declarativa, âncora UTC, máximo de tentativas, intervalos, janela final e consequência da falha;
- `unexpected_payment_policy`, enum `MANUAL_REVIEW` ou `AUTO_REFUND`, versionada e copiada para cada solicitação de cobrança;
- estado comercial derivado de eventos `PENDING`, `ACTIVE_PAID`, `PAST_DUE`, `PAUSED`, `EXPIRED` ou `CANCELED`;
- início, fim opcional e referências comerciais externas;
- próximo período somente em `RECURRING` e histórico append-only de ativação, pausa, retomada, troca de plano e cancelamento.

Subscription é camada de regra e entitlement. Ela informa o modelo comercial, a recorrência, a concessão prevista e os Products permitidos, mas nunca executa cobrança. Um domínio de Billing cobra e confirma o pagamento. Somente uma confirmação confiável pode solicitar a transação `SUBSCRIPTION_PAYMENT` na `customer_wallet`, com o delta persistido pelo plano, inclusive zero.

`PaymentCreditGrant` liga toda confirmação de pagamento ao lançamento da Wallet, inclusive quando o plano concede zero. Possui chave única `(subscription_id, billing_period_start_or_purchase_id)` e também `billing_payment_id` globalmente único. Ele persiste o instantâneo de `plan_version_id`, período/compra, `granted_credit_units >= 0`, Products contratados e referencia exatamente uma `CustomerWalletEntry` `SUBSCRIPTION_PAYMENT` com o mesmo delta. Uma segunda chamada externa para aplicar a confirmação retorna `409` e não repete o lançamento; eventos internos duplicados convergem sem novo efeito e são apenas reconhecidos pelo consumidor.

Regras:

- Subscription controla entitlement, mas nunca altera saldo diretamente;
- `CREDIT_STRICT` suspende entitlement após falhar a janela de cobrança, mesmo com saldo positivo;
- `CREDIT_FLEXIBLE` preserva entitlement após falha de renovação até cancelamento; em Product medido o acesso ainda exige `balance_credit_units > 0`, enquanto Product sem medição ignora saldo; inadimplência impede novos créditos daquele ciclo;
- `ENTITLEMENT_ONLY` nunca altera o saldo, mas cada pagamento confirmado cria lançamento zero na customer wallet; em `RECURRING` suspende após falhar a renovação, e em `NON_RENEWING` ativa por prazo indeterminado após o pagamento único;
- apenas pagamento confirmado aplica `granted_credit_units` do Plan, inclusive zero;
- o crédito confirmado é aplicado mesmo que a Wallet esteja negativa: `-10 + 20 = +10`;
- falha, pendência, estorno ou ausência de pagamento não geram crédito automaticamente;
- pausa/cancelamento afetam entitlement conforme seu instante efetivo e não revertem créditos anteriores;
- troca de plano cria nova referência versionada; não reescreve períodos, direitos ou concessões anteriores;
- um Product `ENTITLEMENT_ONLY` ignora saldo; um Product `CREDIT_METERED` exige entitlement e saldo elegível;
- Billing não define calendário, política de retries, entitlement ou quantidade de créditos; ele materializa e executa o calendário e os retries declarados pela Subscription.

### 4.11 BillingConnection, CollectionRequest, BillingPayment e WebhookInbox

Nosso sistema é sempre a fonte de verdade da assinatura. Provedores como Stripe, PayPal ou PagBank armazenam/tokenizam o meio de pagamento e processam cobranças iniciadas pelo Billing, sem governar o plano ou calendário local.

O núcleo de Billing é agnóstico a provedor. Um provedor só participa depois que um adaptador implementa `BillingConnector`; “suportar qualquer provedor” significa poder adicionar conectores sem alterar Subscription, Wallet ou a máquina de estados normalizada, não integração automática sem implementação.

Contrato mínimo de `BillingConnector`:

- validar configuração e retornar capacidades do conector/conta;
- criar ou localizar customer externo quando o provedor exigir;
- criar sessão hospedada/SDK de setup e transformar o resultado em `PaymentMethodBinding` tokenizado;
- criar cobrança a partir de `CollectionRequest`, com idempotency key fornecida pelo núcleo;
- consultar cobrança por ID externo para reconciliação ativa;
- normalizar respostas síncronas e webhooks para os mesmos estados/eventos internos;
- cancelar autorização (`VOID`) e solicitar devolução (`REFUND`) quando declarado;
- produzir erro tipado, retryability e IDs externos.

Capacidades declarativas incluem, no mínimo, meios (`CARD`, `PIX`, `BOLETO`, wallet do provedor ou outros), `supports_setup_session`, `supports_vault`, `supports_off_session_charge`, `supports_payment_query`, `supports_webhook`, `supports_void`, `supports_full_refund`, `supports_partial_refund` e necessidade de ação do customer. O núcleo valida a operação contra essas capacidades antes de chamar o adaptador; ausência de capacidade nunca é tratada como sucesso.

Para Pix, boleto e meios semelhantes, a resposta normalizada pode expor instruções de pagamento, como QR Code, código copiável, linha digitável e vencimento, sem tornar esses campos parte de Subscription.

- `BillingConnection`: tipo de adaptador, customer/merchant externo, capacidades como `supports_void`, `supports_refund`, meios suportados e configuração de webhook;
- `PaymentMethodBinding`: token/ID opaco do meio salvo no provedor, consentimento/mandato e estado;
- `CollectionRequest`: cobrança criada pelo motor de Subscription, única por assinatura + período/compra, com valor/moeda do snapshot do Plan e snapshot de `unexpected_payment_policy`;
- `CollectionAttempt`: execução individual e imutável de uma `CollectionRequest`, numerada desde 1, com `scheduled_at`, início/fim, conector, meio, chave idempotente, resultado normalizado e próxima ação; uma nova tentativa nunca sobrescreve a anterior;
- `BillingPayment`: pagamento normalizado associado à solicitação e à tentativa que o originou, com estados `PENDING`, `REQUIRES_ACTION`, `CONFIRMED`, `FAILED` ou `CANCELED`;
- `WebhookInbox`: evento bruto imutável, ID externo único, instante recebido/processado e resultado;
- `ExternalSubscriptionBinding`: correlação técnica opcional; nunca torna a assinatura externa fonte de verdade.

`UnmatchedPaymentCase` representa pagamento confirmado que não corresponde de forma inequívoca a uma `CollectionRequest` pendente. Guarda provedor, pagamento, valor/moeda, motivo tipado, assinatura candidata opcional, evidências, estado `OPEN`, `RECONCILED`, `REFUND_PENDING`, `REFUNDED` ou `CLOSED_WITH_JUSTIFICATION`, ator e histórico imutável.

`RefundRequest` representa devolução idempotente e referencia o caso, `BillingPayment`, valor, moeda, motivo, capacidade usada (`VOID` antes de captura ou `REFUND` depois), IDs externos e estados `PENDING`, `PROCESSING`, `COMPLETED` ou `FAILED`. Unicidade impede devolver duas vezes o mesmo valor do mesmo pagamento; a soma devolvida nunca excede o capturado.

Resposta síncrona e webhook alimentam a mesma máquina de estados. Mesmo uma confirmação imediata é normalizada como o mesmo evento interno. Eventos duplicados ou fora de ordem não podem repetir concessão. Somente a primeira transição válida para `CONFIRMED` pode criar `PaymentCreditGrant`; falha agenda nova tentativa conforme a política persistida na assinatura. Billing não concede crédito diretamente: ele solicita a transação idempotente da `customer_wallet`.

#### 4.11.1 Orquestração durável de cobranças e retentativas

`CustomerSubscription` é dona da intenção comercial: regra e âncora de recorrência, janela de renovação e política de retentativas, incluindo quantidade máxima, intervalos declarativos e condição de encerramento. Ao abrir um período, esses valores são copiados para a `CollectionRequest`; alterações posteriores na assinatura só afetam períodos futuros.

Billing é dono da execução operacional. Um scheduler durável encontra solicitações vencidas por `next_action_at`, adquire lease/lock, cria a próxima `CollectionAttempt` e chama o conector com uma chave idempotente estável. Reinício do processo, execução concorrente do scheduler ou timeout externo não pode criar duas tentativas lógicas para o mesmo número nem duas cobranças efetivas. A unicidade mínima é `(collection_request_id, attempt_number)` e a chave enviada ao provedor deriva dessa identidade.

Cada `CollectionRequest` expõe o progresso completo: total permitido, tentativas realizadas, tentativas restantes, última falha normalizada, `next_action_at` e estado `SCHEDULED`, `COLLECTING`, `AWAITING_CUSTOMER`, `PAID`, `RETRY_SCHEDULED`, `EXHAUSTED`, `CANCELED` ou `UNMATCHED`. Falhas classificadas como definitivas não são repetidas automaticamente; falhas transitórias seguem o snapshot da política. Meios que dependem de ação do customer, como Pix ou boleto, podem gerar nova instrução ou apenas aguardar vencimento conforme a capacidade e a regra do conector.

Quando todas as tentativas se esgotam, Billing marca a cobrança como `EXHAUSTED`, publica o fato e solicita à Subscription a transição prevista pelo modelo contratado. Ele não decide por conta própria se o entitlement será suspenso ou preservado. Uma confirmação válida antes do encerramento cancela jobs futuros; confirmação posterior passa pela conciliação e não renova automaticamente outro período.

#### 4.11.2 Eventos de Billing e fronteira com Notifications

Billing mantém uma outbox transacional e publica eventos versionados depois de persistir a mudança de estado. O conjunto mínimo inclui `collection.created`, `collection.attempt_scheduled`, `collection.attempt_started`, `collection.awaiting_customer`, `collection.attempt_failed`, `collection.retry_scheduled`, `collection.exhausted`, `payment.confirmed`, `payment.unmatched`, `refund.requested` e `refund.completed`. Cada evento contém IDs correlatos, workspace/customer, assinatura e período quando aplicáveis, estado, motivo tipado e instante.

Regra normativa de cobertura: toda ação financeira ou integração externa observável do Billing produz pelo menos um evento de intenção antes do efeito externo e exatamente um resultado terminal correspondente depois, sem exigir eventos para cada passo puramente interno. Pares mínimos incluem cobrança `attempt.requested` → `attempt.succeeded|failed|pending`, consulta `reconciliation.requested` → `reconciliation.matched|unmatched|inconclusive`, concessão `credit_grant.requested` → `credit_grant.applied|failed`, e devolução `refund.requested` → `refund.completed|failed`. Estados intermediários podem gerar eventos adicionais, mas nunca substituem o resultado terminal.

Todos os eventos usam um envelope comum com `billing_event_id`, `event_type`, `schema_version`, `occurred_at`, `workspace_id`, `correlation_id`, `causation_id`, identificadores da entidade/agregado, `attempt_number` quando aplicável e payload tipado. `correlation_id` acompanha toda a jornada da cobrança; `causation_id` aponta para o evento/comando que provocou o próximo passo. A unicidade de `billing_event_id` e a outbox transacional tornam a publicação ao menos uma vez, portanto consumidores devem deduplicar. A ordem é garantida somente dentro do mesmo agregado; consumidores não devem presumir ordem global.

O catálogo completo, os destinos e o transporte dos eventos — broker, webhook de saída, API de polling ou combinação — serão detalhados em contrato próprio. Esta pendência não altera a obrigação atual de registrar eventos duráveis, versionados e correlacionáveis em cada fronteira de ação do Billing.

`collection.attempt_scheduled` é o fato apropriado para avisar antecipadamente que uma cobrança será tentada, com `attempt_number` e `scheduled_at`; ele pode ser emitido na criação da tentativa ou nos offsets de aviso declarados pela Subscription. `collection.attempt_started` significa que Billing efetivamente iniciou a chamada ao provedor e não deve ser usado como promessa antecipada. A política pode declarar `pre_charge_notice_offsets`, mas Notifications decide canal e conteúdo.

Após iniciar uma tentativa, Billing classifica a resposta em três grupos. Uma confirmação terminal conclui a cobrança e cancela jobs restantes. Uma falha terminal, como recusa confirmada, persiste o motivo tipado e agenda a próxima tentativa permitida. Um resultado `PENDING` ou `REQUIRES_ACTION` não é tratado como falha apenas porque o pagamento ainda não apareceu: Billing aguarda webhook até o prazo, pode consultar o provedor quando suportado e só então decide falha, expiração ou nova tentativa. O scheduler trabalha sobre `next_action_at` persistido; ele não infere ausência de pagamento apenas pela passagem do cron.

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
- referências tipadas à transação original, Vale, Plano, pagamento, incidente ou outra origem quando aplicável;
- identidade do solicitante, aprovador quando exigido, executor, observação opcional de execução e timestamps;
- `compensation_batch_id` opcional;
- estados `DRAFT`, `PENDING_APPROVAL`, `READY`, `EXECUTED`, `CANCELED` ou `REJECTED`.

Criar, editar enquanto `DRAFT`, submeter, aprovar quando aplicável e executar são ações separadas e auditadas. Na submissão, um tipo sem aprovação obrigatória transita diretamente para `READY`; um tipo que exige aprovação transita para `PENDING_APPROVAL` e somente uma ação explícita de aprovação o leva a `READY`. Apenas `READY` pode ser executada. A execução exige `transaction_id` e `Idempotency-Key`, cria exatamente uma entrada `COMPENSATION` na `customer_wallet` com referência oficial à Compensation e muda seu estado para `EXECUTED` no mesmo commit. Nova execução ou reutilização de qualquer das chaves retorna `409`; alteração posterior do valor ou cancelamento depois de executada também são rejeitados.

Compensação pode creditar ou debitar, mas nunca aceita delta zero, nunca edita/apaga lançamento anterior e não pode ser usada como atalho para consumo, pagamento de Plano ou resgate de Vale. `approval_required` controla apenas a passagem de estado do workflow.

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
- `GET /v1/workspaces/{workspace_id}/products/{product_id}/eligibility` — consulta não vinculante que retorna `usage_model`, estado comercial, entitlement efetivo e, somente para `CREDIT_METERED`, suficiência de créditos.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet` — medidor atual de item units, vínculo à customer wallet e próximo bloco.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement?cursor=...` — extrato completo de consumo do item, incluindo chamadas que não emitiram débito.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/statement/{item_wallet_entry_id}` — detalhe e correlação opcional com o débito na customer wallet.
- `GET /v1/workspaces/{workspace_id}/items/{item_id}/item-wallet/pricing-accumulators?price_version_id=...&cursor=...` — acumuladores vitalícios ou por ciclo.

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
  "eligible": false,
  "reasons": ["INSUFFICIENT_CREDIT"],
  "subscription": {
    "commercial_status": "ACTIVE_PAID",
    "subscription_model": "CREDIT_STRICT",
    "entitled": true
  },
  "wallet": {
    "wallet_type": "customer_wallet",
    "balance_credit_units": "-10",
    "version": "42"
  },
  "evaluated_at": "2026-08-25T20:00:00Z"
}
```

Razões iniciais: `PRODUCT_INACTIVE`, `SUBSCRIPTION_REQUIRED`, `SUBSCRIPTION_PAYMENT_REQUIRED`, `SUBSCRIPTION_CANCELED`, `PRODUCT_NOT_ENTITLED` e, apenas em `CREDIT_METERED`, `INSUFFICIENT_CREDIT`. Saldo igual a zero é elegível pela regra de créditos; saldo negativo não é. Em `ENTITLEMENT_ONLY`, `wallet` e `credit_sufficient` são nulos/omitidos. A resposta é informativa e pode mudar antes do consumo.

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

Toda chamada inédita válida cria `UsageEvent` e `ItemWalletEntry`. Se não completar bloco, retorna `201 Created` sem `Debit` e sem `CustomerWalletEntry`. Se completar bloco e `balance_after_credit_units` for zero ou positivo, cria o débito global e retorna `201`. Se completar bloco e o saldo da `customer_wallet` ficar negativo, persiste ambos os extratos e retorna `402 Payment Required`.

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
- `POST /v1/subscription-offerings` e `POST /v1/subscription-offerings/{id}/plans` — catálogo administrativo e versões de planos.
- `GET /v1/subscription-offerings/{id}` e `GET /v1/subscription-plans/{id}` — a oferta expõe modelos/Products permitidos; o Plan expõe preço comercial e `granted_credit_units`, sem recorrência.
- `POST /v1/workspaces/{workspace_id}/subscriptions` — adere a uma versão de Plan e define `renewal_mode`; quando recorrente, recebe regra, âncora e política de retentativas. Não cobra nem credita.
- `GET /v1/workspaces/{workspace_id}/subscriptions/{id}`;
- `POST /v1/workspaces/{workspace_id}/subscriptions/{id}/pause`;
- `POST /v1/workspaces/{workspace_id}/subscriptions/{id}/resume`;
- `POST /v1/workspaces/{workspace_id}/subscriptions/{id}/cancel`;
- `POST /v1/workspaces/{workspace_id}/subscriptions/{id}/payment-credit-grants` — integração interna do Billing após pagamento confirmado; exige `transaction_id`, `billing_payment_id` e período, mas usa `granted_credit_units` persistido no plano versionado.
- `POST /v1/workspaces/{workspace_id}/billing-connections` e `GET /v1/workspaces/{workspace_id}/billing-connections/{id}` — configura adaptador e expõe endpoint de webhook;
- `GET /v1/workspaces/{workspace_id}/billing-connections/{id}/capabilities` — expõe meios e operações efetivamente suportados pela conexão;
- `POST /v1/workspaces/{workspace_id}/billing-connections/{id}/payment-method-setup-sessions` — inicia tokenização, hosted fields ou checkout do provedor e devolve somente artefatos públicos necessários ao cliente;
- `POST /v1/workspaces/{workspace_id}/payment-method-bindings` e `GET /v1/workspaces/{workspace_id}/payment-method-bindings` — conclui/lista vínculos entre customer, conexão e, opcionalmente, assinatura;
- `POST /v1/workspaces/{workspace_id}/subscriptions/{id}/collection-requests` — cria/retorna solicitação idempotente; normalmente responde `202`;
- `GET /v1/workspaces/{workspace_id}/collection-requests/{id}` — estado normalizado, tentativas, ação requerida e, quando aplicável, instruções seguras de Pix, boleto ou outro meio assíncrono;
- `POST /v1/billing/webhooks/{connection_id}` — entrada de eventos por conexão;
- `GET /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments?cursor=...` — fila operacional com motivo, evidências, capacidade de devolução e assinatura candidata;
- `POST /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments/{id}/reconcile` — vincula a uma `CollectionRequest` válida após revalidação completa;
- `POST /v1/admin/workspaces/{workspace_id}/billing/unmatched-payments/{id}/refund` — cria `RefundRequest` idempotente com valor, motivo e aprovação; rejeita quando o conector/meio não possui capacidade;
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
- Plan define preço comercial, `granted_credit_units` e Products; CustomerSubscription define se há renovação, calendário e retentativas. A adesão não recebe quantidade livre de créditos;
- confirmação do Billing não recebe quantidade autoritativa: a API usa `granted_credit_units` da versão contratada e valida pagamento/período por referências idempotentes;
- criação administrativa de vale recebe `credit_units` estritamente positivo persistido no vale;
- resgate de vale/cupom não recebe quantidade: usa `credit_units` do vale selecionado;
- Compensation recebe `signed_credit_units` diferente de zero, tipo, descrição obrigatória, referências e ator; aprovação é exigida apenas pela política aplicada. Somente o endpoint de execução recebe `transaction_id` e movimenta a Wallet.

Contratos de Wallet, consumo, vale, cupom e Compensation não aceitam moeda nem preço comercial. Apenas o catálogo `SubscriptionPlanVersion` armazena o preço monetário do plano, separado da Wallet. Caso uma compra direta externa origine créditos, o serviço comercial decide a quantidade antes de chamar esta API; no caso recorrente, a quantidade vem obrigatoriamente da versão do plano associada ao período confirmado.

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
- `422`: quantidade, `credit_units`, faixas, período ou transição de estado inválidos;
- `503 wallet_not_provisioned`: hierarquia materializada ausente/incompleta; nenhuma wallet ou entrada é criada pelo endpoint que detectou a falha;
- `402`: somente recibo de débito persistido cujo `balance_after_credit_units` ficou negativo.

Nunca usar `402` para cupom esgotado, assinatura desabilitada ou falha técnica.

`POST /usage-events` é permitido apenas para Product `CREDIT_METERED`. Product `ENTITLEMENT_ONLY` usa somente a consulta de elegibilidade; tentativa de registrar consumo retorna `409 product_not_metered` sem criar evento ou Wallet.

## 6. Fluxos transacionais

### 6.1 Registro de consumo e débito por bloco

1. Validar a forma do `workspace_id` e a existência do workspace.
2. Validar forma básica sem consultar saldo.
3. Abrir transação de banco e tentar inserir `IdempotencyRecord(customer_id, idempotency_key)` e reservar `(customer_id, transaction_id)`. Restrições únicas fazem uma chamada concorrente aguardar o commit ou rollback da dona das chaves.
4. Se qualquer chave já existir após a espera, devolver o `409` específico sem executar o domínio nem reproduzir a resposta original.
5. Validar produto/item, aplicabilidade ao cluster/escopo, entitlement do Product em uma `CustomerSubscription` ativa e estado `ACTIVE` da `customer_wallet` e `item_wallet` já materializadas. Product fora do plano retorna `403 product_not_entitled` sem registrar uso; Wallet ausente retorna `503 wallet_not_provisioned`. Este fluxo nunca cria Wallet.
6. Bloquear a `ItemWallet` de `(customer_id, item_id)` e ler seu estado mais recente. Nesse instante, obter `accepted_at` do relógio do banco; essa ordem serial define versão ativa e fronteira de ciclo.
7. Resolver a versão de preço ativa em `accepted_at` e conferir `expected_price_version_id`, quando enviado.
8. Atribuir ao evento o intervalo cumulativo de item units e somar `item_units` uma única vez.
9. Primeiro completar, se existir, o bloco pendente preso à versão antiga. Para a quantidade restante, usar a versão ativa validada. Calcular `converted_blocks`, `converted_item_units` e `pending_item_units_after` com divisão inteira e resto.
10. Para cada bloco completo, resolver `cycle_key` pela regra declarativa da respectiva versão em `accepted_at`. Bloquear os `PricingAccumulator` tocados em ordem lexicográfica de `(price_version_id, cycle_key)`, aplicar a faixa do intervalo acumulado e inserir um `BillingBlock` imutável.
11. Inserir `UsageEvent` e exatamente uma `ItemWalletEntry`, atualizar contadores/pending da `ItemWallet` e associar os `BillingBlock` formados.
12. Se nenhum bloco foi completado, persistir o resultado e as chaves de deduplicação com `customer_wallet_entry_id = null`; não bloquear nem criar entrada na `CustomerWallet`.
13. Se houve blocos, bloquear a `CustomerWallet` pai já existente, somar `credit_units` dos blocos, inserir um único `Debit` e `CustomerWalletEntry`, atualizar `balance_credit_units` e ligar ambos os extratos aos mesmos blocos.
14. Persistir `PricingAccumulator`, vínculos e resultado da operação no mesmo commit.
15. Responder `201` quando não houve débito ou quando `balance_after_credit_units >= 0`; responder `402` somente quando houve débito e `balance_after_credit_units < 0`.

Não existe commit intermediário entre sensibilizar a `item_wallet` e, quando houver bloco, debitar a `customer_wallet`. Uma falha reverte evento, entrada do item, pendente, bloco, entrada de créditos e saldo. A consulta prévia de elegibilidade e as leituras dos extratos não participam desta transação.

### 6.2 Crédito direto e Compensation

Crédito direto segue a ordem: idempotência, validação da flag, bloqueio da `CustomerWallet`, `CustomerWalletEntry`, saldo de créditos, recibo e commit.

Compensation possui duas fases. Criar/submeter/aprovar persiste somente o recurso e sua auditoria, sem bloquear ou movimentar a Wallet. Executar uma Compensation aprovada valida idempotência e estado, bloqueia a `CustomerWallet`, cria a única `CustomerWalletEntry`, atualiza o saldo, grava referências/recibo e marca `EXECUTED` no mesmo commit. Nenhuma dessas fontes toca `ItemWallet`.

### 6.3 Crédito recorrente após pagamento confirmado

1. O motor de Subscription encontra período devido e cria uma única `CollectionRequest` por assinatura + período, usando o snapshot do plano.
2. Um trabalhador cria `BillingPayment` e chama o adaptador com chave idempotente. A resposta pode ser pendente, exigir ação, confirmar ou falhar.
3. A resposta síncrona é transformada no mesmo evento normalizado usado por webhook; o request HTTP não concede crédito por um caminho especial.
4. O webhook entra em `WebhookInbox`, é deduplicado pelo ID do provedor e tenta avançar o mesmo `BillingPayment`; eventos antigos não regressam estado terminal.
5. Em Subscription `RECURRING`, falha reprograma nova tentativa segundo sua política persistida. Exaustão bloqueia `CREDIT_STRICT`/`ENTITLEMENT_ONLY`; em `CREDIT_FLEXIBLE`, mantém entitlement e deixa cada Product aplicar sua regra de saldo. Subscription `NON_RENEWING` não agenda renovação.
6. Na primeira transição válida para `CONFIRMED`, validar assinatura, versão do Plan, valor, moeda e período/compra; ler `granted_credit_units` persistido no Plan, inclusive zero.
7. Inserir `PaymentCreditGrant`, bloquear a `CustomerWallet` e criar `CustomerWalletEntry` `SUBSCRIPTION_PAYMENT`. Com delta positivo, atualizar o saldo mesmo que estivesse negativo; com delta zero, persistir `balance_before_credit_units = balance_after_credit_units` sem alterar a projeção numérica.
8. Ativar/renovar entitlement e, em `ENTITLEMENT_ONLY + NON_RENEWING`, persistir ausência de próximo vencimento. Confirmar pagamento, grant, lançamento, saldo/projeção e recibo idempotente de forma atômica ou por saga com outbox e consumidor idempotente, sem janela capaz de lançar duas vezes.

O vencimento apenas cria intenção de cobrança. Respostas, webhooks e retries podem chegar pelo menos uma vez, mas as chaves naturais garantem uma única confirmação e exatamente um lançamento de assinatura observável, positivo ou zero conforme o plano.

#### Pagamento inesperado, conciliação e devolução

1. Toda confirmação deve corresponder a uma `CollectionRequest` criada pelo nosso sistema, com assinatura, período/compra, Plan, valor, moeda e provedor compatíveis.
2. Evento duplicado é reconhecido sem novo efeito. Evento atrasado de uma solicitação legítima é conciliado pelo ID, não pela hora de chegada.
3. Sem correspondência inequívoca, criar/atualizar `UnmatchedPaymentCase`; não renovar, não ativar entitlement e não lançar crédito na Wallet.
4. Tentar conciliação determinística antes de qualquer devolução. Nunca inferir renovação apenas por proximidade de data ou assinatura candidata.
5. Com política `MANUAL_REVIEW`, aguardar ação manual de reconciliar, devolver ou encerrar com justificativa.
6. Com `AUTO_REFUND`, somente após classificar definitivamente como indevido e confirmar capacidade do conector/meio, criar `RefundRequest` idempotente. Usar `VOID` se ainda autorizado e não capturado; usar `REFUND` se capturado.
7. Se o meio não suportar devolução automática, manter o caso aberto para procedimento manual; não simular sucesso.
8. Resultado de devolução é assíncrono, passa pela mesma normalização/deduplicação de eventos e preserva IDs externos.
9. Se um pagamento aplicado for posteriormente considerado indevido, estornar no provedor e criar lançamento compensatório na Wallet; nunca editar ou apagar o lançamento original.

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

- débitos aceitos por status `201` e `402`;
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
- com bloco 1 e custo de 7 créditos, saldo `5` após uma unidade: entrada criada, saldo `-2`, HTTP `402`, não elegível por crédito;
- com bloco 1 e custo de 7 créditos, saldo `-2` após uma unidade: nova entrada criada, saldo `-9`, HTTP `402`;
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
- criar ou ativar assinatura não movimenta a Wallet;
- Product fora do plano ativo retorna `403 product_not_entitled` sem registrar consumo, mesmo que haja saldo;
- elegibilidade expõe separadamente `usage_model`, `commercial_status`, `subscription_entitled`, `credit_sufficient` quando aplicável, `access_allowed` e motivo;
- `CREDIT_STRICT` aceita somente `CREDIT_METERED`, exige Subscription `RECURRING` com Plan de créditos e bloqueia após falha definitiva mesmo com saldo;
- `CREDIT_FLEXIBLE` aceita mistura de `CREDIT_METERED` e `ENTITLEMENT_ONLY`, exige Subscription `RECURRING` com Plan de créditos e, após falha, mantém Products sem medição enquanto os medidos exigem `balance_credit_units > 0`;
- `ENTITLEMENT_ONLY` aceita somente Products do mesmo `usage_model` e Plan com `granted_credit_units = 0`; pagamento confirmado cria `SUBSCRIPTION_PAYMENT` de delta zero, sem mudar saldo;
- `ENTITLEMENT_ONLY + RECURRING` mantém acesso por renovação confirmada e bloqueia após falha definitiva;
- `ENTITLEMENT_ONLY + NON_RENEWING` ativa somente após o pagamento único, deixa `next_renewal_at`/`entitlement_ends_at` nulos e não cria cobrança futura;
- publicação rejeita oferta sem plano base compatível, Product incompatível com o perfil ou política de crédito divergente; mistura dos dois `usage_model` é válida somente em `CREDIT_FLEXIBLE`;
- apenas confirmação interna correlacionada de pagamento cria `PaymentCreditGrant` e `SUBSCRIPTION_PAYMENT`; plano sem créditos cria exatamente um lançamento zero correlacionado;
- `signed_credit_units = 0` é aceito somente nesse `SUBSCRIPTION_PAYMENT`; crédito direto, Vale, consumo e Compensation com zero são rejeitados sem lançamento;
- saldo `-10` mais concessão recorrente de `20 credit_units` resulta `10`;
- concessão direta usa a quantidade solicitada, Vale usa a quantidade persistida e Cupom apenas seleciona o Vale; recorrência usa exclusivamente `granted_credit_units` da versão do plano confirmada;
- dez entregas concorrentes da mesma confirmação/período geram uma concessão e uma entrada;
- mesmo `billing_payment_id` ou mesmo período não pode conceder créditos duas vezes;
- resposta síncrona confirmada e webhook duplicado convergem para o mesmo `BillingPayment` e uma única concessão;
- adicionar um novo provedor exige somente um `BillingConnector` compatível; não altera contratos, estados ou invariantes de Subscription e Wallet;
- o núcleo rejeita uma operação antes da chamada externa quando o conector não declara a capacidade ou o meio solicitado;
- uma `CollectionRequest` usa o mesmo contrato normalizado para cartão, Pix, boleto ou outro meio, variando apenas capacidades e instruções retornadas pelo conector;
- `PaymentMethodBinding` pertence ao mesmo workspace, customer e `BillingConnection` da cobrança; vínculo opcional com assinatura não permite uso cruzado;
- confirmação síncrona, consulta posterior e webhook convergem para o mesmo estado terminal e não duplicam cobrança, renovação ou lançamento;
- webhook inválido não altera cobrança; webhook antigo não regride estado terminal; evento válido pode ser reprocessado pela `WebhookInbox`;
- retries obedecem à política copiada para a assinatura, e Billing não altera calendário ou quantidade de tentativas;
- pagamento confirmado sem `CollectionRequest` compatível cria `UnmatchedPaymentCase` e não renova, não ativa e não lança créditos;
- webhook atrasado de cobrança legítima é conciliado por IDs mesmo fora da janela cronológica esperada;
- `AUTO_REFUND` só cria devolução após conciliação negativa definitiva e quando `BillingConnection` declara `supports_void`/`supports_refund` para o meio;
- meio sem devolução automática mantém caso para revisão; a API não retorna devolução concluída fictícia;
- dois cliques, retries ou webhooks para a mesma devolução resultam em uma única `RefundRequest` efetiva e nunca excedem o valor capturado;
- devolver pagamento já aplicado cria compensação na Wallet sem editar o lançamento original;
- pausa, retomada, troca de plano e cancelamento preservam histórico e entitlement por vigência;
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
6. **Item wallet e consumo:** `ItemWalletEntry`, `PricingAccumulator`, `BillingBlock`, extratos correlatos, contrato `201/402`, pendentes e testes concorrentes.
7. **Crédito direto e configuração:** flags por workspace e fronteira com confirmação externa.
8. **Subscription e Billing:** ofertas, Plans com preço/créditos, Subscription `RECURRING`/`NON_RENEWING`, entitlement, cobrança/retries locais, conexões/adaptadores, tokenização referenciada, `CollectionRequest`, `BillingPayment`, `WebhookInbox` e `PaymentCreditGrant` idempotente.
9. **Vales:** ciclo de vida, resgate direto e auditoria promocional.
10. **Cupons:** lote, seleção concorrente, modo livre e limite por identificador externo.
11. **Compensações administrativas:** lifecycle de intenção/aprovação/execução, auditoria de atores e lançamento idempotente.
12. **Operação:** outbox, métricas, reconciliação de wallets/extratos, testes de carga/lock e documentação OpenAPI.

Cada etapa deve terminar com migrações reversíveis, contrato OpenAPI atualizado, testes unitários e de integração, cenários concorrentes no banco real e evidência de que os invariantes anteriores continuam válidos. Consumo não começa antes do provisionamento materializado e da verificação de entitlement; concessão por pagamento confirmado, vale e cupom não começam antes de `CustomerWalletEntry`/idempotência passarem nos testes de concorrência.

## 11. Pontos a ratificar antes da implementação

O plano adota defaults definidos para não bloquear o desenho, mas há decisões e detalhamentos que devem ser fechados antes de codificar seus módulos:

1. **Eventos de domínio e entrega:** catalogar eventos correlacionados para toda ação e transição de estado relevante de Product, Price, Wallet, Subscription, Billing, Vale, Cupom e Compensation; definir pares antes/depois quando houver efeito externo, schemas/versionamento, ordem por agregado, outbox, retry, deduplicação, dead-letter, retenção e transporte por broker, webhook de saída, polling ou combinação. Também definir cadastro de destinos, filtros, replay operacional e observabilidade de entrega.
2. **Pausa, retomada, troca e cancelamento:** definir instante efetivo, efeito no período já pago, entitlement, cobrança pendente, saldo remanescente, Products habilitados, Price/Plan versionados, prorrata ou ausência dela e eventos emitidos para cada transição.
3. **Consultas, relatórios e métricas:** separar consultas operacionais de recursos individuais, relatórios agregados exportáveis e métricas técnicas. Definir filtros, paginação, consistência temporal, exportação e retenção para extratos, consumo, saldo, concessões, assinaturas, cobranças, falhas, Vales/Cupons e Compensations. Métricas são séries agregadas para operação; relatórios são dados de negócio consultáveis/exportáveis e não substituem o razão.
4. **Rotinas de reconciliação:** detalhar jobs, frequência, cursores, tolerâncias, alertas e reparos para comparar saldo com extrato, item units com blocos/pendente, PaymentCreditGrant com pagamentos, assinatura com cobrança, estoque de Vales com resgates e Compensations com lançamentos. Divergência nunca permite edição destrutiva; reparo usa reprocessamento idempotente ou lançamento compensatório.
5. **Tarefa de desenho de meios de pagamento do Billing:** especificar separadamente o fluxo completo de cada meio inicialmente suportado, como cartão, Pix, boleto e wallet de provedor. Para cada meio, definir criação/captura, tokenização ou instrução de pagamento, vínculo com customer e assinatura, mandato para cobrança recorrente/off-session, estados síncronos e assíncronos, expiração, consulta, webhook, conciliação, retentativa, cancelamento, devolução, capacidades exigidas do conector, eventos antes/depois e respostas HTTP. Também deve ser decidido o contrato definitivo entre `BillingConnection`, rotas aceitas pela oferta, instrumento tokenizado do customer e vínculo escolhido pela assinatura. A proposta discutida nesta revisão é referência de trabalho, não contrato fechado.

Decisão ratificada nesta revisão: todos os Vales vinculados ao mesmo Cupom devem possuir exatamente a mesma quantidade de `credit_units`. A publicação/vinculação rejeita lote heterogêneo, e alterações posteriores não podem quebrar essa homogeneidade.

Alterar qualquer uma dessas respostas exige atualizar este documento e os critérios de aceitação antes da implementação; nenhuma delas impede concluir o planejamento atual.
