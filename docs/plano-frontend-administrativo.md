# Plano do painel administrativo

Para a descrição detalhada das páginas, fluxos, contratos, decisões técnicas e
comandos, consulte [Frontend administrativo: funcionalidades e operação](frontend-administrativo.md).

## Decisões

O painel fica em `admin-ui/` e usa Vite, React, TypeScript, shadcn/ui, Tailwind,
Zustand e TanStack Query. O Zustand guarda apenas seleção e filtros locais; as
respostas da API ficam no cache do TanStack Query. O cliente HTTP usa os tipos
gerados do OpenAPI. Crédito e unidades continuam como strings decimais, e datas
UTC são mostradas no fuso do navegador.

O painel não possui login nesta entrega e deve ser acessado apenas em rede local
ou VPN. O Subscription não armazena nomes, usuários ou permissões do Accounts.
Nenhuma referência de ator digitada na UI comprova identidade. Exposição externa
exige autenticação e autorização antes de ocorrer.

## Fases

| Fase                  | Entrega                                                                                                                     | Critério de aceite                                                                                     | Estado                                                                                                                              |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| 0 — Base              | Skill shadcn, frontend Vite, navegação, OpenAPI tipado, documentação                                                        | Frontend inicia e compila; tipos podem ser regenerados                                                 | Concluída                                                                                                                           |
| 1 — Consultas         | Listagem/detalhe de projeções de account e listagem de planos por account, paginadas por cursor                         | Consulta sem SQL manual, sem dados de identidade inventados                                            | Concluída                                                                                                                           |
| 2 — Painel de leitura | Contadores globais, abertura por UUID, estado do account, planos/ciclos, carteira, extrato e consumo por item sob demanda | Operador consegue explicar estado comercial, saldo e consumo; consultas têm carregamento, vazio e erro | Concluída                                                                                                                           |
| 3 — Billing           | Listas e detalhes de cobranças, tentativas, pagamentos, webhooks, outbox e pagamentos não conciliados                       | Cada contador abre seus registros; IDs e horários permitem seguir o runbook                            | Concluída                                                                                                                           |
| 4 — Catálogo          | Listas e formulários de produtos, itens, preços, assinaturas, planos, ofertas avulsas e políticas                           | Nova oferta publicada é conferível; versões publicadas aparecem imutáveis                              | Concluída                                                                                                                           |
| 5 — Ações             | Cancelamento, transição, revogação, crédito direto, configuração, reconciliação e replay                                    | Repetições preservam idempotência; conflito mostra `existing_operation` quando presente                | Concluída                                                                                                                           |
| 6 — Operação          | Auditoria consultável, testes ponta a ponta, acessibilidade e observabilidade                                               | Fluxos críticos executáveis e investigáveis sem SQL manual                                             | Concluída: fluxos críticos testados no navegador com API simulada, axe nas telas de operação e erros registrados em JSON no console |

## Contratos de consulta da primeira entrega

- `GET /v1/admin/accounts?cursor=<uuid>&limit=20`: IDs de account em ordem
  estável; `next_cursor` só existe quando há outra página. Limite permitido: 1–100.
- `GET /v1/admin/accounts/{account_id}`: projeção operacional mínima.
- `GET /v1/admin/accounts/{account_id}/customer-plans?cursor=<uuid>&limit=20`:
  planos daquele account e seus ciclos ativos; account inexistente retorna 404.
- O painel usa também os endpoints existentes de operações de Billing, wallets,
  provisionamento, extrato de créditos, medidor e extrato de uso.

As consultas novas seguem `route → dto → service → repository`, usam o esquema
existente e não acrescentam migração. Os contratos publicados podem ser exportados
sem banco com `cargo run --bin export-openapi`.

## Contratos e operação adicionais

As filas de Billing e catálogo e os eventos de auditoria usam paginação por UUID,
limite de 1–100 e tipos gerados pelo OpenAPI. A investigação de Billing mostra
IDs de cobrança, tentativa, pagamento, evento do provedor e correlação quando
presentes. A UI usa o código estruturado de erro, inclusive
`existing_operation` em conflitos.

As ações de crédito e transição guardam a chave de idempotência por escopo e
transação no armazenamento local até a API confirmar o resultado. Uma nova
transação recebe outra chave. Se o operador trocar de navegador, essa chave não
acompanha a sessão; use o `transaction_id` para investigar antes de repetir.

## Limites atuais

- A observabilidade escreve erros estruturados no console do navegador; ainda não envia dados para um agregador remoto.
- Os testes de navegador simulam as respostas HTTP e verificam os fluxos da interface; os testes de integração Rust verificam os contratos contra PostgreSQL.
- Cupons, vouchers, compensações, estornos iniciados pelo produto e pausa de planos dependem de implementação de backend e não aparecem como ações do painel.
- Login e autorização continuam fora do escopo. Mantenha a API e o painel em rede interna ou VPN.
