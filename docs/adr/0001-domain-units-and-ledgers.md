# ADR 0001: unidades de domínio e razões imutáveis

## Decisão

Créditos e quantidades são `BIGINT` no PostgreSQL e `i64` no Rust, mas usam
strings decimais no JSON. Toda aritmética usa operações verificadas. Wallet
credits não representam moeda e item units de itens diferentes não são
fungíveis.

Wallets mantêm projeções para leitura eficiente, porém toda mudança financeira
ou de consumo cria antes um lançamento append-only correlacionável. Correções
criam novos lançamentos; não alteram ou apagam o histórico.

## Consequências

- valores fora de `i64` ou com sintaxe não decimal são rejeitados;
- tabelas de razão recebem proteção contra `UPDATE` e `DELETE`;
- saldos, lotes, blocos e acumuladores devem ser reconciliáveis a partir dos
  lançamentos persistidos.
